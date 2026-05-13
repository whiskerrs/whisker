//! End-to-end build of an example app: cargo cross-compile + jniLibs
//! staging + Lynx AAR unpack + gradle assembleDebug. Produces an
//! installable APK.
//!
//! Composed from the lower-level xtask building blocks
//! (`cargo_build`, `unpack_lynx`).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{cargo_build, ndk, unpack_lynx};
use crate::paths;

#[derive(clap::Args)]
pub struct BuildExampleArgs {
    /// User crate to build. Also used to derive
    /// `examples/<package>/android/` (gradle project) and
    /// `lib<package_underscored>.so` (cdylib filename).
    #[arg(short = 'p', long, default_value = "hello-world")]
    pub package: String,

    #[arg(long, default_value = "arm64-v8a")]
    pub abi: String,

    #[arg(long, default_value_t = 24)]
    pub api: u32,
}

pub fn run(args: BuildExampleArgs) -> Result<()> {
    let root = paths::workspace_root()?;
    let example_dir = root
        .join("examples")
        .join(&args.package)
        .join("android");
    if !example_dir.is_dir() {
        anyhow::bail!(
            "no gradle project at {} (expected examples/<package>/android/)",
            example_dir.display()
        );
    }
    let java_home = resolve_java_home()?;

    // 1. Ensure Lynx AARs are unpacked first. The Rust cdylib's
    //    `build.rs` adds `target/lynx-android-unpacked/jni/<abi>/` as
    //    a `-L` search path so `-llynx` / `-llynxbase` resolve at link
    //    time — so the unpack has to happen *before* cargo build.
    let unpack_dir = root.join("target/lynx-android-unpacked");
    if !unpack_dir.join("jni").join(&args.abi).is_dir() {
        let aar_dir = root.join("target/lynx-android");
        if !has_any_aar(&aar_dir) {
            anyhow::bail!(
                "no Lynx AARs in {}; run `cargo xtask android build-lynx-aar` first",
                aar_dir.display()
            );
        }
        println!("==> Unpacking Lynx AARs");
        unpack_lynx::run_with(&aar_dir, &unpack_dir)?;
    }

    // 2. Rust cdylib.
    println!("==> Building Rust cdylib for {}", args.abi);
    cargo_build::run(cargo_build::CargoBuildArgs {
        package: args.package.clone(),
        abi: args.abi.clone(),
        api: args.api,
        profile: "release".to_string(),
        cargo_args: vec![],
    })?;

    // 3. Drop .so + libc++_shared.so into jniLibs/<abi>/.
    let triple = ndk::abi_to_triple(&args.abi)?;
    let lib_name = format!("lib{}.so", args.package.replace('-', "_"));
    let so_src = root
        .join("target")
        .join(triple)
        .join("release")
        .join(&lib_name);
    if !so_src.is_file() {
        anyhow::bail!("cargo did not produce {}", so_src.display());
    }
    let jni_libs = example_dir
        .join("app/src/main/jniLibs")
        .join(&args.abi);
    std::fs::create_dir_all(&jni_libs)?;
    let so_dst = jni_libs.join(&lib_name);
    println!("==> Copying {lib_name} → app jniLibs");
    std::fs::copy(&so_src, &so_dst)
        .with_context(|| format!("copy {} → {}", so_src.display(), so_dst.display()))?;

    println!("==> Bundling libc++_shared.so from NDK");
    let libcxx = find_libcxx_shared(&args.abi)?;
    std::fs::copy(&libcxx, jni_libs.join("libc++_shared.so"))
        .context("copy libc++_shared.so")?;

    // 4. gradle assembleDebug.
    println!("==> Running gradle :app:assembleDebug");
    run_gradle(&example_dir, &java_home, &[":app:assembleDebug", "--no-daemon"])?;

    let apk = example_dir.join("app/build/outputs/apk/debug/app-debug.apk");
    if apk.is_file() {
        println!("\n✅ APK: {}", apk.display());
    }
    Ok(())
}

fn resolve_java_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("JAVA_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    // Android Studio bundles a JDK (Java 17/21) at a known location on
    // macOS; this is the most reliable choice across machines.
    let candidates = [
        "/Applications/Android Studio.app/Contents/jbr/Contents/Home",
        "/Applications/Android Studio Preview.app/Contents/jbr/Contents/Home",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.is_dir() {
            return Ok(p);
        }
    }
    anyhow::bail!(
        "JAVA_HOME not set and Android Studio JBR not found. \
         Install Android Studio or set JAVA_HOME explicitly."
    )
}

fn has_any_aar(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries
        .flatten()
        .any(|e| e.path().extension().is_some_and(|x| x == "aar"))
}

fn find_libcxx_shared(abi: &str) -> Result<PathBuf> {
    // The libc++_shared.so on aarch64 is ABI-compatible across recent
    // NDK versions, so we accept the first one we find under
    // $ANDROID_HOME/ndk/*/toolchains/llvm/prebuilt/<host>/sysroot/usr/lib/<triple>/.
    let android_home = ndk::android_home()?;
    let ndk_root = android_home.join("ndk");
    let host = ndk::host_tag()?;
    let triple = ndk::abi_to_triple(abi)?;
    for entry in std::fs::read_dir(&ndk_root)
        .with_context(|| format!("read {}", ndk_root.display()))?
    {
        let entry = entry?;
        let candidate = entry
            .path()
            .join("toolchains/llvm/prebuilt")
            .join(host)
            .join("sysroot/usr/lib")
            .join(triple)
            .join("libc++_shared.so");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "libc++_shared.so not found under any NDK in {}",
        ndk_root.display()
    )
}

pub(super) fn run_gradle(dir: &Path, java_home: &Path, gradle_args: &[&str]) -> Result<()> {
    let gradlew = if cfg!(target_os = "windows") {
        dir.join("gradlew.bat")
    } else {
        dir.join("gradlew")
    };
    if !gradlew.is_file() {
        anyhow::bail!("gradle wrapper not found at {}", gradlew.display());
    }
    let prev_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!(
        "{}/bin{}{}",
        java_home.display(),
        if prev_path.is_empty() { "" } else { ":" },
        prev_path
    );

    let status = Command::new(&gradlew)
        .args(gradle_args)
        .current_dir(dir)
        .env("JAVA_HOME", java_home)
        .env("PATH", new_path)
        .status()
        .with_context(|| format!("failed to spawn {}", gradlew.display()))?;
    if !status.success() {
        anyhow::bail!("gradle failed (exit {status})");
    }
    Ok(())
}
