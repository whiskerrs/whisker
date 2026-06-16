//! Runtime support for Whisker's asset system.
//!
//! App authors place assets under their crate's `assets/` directory and
//! reference them with the [`asset!`], [`asset_str!`] and [`asset_bytes!`]
//! macros (re-exported here from `whisker-asset-macros`):
//!
//! ```ignore
//! use whisker_asset::asset;
//! let logo_url = asset!("images/logo.png"); // -> resolve("images/logo.png")
//! ```
//!
//! [`asset!`] lowers to a runtime call to [`resolve`], which composes a
//! platform-absolute path/URL from a process-global [`AssetBase`] plus the
//! logical relative path. The base is installed once, early at startup, by
//! native code (a later phase) via [`set_base`] or one of the C-ABI entry
//! points ([`whisker_asset_set_ios_base`] / [`whisker_asset_set_android`]).
//!
//! # Resolution
//!
//! | Base                       | `resolve("images/logo.png")`                          |
//! |----------------------------|-------------------------------------------------------|
//! | `IosDir("/var/app.app/whisker_assets")` | `/var/app.app/whisker_assets/images/logo.png` |
//! | `AndroidAssets`            | `file:///android_asset/whisker/images/logo.png`       |
//! | *(unset)*                  | the relative path, unchanged (see "Fallback")         |
//!
//! # Fallback
//!
//! If no base has been installed yet (unit tests, or any render that runs
//! before native init), [`resolve`] returns the **logical relative path
//! unchanged**. This is intentional: it is harmless, deterministic, and
//! lets pure-Rust tests and tooling run without a platform. Callers that
//! need to distinguish "resolved" from "fallback" can check [`base_is_set`].
//!
//! # Path normalization
//!
//! [`resolve`] normalizes its input the same way the macros validate it: a
//! leading `/` is stripped and any `..` traversal component is dropped, so a
//! base can never be escaped. (The macros already reject these at compile
//! time; this is defense-in-depth for paths that reach `resolve` by other
//! means.)

use std::sync::RwLock;

pub use whisker_asset_macros::{asset, asset_bytes, asset_str};

/// Whisker build plugin — bundles the app's declared assets into the
/// generated native projects (`gen/ios` / `gen/android`) so the
/// runtime resolver above finds them. Wired up by the consuming app via
/// `app.plugin::<WhiskerAsset>(|c| c.dir("assets"))` in `whisker.rs`.
/// See [`plugin`] for the full surface.
mod plugin;
pub use plugin::{WhiskerAsset, WhiskerAssetConfig};

/// The Android `assets/` URL prefix. Lynx/WebView load `file://` URLs and
/// Android exposes packaged assets under `/android_asset`. Whisker bundles
/// under a `whisker/` subdir to avoid colliding with host-app assets.
const ANDROID_PREFIX: &str = "file:///android_asset/whisker/";

/// How [`resolve`] turns a logical relative path into a platform path/URL.
///
/// Installed once at startup via [`set_base`] (or the C-ABI setters) and
/// read by every [`resolve`] call thereafter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetBase {
    /// iOS: an absolute directory (the app bundle's `whisker_assets` dir).
    /// Resolves to `"{dir}/{rel}"`.
    IosDir(String),
    /// Android: packaged assets. Resolves to
    /// `"file:///android_asset/whisker/{rel}"`. Carries no data because the
    /// prefix is fixed by the platform + Whisker's bundling convention.
    AndroidAssets,
}

impl AssetBase {
    /// Compose the platform path/URL for an already-normalized `rel`.
    fn compose(&self, rel: &str) -> String {
        match self {
            AssetBase::IosDir(dir) => {
                let dir = dir.strip_suffix('/').unwrap_or(dir);
                format!("{dir}/{rel}")
            }
            AssetBase::AndroidAssets => format!("{ANDROID_PREFIX}{rel}"),
        }
    }
}

/// Process-global base. `None` until native init installs one; reads fall
/// back to the unchanged relative path (see module docs).
static BASE: RwLock<Option<AssetBase>> = RwLock::new(None);

/// Install the process-global asset resolution base.
///
/// Intended to be called once, early at startup, by native code. Safe to
/// call again (it overwrites). Thread-safe: renders may read concurrently.
pub fn set_base(base: AssetBase) {
    *BASE.write().expect("whisker-asset BASE lock poisoned") = Some(base);
}

/// Whether a base has been installed yet. When `false`, [`resolve`] returns
/// its input unchanged (the documented fallback).
pub fn base_is_set() -> bool {
    BASE.read()
        .expect("whisker-asset BASE lock poisoned")
        .is_some()
}

