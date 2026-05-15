//! Android cargo + gradle orchestration. Shared by `whisker-cli`'s
//! `whisker build` and `whisker-dev-server`'s Tier 2 cold rebuild
//! path.
//!
//! Three phases:
//!
//! 1. [`cargo_build_dylib`] — cross-compile the user crate as a Mach-O
//!    `.so` via `cargo rustc --crate-type dylib --target <triple>`.
//!    Why `dylib` (and not `cdylib`)? rustc unconditionally injects
//!    `-Wl,--exclude-libs,ALL` for `cdylib`, which strips every
//!    mangled Rust symbol from `.dynsym`. The `dylib` flavour keeps
//!    them — `System.loadLibrary` doesn't care which flavour, but
//!    the symmetric Tier 1 hot-patch path (dev mode) does. Production
//!    builds use the same shape for consistency.
//!
//! 2. [`stage_jni_libs`] — drop the `.so` plus the matching
//!    `libc++_shared.so` from the NDK sysroot into the gen tree's
//!    `app/src/main/jniLibs/<abi>/`. The bridge is dynamically linked
//!    against `libc++_shared`; without it `System.loadLibrary` fails
//!    with `dlopen failed: cannot locate symbol _ZNSt6__ndk1…`.
//!
//! 3. [`run_gradle_assemble`] — invoke `gradle :app:assemble{Release,Debug}`
//!    against the generated project. Output is `app-{release,debug}.apk`
//!    under `app/build/outputs/apk/<profile>/`.
//!
//! Tier 1 fat-build capture (see [`crate::capture`]) is opt-in via
//! the `capture` field on [`CargoBuild`] — dev-server's Tier 2
//! cold rebuild passes `Some(&shims)`, `whisker build` passes `None`.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::capture::{capture_env_vars, CaptureShims};
use crate::Profile;

// ----- NDK toolchain resolution --------------------------------------------

/// NDK versions Whisker is known to link against. Preferred first.
const PREFERRED_NDKS: &[&str] = &[
    "23.1.7779620",
    "25.1.8937393",
    "26.1.10909125",
    "26.3.11579264",
    "27.0.12077973",
    "27.1.12297006",
];

/// Toolchain paths for a given (ABI, API level) pair.
pub struct AndroidToolchain {
    pub ndk: PathBuf,
    pub clang: PathBuf,
    pub clang_cpp: PathBuf,
    pub ar: PathBuf,
    pub triple: &'static str,
}

pub fn resolve_toolchain(abi: &str, api: u32) -> Result<AndroidToolchain> {
    let ndk = ndk_home()?;
    let host = host_tag()?;
    let bin = ndk.join("toolchains/llvm/prebuilt").join(host).join("bin");
    let clang_prefix = clang_target_prefix(abi)?;
    let clang = bin.join(format!("{clang_prefix}{api}-clang"));
    let clang_cpp = bin.join(format!("{clang_prefix}{api}-clang++"));
    let ar = bin.join("llvm-ar");
    for p in [&clang, &clang_cpp, &ar] {
        if !p.exists() {
            return Err(anyhow!(
                "expected NDK toolchain binary not found: {} \
                 (check `sdkmanager --install \"ndk;{}\"`)",
                p.display(),
                PREFERRED_NDKS[0],
            ));
        }
    }
    Ok(AndroidToolchain {
        ndk,
        clang,
        clang_cpp,
        ar,
        triple: abi_to_triple(abi)?,
    })
}

fn android_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ANDROID_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let cand = home.join("Library/Android/sdk");
        if cand.is_dir() {
            return Ok(cand);
        }
    }
    Err(anyhow!(
        "ANDROID_HOME not set and no SDK at $HOME/Library/Android/sdk",
    ))
}

