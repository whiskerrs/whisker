//! Single source of truth for "where Lynx / bridge artifacts live".
//!
//! Both `tuft_build::compile()` (per-crate build.rs) and `xtask`
//! (orchestration) call into these. Add a new artifact location? Add
//! a function here. Move one? Change it once. The compile / pack
//! sides can't drift because they're reading from the same constants.
//!
//! All paths are resolved against [`workspace_root`], which is
//! determined at *compile time of this crate* via
//! `env!("CARGO_MANIFEST_DIR")`. So no matter where in the workspace
//! cargo / xtask was invoked from, the same paths come back.

use std::path::PathBuf;

/// Absolute path to the Tuft workspace root.
///
/// `CARGO_MANIFEST_DIR` resolves to `<workspace>/crates/tuft-build`
/// at compile time, so its grandparent is the workspace root.
pub fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .expect("tuft-build manifest dir has no grandparent")
        .to_path_buf()
}

/// `<workspace>/target` (or `$CARGO_TARGET_DIR` if set).
///
/// We don't honour `CARGO_TARGET_DIR` automatically because Lynx
/// outputs are conceptually source-vendoring, not Cargo artifacts.
/// Keeping them under `target/` is a convenience (it's gitignored)
/// but they're not produced by `cargo`.
pub fn target_dir() -> PathBuf {
    workspace_root().join("target")
}

// --- Bridge sources --------------------------------------------------

pub fn bridge_root() -> PathBuf {
    workspace_root().join("crates/tuft-driver-sys/bridge")
}

pub fn bridge_include() -> PathBuf {
    bridge_root().join("include")
}

pub fn bridge_src() -> PathBuf {
    bridge_root().join("src")
}

// --- Lynx (Android) --------------------------------------------------

/// Where `cargo xtask android build-lynx-aar` drops AARs and where
/// `unpack-lynx` reads them from. Also where `build-android-example`
/// expects them.
pub fn lynx_android_aars() -> PathBuf {
    target_dir().join("lynx-android")
}

/// Where `unpack-lynx` extracts `jni/<abi>/` from each AAR. The
/// platform `build.rs` adds this as `-L` so `-llynx` etc. resolve at
/// link time.
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

pub fn lynx_ios_xcframework(name: &str) -> PathBuf {
    lynx_ios_root().join(format!("{name}.xcframework"))
}

/// Lynx public C++ headers staged by
/// `cargo xtask ios build-lynx-frameworks`. **Both** iOS and Android
/// reads from this directory — the C++ headers are platform-
/// agnostic and we don't want to duplicate them in two staging
/// trees.
pub fn lynx_staged_headers() -> PathBuf {
    lynx_ios_root().join("sources")
}

// --- Lynx source (for build-lynx-aar / build-lynx-frameworks) -------

/// Default location for a Lynx source checkout. The user can override
/// via `LYNX_SRC` env var or `--lynx-src`. Keeping it under `target/`
/// (5 GB or so once `tools/hab sync`'d) means it's gitignored and
/// `cargo clean` doesn't wipe it.
pub fn lynx_src_default() -> PathBuf {
    target_dir().join("lynx-src")
}

// --- Tuft driver xcframework ----------------------------------------

/// Output dir for the user-crate xcframework produced by
/// `cargo xtask ios build-xcframework`.
pub fn tuft_driver_out() -> PathBuf {
    target_dir().join("tuft-driver")
}