/// Strip a leading `/` and drop any `..`/`.` components so a normalized,
/// base-relative path can never escape the base. Mirrors the compile-time
/// validation in `whisker-asset-macros`.
fn normalize(rel: &str) -> String {
    let trimmed = rel.trim_start_matches('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in trimmed.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Resolve a logical asset path (relative to the app's `assets/` dir) to a
/// platform-absolute path or URL.
///
/// - With an [`AssetBase::IosDir`] base: `"{dir}/{rel}"`.
/// - With [`AssetBase::AndroidAssets`]: `"file:///android_asset/whisker/{rel}"`.
/// - With no base installed: the normalized relative path, unchanged.
///
/// The input is normalized first (leading `/` stripped, `..` traversal
/// removed), so the base can never be escaped.
pub fn resolve(rel: &str) -> String {
    let rel = normalize(rel);
    match BASE
        .read()
        .expect("whisker-asset BASE lock poisoned")
        .as_ref()
    {
        Some(base) => base.compose(&rel),
        None => rel,
    }
}

// ---------------------------------------------------------------------------
// C ABI — called by native (iOS/Android) at startup. Phase 1 only wires
// these to `set_base`; native registration lands in a later phase.
// ---------------------------------------------------------------------------

/// Install an iOS directory base from native code.
///
/// # C ABI
///
/// ```c
/// void whisker_asset_set_ios_base(const uint8_t *ptr, size_t len);
/// ```
///
/// `ptr`/`len` describe a UTF-8 byte buffer holding the absolute path of
/// the app bundle's `whisker_assets` directory (no NUL terminator needed).
/// The bytes are copied; the caller retains ownership of the buffer.
///
/// # Safety
///
/// `ptr` must point to at least `len` valid, initialized bytes for the
/// duration of the call (or `len` must be 0). The bytes must be valid
/// UTF-8; invalid input is ignored (the base is left unchanged).
#[no_mangle]
pub unsafe extern "C" fn whisker_asset_set_ios_base(ptr: *const u8, len: usize) {
    if ptr.is_null() && len != 0 {
        return;
    }
    let slice = if len == 0 {
        &[][..]
    } else {
        // SAFETY: caller guarantees `ptr` is valid for `len` bytes.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    };
    match std::str::from_utf8(slice) {
        Ok(dir) => set_base(AssetBase::IosDir(dir.to_owned())),
        Err(_) => { /* invalid UTF-8: ignore, leave base unchanged */ }
    }
}

/// Install the Android packaged-assets base from native code.
///
/// # C ABI
///
/// ```c
/// void whisker_asset_set_android(void);
/// ```
///
/// Takes no arguments — the Android resolution prefix is fixed
/// (`file:///android_asset/whisker/`).
#[no_mangle]
pub extern "C" fn whisker_asset_set_android() {
    set_base(AssetBase::AndroidAssets);
}

#[cfg(test)]
mod tests {
    use super::*;

    // The global BASE is shared process state. Tests that mutate it run
    // under this mutex to stay deterministic regardless of test threading,
    // and each restores BASE to `None` when done.
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset() {
        *BASE.write().unwrap() = None;
    }

    #[test]
    fn resolve_ios_base() {
        let _g = GUARD.lock().unwrap();
        reset();
        set_base(AssetBase::IosDir("/var/app.app/whisker_assets".into()));
        assert_eq!(
            resolve("images/logo.png"),
            "/var/app.app/whisker_assets/images/logo.png"
        );
        reset();
    }

    #[test]
    fn resolve_ios_base_trailing_slash_normalized() {
        let _g = GUARD.lock().unwrap();
        reset();
        set_base(AssetBase::IosDir("/var/app.app/whisker_assets/".into()));
        assert_eq!(
            resolve("images/logo.png"),
            "/var/app.app/whisker_assets/images/logo.png"
        );
        reset();
    }

    #[test]
    fn resolve_android_base() {
        let _g = GUARD.lock().unwrap();
        reset();
        set_base(AssetBase::AndroidAssets);
        assert_eq!(
            resolve("images/logo.png"),
            "file:///android_asset/whisker/images/logo.png"
        );
        reset();
    }

    #[test]
    fn resolve_fallback_when_unset() {
        let _g = GUARD.lock().unwrap();
        reset();
        assert!(!base_is_set());
        assert_eq!(resolve("images/logo.png"), "images/logo.png");
        reset();
    }

    #[test]
    fn ffi_ios_setter_updates_resolve() {
        let _g = GUARD.lock().unwrap();
        reset();
        let dir = b"/tmp/bundle/whisker_assets";
        // SAFETY: valid pointer/len into a live byte array.
        unsafe { whisker_asset_set_ios_base(dir.as_ptr(), dir.len()) };
        assert!(base_is_set());
        assert_eq!(resolve("a/b.png"), "/tmp/bundle/whisker_assets/a/b.png");
        reset();
    }

    #[test]
    fn ffi_android_setter_updates_resolve() {
        let _g = GUARD.lock().unwrap();
        reset();
        whisker_asset_set_android();
        assert_eq!(resolve("a/b.png"), "file:///android_asset/whisker/a/b.png");
        reset();
    }

    #[test]
    fn ffi_ios_setter_ignores_invalid_utf8() {
        let _g = GUARD.lock().unwrap();
        reset();
        let bad = [0xff, 0xfe, 0xfd];
        // SAFETY: valid pointer/len; bytes are intentionally invalid UTF-8.
        unsafe { whisker_asset_set_ios_base(bad.as_ptr(), bad.len()) };
        assert!(!base_is_set(), "invalid UTF-8 must leave base unset");
        reset();
    }

    #[test]
    fn normalize_strips_leading_slash() {
        let _g = GUARD.lock().unwrap();
        reset();
        set_base(AssetBase::IosDir("/base".into()));
        // A leading slash is stripped so the base is preserved.
        assert_eq!(resolve("/images/logo.png"), "/base/images/logo.png");
        reset();
    }

    #[test]
    fn normalize_drops_parent_traversal() {
        let _g = GUARD.lock().unwrap();
        reset();
        set_base(AssetBase::AndroidAssets);
        // `..` components are dropped; the base can't be escaped.
        assert_eq!(
            resolve("images/../../../etc/passwd"),
            "file:///android_asset/whisker/etc/passwd"
        );
        reset();
    }

    #[test]
    fn normalize_fallback_path() {
        let _g = GUARD.lock().unwrap();
        reset();
        assert_eq!(resolve("/a/./b/../c.png"), "a/c.png");
        reset();
    }
}