fn ndk_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ANDROID_NDK_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    let ndk_dir = android_home()?.join("ndk");
    for v in PREFERRED_NDKS {
        let cand = ndk_dir.join(v);
        if cand.is_dir() {
            return Ok(cand);
        }
    }
    Err(anyhow!(
        "no supported NDK at {} (need one of: {})",
        ndk_dir.display(),
        PREFERRED_NDKS.join(", "),
    ))
}

fn host_tag() -> Result<&'static str> {
    if cfg!(target_os = "macos") {
        Ok("darwin-x86_64") // universal, runs on Apple Silicon too
    } else if cfg!(target_os = "linux") {
        Ok("linux-x86_64")
    } else if cfg!(target_os = "windows") {
        Ok("windows-x86_64")
    } else {
        Err(anyhow!("unsupported host OS for Android cross-compile"))
    }
}

pub fn abi_to_triple(abi: &str) -> Result<&'static str> {
    match abi {
        "arm64-v8a" => Ok("aarch64-linux-android"),
        "armeabi-v7a" => Ok("armv7-linux-androideabi"),
        "x86_64" => Ok("x86_64-linux-android"),
        "x86" => Ok("i686-linux-android"),
        other => Err(anyhow!("unknown Android ABI: {other}")),
    }
}

fn clang_target_prefix(abi: &str) -> Result<&'static str> {
    match abi {
        "arm64-v8a" => Ok("aarch64-linux-android"),
        "armeabi-v7a" => Ok("armv7a-linux-androideabi"),
        "x86_64" => Ok("x86_64-linux-android"),
        "x86" => Ok("i686-linux-android"),
        other => Err(anyhow!("unknown Android ABI: {other}")),
    }
}

// ----- cargo build ----------------------------------------------------------

pub struct CargoBuild<'a> {
    pub workspace_root: &'a Path,
    pub package: &'a str,
    pub toolchain: &'a AndroidToolchain,
    pub profile: Profile,
    /// Cargo features to forward (`--features <each>`). Empty for prod.
    pub features: &'a [String],
    /// `Some` → fold rustc/linker shim env vars into the cargo
    /// invocation, populating the Tier 1 capture caches. `None` →
    /// plain build. Dev-server passes `Some(&shims)` for its initial
    /// fat build and Tier 2 cold rebuilds; `whisker build` passes
    /// `None` (no Tier 1 in prod).
    pub capture: Option<&'a CaptureShims>,
}

