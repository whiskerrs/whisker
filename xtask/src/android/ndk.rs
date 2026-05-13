//! NDK / Android-host toolchain discovery.
//!
//! Centralised so every subcommand resolves the toolchain the same
//! way and can be overridden via the same env vars.

use anyhow::Result;
use std::path::PathBuf;

/// NDK versions we know work with Tuft's link setup, in preference
/// order. NDK 23 was the cargo-ndk minimum and is the most-tested,
/// but anything ≥ 23 should work now that we provide
/// `__aarch64_have_lse_atomics` ourselves and don't link
/// `clang_rt.builtins` (no more outline-atomics init crash on
/// API 30+).
const PREFERRED_NDKS: &[&str] = &[
    "23.1.7779620",
    "25.1.8937393",
    "26.1.10909125",
    "26.3.11579264",
    "27.0.12077973",
    "27.1.12297006",
];

pub fn android_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ANDROID_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    // macOS default install location.
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let cand = home.join("Library/Android/sdk");
        if cand.is_dir() {
            return Ok(cand);
        }
    }
    anyhow::bail!(
        "ANDROID_HOME not set and no SDK found at the default macOS \
         location ($HOME/Library/Android/sdk)."
    )
}

/// Resolve the NDK root. Respects `ANDROID_NDK_HOME` first; otherwise
/// picks the first installed entry from `PREFERRED_NDKS` under
/// `$ANDROID_HOME/ndk/`.
pub fn ndk_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ANDROID_NDK_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }

    let ndk_dir = android_home()?.join("ndk");
    for version in PREFERRED_NDKS {
        let cand = ndk_dir.join(version);
        if cand.is_dir() {
            return Ok(cand);
        }
    }
    anyhow::bail!(
        "no supported NDK found in {} (need one of: {}). \
         Install via `sdkmanager 'ndk;23.1.7779620'` or set ANDROID_NDK_HOME.",
        ndk_dir.display(),
        PREFERRED_NDKS.join(", ")
    )
}

/// NDK toolchain host tag (`darwin-x86_64` / `linux-x86_64` / …).
pub fn host_tag() -> Result<&'static str> {
    if cfg!(target_os = "macos") {
        // NDK toolchains ship as `darwin-x86_64` even on Apple Silicon
        // (the binaries are universal / run under Rosetta).
        Ok("darwin-x86_64")
    } else if cfg!(target_os = "linux") {
        Ok("linux-x86_64")
    } else if cfg!(target_os = "windows") {
        Ok("windows-x86_64")
    } else {
        anyhow::bail!("unsupported host OS for Android cross-compilation")
    }
}

/// NDK ABI → Rust target triple.
pub fn abi_to_triple(abi: &str) -> Result<&'static str> {
    match abi {
        "arm64-v8a" => Ok("aarch64-linux-android"),
        "armeabi-v7a" => Ok("armv7-linux-androideabi"),
        "x86_64" => Ok("x86_64-linux-android"),
        "x86" => Ok("i686-linux-android"),
        other => anyhow::bail!("unknown Android ABI: {other}"),
    }
}

/// clang's `--target=<prefix><api>` prefix per ABI. Differs from the
/// Rust triple for `armeabi-v7a` only (clang wants `armv7a-linux-androideabi`).
pub fn clang_target_prefix(abi: &str) -> Result<&'static str> {
    match abi {
        "arm64-v8a" => Ok("aarch64-linux-android"),
        "armeabi-v7a" => Ok("armv7a-linux-androideabi"),
        "x86_64" => Ok("x86_64-linux-android"),
        "x86" => Ok("i686-linux-android"),
        other => anyhow::bail!("unknown Android ABI: {other}"),
    }
}

pub struct Toolchain {
    pub ndk: PathBuf,
    pub clang: PathBuf,
    pub clang_cpp: PathBuf,
    pub ar: PathBuf,
}

/// Resolve all toolchain paths for the given ABI + API level.
pub fn toolchain(abi: &str, api: u32) -> Result<Toolchain> {
    let ndk = ndk_home()?;
    let host = host_tag()?;
    let bin = ndk.join("toolchains/llvm/prebuilt").join(host).join("bin");
    let prefix = clang_target_prefix(abi)?;
    let clang = bin.join(format!("{prefix}{api}-clang"));
    let clang_cpp = bin.join(format!("{prefix}{api}-clang++"));
    let ar = bin.join("llvm-ar");
    for p in [&clang, &clang_cpp, &ar] {
        if !p.exists() {
            anyhow::bail!(
                "expected toolchain binary not found: {} (check NDK install)",
                p.display()
            );
        }
    }
    Ok(Toolchain {
        ndk,
        clang,
        clang_cpp,
        ar,
    })
}

