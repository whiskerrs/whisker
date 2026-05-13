//! Build Lynx's Android AARs (LynxBase / LynxTrace / LynxAndroid /
//! ServiceAPI) from a local Lynx source checkout with our visibility
//! patches applied. Result lands in `target/lynx-android/`.
//!
//! We deliberately don't fetch Lynx source ourselves — Lynx has its
//! own bootstrap (`tools/envsetup.sh` + `tools/hab sync`) and we
//! expect the user to put the result under `target/lynx-src/` (or
//! override via `LYNX_SRC` / `--lynx-src`).
//!
//! See `patches/lynx-android/README.md` for why these patches exist.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::build_example;
use tuft_build::paths;

#[derive(clap::Args)]
pub struct BuildLynxAarArgs {
    /// Path to a Lynx source checkout. Default: `$LYNX_SRC` or
    /// `target/lynx-src/` under the workspace.
    #[arg(long)]
    pub lynx_src: Option<PathBuf>,
}

pub fn run(args: BuildLynxAarArgs) -> Result<()> {
    let lynx_src = resolve_lynx_src(args.lynx_src.as_deref())?;
    let java11 = resolve_java11()?;
    let ndk21 = resolve_ndk21()?;

    println!("==> Lynx source: {}", lynx_src.display());
    println!("    JAVA_HOME (11): {}", java11.display());
    println!("    NDK 21:         {}", ndk21.display());

    // 1. Sanity-check the Lynx tree.
    for required in [
        "platform/android/lynx_android",
        "build/config",
    ] {
        if !lynx_src.join(required).is_dir() {
            anyhow::bail!(
                "Lynx source incomplete at {} (missing {required}). \
                 Did you run Lynx's bootstrap (tools/envsetup.sh + tools/hab sync)?",
                lynx_src.display()
            );
        }
    }

    // 2. Apply patches (idempotent).
    println!("==> Applying Tuft patches to Lynx source");
    let patches_dir = paths::workspace_root().join("patches/lynx-android");
    apply_patch(&lynx_src.join("build"), &patches_dir.join("buildroot.patch"))?;
    apply_patch(&lynx_src, &patches_dir.join("lynx.patch"))?;

    // 3. Gradle assemble.
    // Lynx's CMake-driven native build picks the NDK from
    // `ANDROID_NDK_HOME` (or `ANDROID_NDK`); without those set, gradle
    // ends up trying to compile against a non-existent
    // `build/config/None/toolchains/...` sysroot. Push the env into
    // this process so the spawned `gradlew` inherits it.
    // SAFETY: short-lived xtask process, no other threads racing.
    unsafe {
        std::env::set_var("ANDROID_NDK_HOME", &ndk21);
        std::env::set_var("ANDROID_NDK", &ndk21);
    }
    println!("==> Building AARs (this takes a few minutes the first time)");
    let android_dir = lynx_src.join("platform/android");
    build_example::run_gradle(
        &android_dir,
        &java11,
        &[
            "--no-daemon",
            ":LynxBase:assembleNoasanRelease",
            ":LynxTrace:assembleNoasanRelease",
            ":LynxAndroid:assembleNoasanRelease",
            ":ServiceAPI:assembleNoasanRelease",
        ],
    )?;

    // 4. Copy results into target/lynx-android/.
    let dest = paths::lynx_android_aars();
    std::fs::create_dir_all(&dest)?;
    let copies: &[(&str, &str)] = &[
        (
            "base/platform/android/build/outputs/aar/LynxBase-noasan-release.aar",
            "LynxBase.aar",
        ),
        (
            "base/trace/android/build/outputs/aar/LynxTrace-noasan-release.aar",
            "LynxTrace.aar",
        ),
        (
            "platform/android/lynx_android/build/outputs/aar/LynxAndroid-noasan-release.aar",
            "LynxAndroid.aar",
        ),
        (
            "platform/android/service_api/build/outputs/aar/ServiceAPI-noasan-release.aar",
            "ServiceAPI.aar",
        ),
    ];
    println!("==> Copying AARs to {}", dest.display());
    for (src_rel, dst_name) in copies {
        let src = lynx_src.join(src_rel);
        if !src.is_file() {
            anyhow::bail!("expected AAR not produced: {}", src.display());
        }
        std::fs::copy(&src, dest.join(dst_name))
            .with_context(|| format!("copy {} → {}", src.display(), dst_name))?;
        println!("  {dst_name}");
    }

    println!(
        "\n✅ Lynx Android AARs ready at {}\n   Next: cargo xtask android build-example",
        dest.display()
    );
    Ok(())
}