/// Run `cargo rustc --crate-type dylib --target <triple>` against the
/// user crate. Returns the absolute path to the produced `.so`.
pub fn cargo_build_dylib(b: &CargoBuild<'_>) -> Result<PathBuf> {
    // Version-script: rustc auto-generates one that lists Rust-mangled
    // symbols in `global:` and ends with `local: *;`, which would
    // demote `Java_*` and `JNI_OnLoad` to LOCAL — `System.loadLibrary`
    // would then fail to find them. We pass a second, additive
    // version-script listing the JNI symbols; lld unions multiple
    // anonymous scripts, so JNI exports survive without touching
    // rustc's Rust-symbol list.
    let vs_dir = b.workspace_root.join("target/.whisker");
    std::fs::create_dir_all(&vs_dir)
        .with_context(|| format!("create {}", vs_dir.display()))?;
    let vs_path = vs_dir.join("android-jni-exports.ver");
    std::fs::write(
        &vs_path,
        b"{\n  global:\n    Java_*;\n    JNI_OnLoad;\n};\n",
    )
    .with_context(|| format!("write {}", vs_path.display()))?;

    let triple = b.toolchain.triple;
    let triple_env = triple.replace('-', "_");
    let triple_upper = triple_env.to_uppercase();

    let mut cmd = Command::new("cargo");
    cmd.arg("rustc")
        .args(["--target", triple])
        .args(["-p", b.package])
        .args(["--crate-type", "dylib"]);
    if let Some(flag) = b.profile.cargo_flag() {
        cmd.arg(flag);
    }
    for feat in b.features {
        cmd.args(["--features", feat]);
    }
    cmd.arg("--").args([
        "-C".to_string(),
        format!("link-arg=-Wl,--version-script={}", vs_path.display()),
    ]);

    cmd.env(format!("CC_{triple_env}"), &b.toolchain.clang);
    cmd.env(format!("CXX_{triple_env}"), &b.toolchain.clang_cpp);
    cmd.env(format!("AR_{triple_env}"), &b.toolchain.ar);
    let linker_env = format!("CARGO_TARGET_{triple_upper}_LINKER");
    if std::env::var_os(&linker_env).is_none() {
        cmd.env(&linker_env, &b.toolchain.clang);
    }
    cmd.env("ANDROID_NDK_HOME", &b.toolchain.ndk);
    cmd.current_dir(b.workspace_root);

    // Tier 1 capture shims (rustc-shim + linker-shim + cache dirs).
    // `CARGO_TARGET_<triple>_LINKER` set above is overridden here so
    // the linker shim wins for this triple — the shim forwards to
    // `WHISKER_REAL_LINKER` after writing its capture JSON. Host-only
    // artifacts (build scripts, proc-macros) keep their default
    // linker since the env is keyed by target triple.
    if let Some(c) = b.capture {
        std::fs::create_dir_all(&c.rustc_cache_dir).with_context(|| {
            format!("create rustc cache dir {}", c.rustc_cache_dir.display())
        })?;
        std::fs::create_dir_all(&c.linker_cache_dir).with_context(|| {
            format!("create linker cache dir {}", c.linker_cache_dir.display())
        })?;
        for (k, v) in capture_env_vars(c) {
            cmd.env(k, v);
        }
    }

    eprintln!(
        "[whisker-build] cargo rustc --crate-type dylib --target {triple} -p {pkg} ({profile:?})",
        pkg = b.package,
        profile = b.profile,
    );
    let status = cmd
        .status()
        .with_context(|| format!("spawn cargo for {triple}"))?;
    if !status.success() {
        return Err(anyhow!("cargo build failed ({status}) for {triple}"));
    }

    let lib_name = format!("lib{}.so", b.package.replace('-', "_"));
    let so_path = b
        .workspace_root
        .join("target")
        .join(triple)
        .join(b.profile.dir_name())
        .join(&lib_name);
    if !so_path.is_file() {
        return Err(anyhow!(
            "cargo finished but {} is missing",
            so_path.display(),
        ));
    }
    Ok(so_path)
}

// ----- jniLibs staging ------------------------------------------------------

/// Copy `so` plus the NDK-shipped `libc++_shared.so` into
/// `gen/android/app/src/main/jniLibs/<abi>/`.
pub fn stage_jni_libs(gen_android: &Path, abi: &str, so: &Path, tc: &AndroidToolchain) -> Result<()> {
    let dst_dir = gen_android.join("app/src/main/jniLibs").join(abi);
    std::fs::create_dir_all(&dst_dir)
        .with_context(|| format!("mkdir -p {}", dst_dir.display()))?;

    let so_name = so
        .file_name()
        .ok_or_else(|| anyhow!("so path has no filename: {}", so.display()))?;
    let dst_so = dst_dir.join(so_name);
    std::fs::copy(so, &dst_so)
        .with_context(|| format!("copy {} → {}", so.display(), dst_so.display()))?;

    let libcxx = find_libcxx_shared(&tc.ndk, abi)?;
    let dst_libcxx = dst_dir.join("libc++_shared.so");
    std::fs::copy(&libcxx, &dst_libcxx)
        .with_context(|| format!("copy {} → {}", libcxx.display(), dst_libcxx.display()))?;

    eprintln!(
        "[whisker-build] staged jniLibs: {} + libc++_shared.so",
        so_name.to_string_lossy(),
    );
    Ok(())
}

