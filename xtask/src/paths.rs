//! Single source of truth for "where Lynx / Tuft artifacts live".
//!
//! Both `xtask` and `tuft-driver-sys/build.rs` read from these
//! conventions, but they're duplicated rather than shared via a crate
//! because the build.rs case can resolve them from its own
//! `CARGO_MANIFEST_DIR` without an extra dep.

use std::path::PathBuf;

/// Absolute path to the Tuft workspace root.
pub fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(|p| p.to_path_buf())
        .expect("xtask manifest dir has a parent")
}

/// `<workspace>/target` (we don't honour `CARGO_TARGET_DIR` — Lynx
/// outputs are vendoring artifacts, not cargo outputs).
pub fn target_dir() -> PathBuf {
    workspace_root().join("target")
}

// --- Bridge headers (consumed by `cargo xtask ios build-xcframework`
//     when staging public headers into the xcframework). The C++
//     sources themselves live under the same tree but are compiled
//     by `tuft-driver-sys/build.rs` and never read from xtask. -----

pub fn bridge_include() -> PathBuf {
    workspace_root().join("crates/tuft-driver-sys/bridge/include")
}

// --- Lynx (Android) --------------------------------------------------

pub fn lynx_android_aars() -> PathBuf {
    target_dir().join("lynx-android")
}

pub fn lynx_android_unpacked() -> PathBuf {
    target_dir().join("lynx-android-unpacked")
}

pub fn lynx_android_jni(abi: &str) -> PathBuf {
    lynx_android_unpacked().join("jni").join(abi)
}

// --- Lynx (iOS) ------------------------------------------------------

pub fn lynx_ios_root() -> PathBuf {
    target_dir().join("lynx-ios")
}

/// OS-neutral C++ header staging tree. Both `tuft-driver-sys/build.rs`
/// (Android + iOS cc::Build) and the iOS Lynx framework build read
/// from this directory. The C++ headers are platform-agnostic so we
/// don't duplicate per-OS.
pub fn lynx_staged_headers() -> PathBuf {
    target_dir().join("lynx-headers")
}

// --- Lynx source -----------------------------------------------------

pub fn lynx_src_default() -> PathBuf {
    target_dir().join("lynx-src")
}

// --- Tuft driver xcframework ----------------------------------------

pub fn tuft_driver_out() -> PathBuf {
    target_dir().join("tuft-driver")
}