fn resolve_lynx_src(arg: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = arg {
        if p.is_dir() {
            return Ok(p.to_path_buf());
        }
        anyhow::bail!("--lynx-src does not exist: {}", p.display());
    }
    if let Some(p) = std::env::var_os("LYNX_SRC").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    let default = paths::lynx_src_default();
    if default.is_dir() {
        return Ok(default);
    }
    anyhow::bail!(
        "Lynx source not found at {}.\n  \
         Bootstrap Lynx per its docs (e.g. clone + tools/envsetup.sh + \
         tools/hab sync) into that directory, or override with \
         LYNX_SRC=/path/to/lynx (or --lynx-src=/path/to/lynx).",
        default.display()
    )
}

fn resolve_java11() -> Result<PathBuf> {
    // Lynx's gradle wrapper (6.7.1) refuses anything newer than JDK
    // 11. Don't reuse JAVA_HOME if it's set to something newer — the
    // wrapper will fail with "Unsupported class file major version".
    if let Some(p) = std::env::var_os("TUFT_JAVA11_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = &home {
        candidates.push(home.join("work/java11/jdk-11.0.25+9/Contents/Home"));
        candidates.push(home.join("work/java11/jdk-11.0.25+9"));
    }
    candidates.push(PathBuf::from(
        "/Library/Java/JavaVirtualMachines/temurin-11.jdk/Contents/Home",
    ));
    for p in &candidates {
        if p.is_dir() {
            return Ok(p.clone());
        }
    }
    anyhow::bail!(
        "JDK 11 not found. Lynx's gradle 6.7.1 needs it. Install \
         Temurin 11 or set TUFT_JAVA11_HOME."
    )
}

fn resolve_ndk21() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ANDROID_NDK_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    let android_home = super::ndk::android_home()?;
    let cand = android_home.join("ndk/21.1.6352462");
    if cand.is_dir() {
        return Ok(cand);
    }
    anyhow::bail!(
        "NDK 21.1.6352462 not found (Lynx's gn/ninja toolchain \
         requires this exact version). Install via:\n  \
         sdkmanager 'ndk;21.1.6352462'"
    )
}

fn apply_patch(repo: &Path, patch: &Path) -> Result<()> {
    if !patch.is_file() {
        anyhow::bail!("patch file missing: {}", patch.display());
    }
    let name = patch.file_name().and_then(|s| s.to_str()).unwrap_or("?");

    // Already applied?  `git apply --reverse --check` returns 0 if the
    // patch could be reversed cleanly, which is the indicator that it
    // is currently applied.
    let reverse_check = Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
        .args(["apply", "--reverse", "--check"])
        .arg(patch)
        .status()
        .context("failed to spawn git")?;
    if reverse_check.success() {
        println!("  already applied: {name}");
        return Ok(());
    }

    // Not applied — verify it applies forward, then apply.
    let forward_check = Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
        .args(["apply", "--check"])
        .arg(patch)
        .status()
        .context("failed to spawn git")?;
    if !forward_check.success() {
        anyhow::bail!(
            "{} doesn't apply cleanly to {}. Lynx source may have \
             moved; re-record the patch (git diff > {}) after \
             porting the changes.",
            patch.display(),
            repo.display(),
            patch.display()
        );
    }

    let apply = Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
        .arg("apply")
        .arg(patch)
        .status()
        .context("failed to spawn git apply")?;
    if !apply.success() {
        anyhow::bail!("git apply failed for {}", patch.display());
    }
    println!("  applied: {name}");
    Ok(())
}