/// Locate `libc++_shared.so` inside the NDK sysroot for `abi`. NDKs
/// place it under the host-prebuilt sysroot's lib/<triple>/ dir.
fn find_libcxx_shared(ndk: &Path, abi: &str) -> Result<PathBuf> {
    let host = host_tag()?;
    let triple = match abi {
        "arm64-v8a" => "aarch64-linux-android",
        "armeabi-v7a" => "arm-linux-androideabi",
        "x86_64" => "x86_64-linux-android",
        "x86" => "i686-linux-android",
        other => return Err(anyhow!("unknown ABI for libc++_shared lookup: {other}")),
    };
    let cand = ndk
        .join("toolchains/llvm/prebuilt")
        .join(host)
        .join("sysroot/usr/lib")
        .join(triple)
        .join("libc++_shared.so");
    if !cand.is_file() {
        return Err(anyhow!(
            "libc++_shared.so missing at {} (check NDK install)",
            cand.display(),
        ));
    }
    Ok(cand)
}

// ----- gradle ---------------------------------------------------------------

/// Invoke `./gradlew :app:assemble{Release,Debug}` on the gen tree.
/// Returns the path to the produced APK.
pub fn run_gradle_assemble(gen_android: &Path, profile: Profile) -> Result<PathBuf> {
    let task = match profile {
        Profile::Release => ":app:assembleRelease",
        Profile::Debug => ":app:assembleDebug",
    };
    eprintln!("[whisker-build] gradle {task}");
    let java_home = resolve_java_home()?;
    let gradlew = gen_android.join("gradlew");
    if !gradlew.is_file() {
        return Err(anyhow!(
            "gradlew missing at {} — has the gen tree been synced?",
            gradlew.display(),
        ));
    }
    let status = Command::new(&gradlew)
        .arg(task)
        .arg("--no-daemon")
        .current_dir(gen_android)
        .env("JAVA_HOME", &java_home)
        .status()
        .with_context(|| format!("spawn {}", gradlew.display()))?;
    if !status.success() {
        return Err(anyhow!("gradle {task} failed ({status})"));
    }
    let kind = profile.dir_name();
    // Release APKs are unsigned by default; sniff both filenames so the
    // function works whether the user has wired up a signingConfig.
    let outputs = gen_android.join(format!("app/build/outputs/apk/{kind}"));
    for name in [
        format!("app-{kind}.apk"),
        format!("app-{kind}-unsigned.apk"),
    ] {
        let cand = outputs.join(&name);
        if cand.is_file() {
            return Ok(cand);
        }
    }
    Err(anyhow!(
        "gradle succeeded but no APK found under {}",
        outputs.display(),
    ))
}

/// Java 17 home for AGP 8.x. Looks at JAVA_HOME first; otherwise tries
/// `/usr/libexec/java_home -v 17` on macOS.
fn resolve_java_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("JAVA_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("/usr/libexec/java_home")
            .args(["-v", "17"])
            .output()
            .context("spawn /usr/libexec/java_home -v 17")?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let p = PathBuf::from(&path);
            if p.is_dir() {
                return Ok(p);
            }
        }
    }
    Err(anyhow!(
        "JAVA_HOME unset and could not auto-detect a Java 17 JDK",
    ))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_to_triple_maps_known_abis() {
        assert_eq!(abi_to_triple("arm64-v8a").unwrap(), "aarch64-linux-android");
        assert_eq!(abi_to_triple("x86_64").unwrap(), "x86_64-linux-android");
        assert!(abi_to_triple("bogus").is_err());
    }

    #[test]
    fn clang_target_prefix_uses_armv7a_for_armeabi() {
        assert_eq!(
            clang_target_prefix("armeabi-v7a").unwrap(),
            "armv7a-linux-androideabi",
        );
        // arm64 prefix matches the rust triple.
        assert_eq!(
            clang_target_prefix("arm64-v8a").unwrap(),
            "aarch64-linux-android",
        );
    }
}
