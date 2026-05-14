//! Android NDK toolchain resolution for the hot-patch link step.
//!
//! Mirrors `xtask/src/android/ndk.rs` (deliberately, not a shared
//! crate) so the dev-server doesn't pull xtask into its dep tree.
//! The two implementations need to stay in sync — if NDK layouts
//! shift, both have to learn about the new shape.
//!
//! What this module gives us:
//!
//!   - resolve the NDK root (env first, then `$ANDROID_HOME/ndk/<v>`)
//!   - pick a host-tag for the toolchain bin dir
//!   - locate `<prefix><api>-clang` for an ABI
//!
//! Tier 1's link step (build_link_plan + run_link_plan) uses the
//! returned clang as both `linker_path` (what we spawn) and
//! `WHISKER_REAL_LINKER` (what the linker shim forwards to during the
//! fat build). Same binary on both sides keeps SDK / sysroot
//! resolution consistent.

use anyhow::Result;
use std::path::{Path, PathBuf};

/// NDK versions we know work with Whisker, in preference order.
/// Same list as xtask — keep in sync.
const PREFERRED_NDKS: &[&str] = &[
    "23.1.7779620",
    "25.1.8937393",
    "26.1.10909125",
    "26.3.11579264",
    "27.0.12077973",
    "27.1.12297006",
];

/// Find the Android SDK root. Honours `ANDROID_HOME`; otherwise
/// falls back to the macOS default install location.
pub fn android_home() -> Result<PathBuf> {
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
    anyhow::bail!(
        "ANDROID_HOME not set and no SDK found at the default macOS \
         location ($HOME/Library/Android/sdk)."
    )
}

/// Find the NDK root. `ANDROID_NDK_HOME` wins; otherwise picks the
/// first installed entry from [`PREFERRED_NDKS`] under
/// `<ANDROID_HOME>/ndk/`.
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
        "no supported NDK found in {} (need one of: {})",
        ndk_dir.display(),
        PREFERRED_NDKS.join(", "),
    )
}

/// NDK toolchain host tag (`darwin-x86_64` / `linux-x86_64` / …).
/// NDK ships `darwin-x86_64` even on Apple Silicon.
pub fn host_tag() -> Result<&'static str> {
    if cfg!(target_os = "macos") {
        Ok("darwin-x86_64")
    } else if cfg!(target_os = "linux") {
        Ok("linux-x86_64")
    } else if cfg!(target_os = "windows") {
        Ok("windows-x86_64")
    } else {
        anyhow::bail!("unsupported host OS for Android cross-compilation")
    }
}

/// clang's `--target=<prefix><api>` prefix per ABI. Differs from
/// the Rust triple for `armeabi-v7a` (clang wants
/// `armv7a-linux-androideabi`).
pub fn clang_target_prefix(abi: &str) -> Result<&'static str> {
    match abi {
        "arm64-v8a" => Ok("aarch64-linux-android"),
        "armeabi-v7a" => Ok("armv7a-linux-androideabi"),
        "x86_64" => Ok("x86_64-linux-android"),
        "x86" => Ok("i686-linux-android"),
        other => anyhow::bail!("unknown Android ABI: {other}"),
    }
}

/// Locate the NDK clang for `(abi, api)`. The returned path is the
/// API-pinned wrapper (e.g. `aarch64-linux-android21-clang`) — both
/// `Builder::with_capture` (as `WHISKER_REAL_LINKER`) and the
/// thin-rebuild link step (as `linker_path`) need this same
/// binary.
pub fn android_clang_for(abi: &str, api: u32) -> Result<PathBuf> {
    let bin = ndk_bin_dir()?;
    let prefix = clang_target_prefix(abi)?;
    let clang = bin.join(format!("{prefix}{api}-clang"));
    if !clang.exists() {
        anyhow::bail!(
            "NDK clang not found: {} — check that the NDK is installed and \
             API level {api} is supported",
            clang.display(),
        );
    }
    Ok(clang)
}

/// `<NDK>/toolchains/llvm/prebuilt/<host>/bin`. Pulled out so
/// future helpers (llvm-ar, ranlib, lld-link) don't have to recompose
/// it.
pub fn ndk_bin_dir() -> Result<PathBuf> {
    let ndk = ndk_home()?;
    let host = host_tag()?;
    Ok(ndk.join("toolchains/llvm/prebuilt").join(host).join("bin"))
}

/// Pure helper: convert an existing-on-disk dylib path under a
/// jniLibs tree to its ABI. Used by the patcher to figure out which
/// NDK clang to use without the caller having to plumb the ABI
/// separately. Returns `None` if the path doesn't fit the
/// `…/jniLibs/<abi>/lib<crate>.so` shape.
pub fn abi_from_jni_libs_path(p: &Path) -> Option<&'static str> {
    let parent = p.parent()?.file_name()?.to_str()?;
    match parent {
        "arm64-v8a" => Some("arm64-v8a"),
        "armeabi-v7a" => Some("armeabi-v7a"),
        "x86_64" => Some("x86_64"),
        "x86" => Some("x86"),
        _ => None,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ----- pure helpers ------------------------------------------------

    #[test]
    fn host_tag_returns_a_known_string_for_this_host() {
        let t = host_tag().expect("host tag");
        assert!(matches!(
            t,
            "darwin-x86_64" | "linux-x86_64" | "windows-x86_64",
        ));
    }

    #[test]
    fn clang_target_prefix_maps_known_abis() {
        assert_eq!(clang_target_prefix("arm64-v8a").unwrap(), "aarch64-linux-android");
        assert_eq!(clang_target_prefix("armeabi-v7a").unwrap(), "armv7a-linux-androideabi");
        assert_eq!(clang_target_prefix("x86_64").unwrap(), "x86_64-linux-android");
        assert_eq!(clang_target_prefix("x86").unwrap(), "i686-linux-android");
    }

    #[test]
    fn clang_target_prefix_rejects_unknown_abi() {
        let err = clang_target_prefix("riscv64").unwrap_err();
        assert!(format!("{err:#}").contains("unknown Android ABI"));
    }

    #[test]
    fn abi_from_jni_libs_path_maps_known_layouts() {
        assert_eq!(
            abi_from_jni_libs_path(Path::new(
                "/ws/examples/foo/android/app/src/main/jniLibs/arm64-v8a/libfoo.so",
            )),
            Some("arm64-v8a"),
        );
        assert_eq!(
            abi_from_jni_libs_path(Path::new("/ws/jniLibs/x86_64/libfoo.so")),
            Some("x86_64"),
        );
    }

    #[test]
    fn abi_from_jni_libs_path_returns_none_for_non_abi_layout() {
        assert_eq!(
            abi_from_jni_libs_path(Path::new("/random/path/libfoo.so")),
            None,
        );
        assert_eq!(
            abi_from_jni_libs_path(Path::new("/ws/jniLibs/unknown-abi/libfoo.so")),
            None,
        );
    }

    // ----- environment-dependent (skipped if NDK absent) --------------

    #[test]
    fn android_clang_returns_a_path_when_ndk_is_installed() {
        // Skip the whole test if the developer doesn't have an NDK
        // — it's unreasonable to require one for `cargo test`.
        let Ok(ndk) = ndk_home() else { return };
        let clang = android_clang_for("arm64-v8a", 21).expect("ndk clang");
        assert!(clang.starts_with(&ndk));
        assert!(clang.is_file(), "{clang:?} should exist");
    }
}
