//! Single source of truth for "where Lynx / Whisker artifacts live".
//!
//! Both `xtask` and `whisker-driver-sys/build.rs` read from these
//! conventions, but they're duplicated rather than shared via a crate
//! because the build.rs case can resolve them from its own
//! `CARGO_MANIFEST_DIR` without an extra dep.

use std::path::PathBuf;

/// Absolute path to the Whisker workspace root.
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

// --- Lynx (Android) --------------------------------------------------

pub fn lynx_android_aars() -> PathBuf {
    target_dir().join("lynx-android")
}

pub fn lynx_android_unpacked() -> PathBuf {
    target_dir().join("lynx-android-unpacked")
}

// --- Lynx (iOS) ------------------------------------------------------

pub fn lynx_ios_root() -> PathBuf {
    target_dir().join("lynx-ios")
}

/// OS-neutral C++ header staging tree. Both `whisker-driver-sys/build.rs`
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
