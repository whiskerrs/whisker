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
//! native code via [`set_base`] or one of the C-ABI entry
//! points ([`whisker_asset_set_ios_base`] / [`whisker_asset_set_android`]).
//!
//! # Resolution
//!
//! | Base                       | `resolve("images/logo.png")`                          |
//! |----------------------------|-------------------------------------------------------|
//! | `IosDir("/var/app.app/whisker_assets")` | `/var/app.app/whisker_assets/images/logo.png` |
//! | `AndroidAssets`            | `file:///android_asset/whisker/images/logo.png`       |
//! | macOS bundle             | `<app>/Contents/Resources/whisker_assets/images/logo.png` |
//! | Web metadata (`/app/`)    | `/app/images/logo.png`                                 |
//! | *(unset)*                  | the relative path, unchanged (see "Fallback")         |
//!
//! On Web, the project plugin writes a `whisker-asset-base` metadata element.
//! The resolver reads its deployment prefix and returns an origin-relative,
//! percent-encoded URL, independent of the current route or HTML base element.
//! macOS finds `Contents/Resources/whisker_assets` beside the bundled executable.
//! Windows uses a sibling `whisker_assets` directory; Linux uses `../share/<executable>/whisker_assets` from `bin/`.
//!
//! # Fallback
//!
//! If neither a base nor generated Web metadata is available (unit tests, or a
//! render before native init), [`resolve`] returns the **logical relative path
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
/// generated projects (Android, iOS, macOS, Windows, Linux, and Web) so the
/// runtime resolver above finds them. Wired up by the consuming app via
/// `app.project_plugin::<WhiskerAsset>(|c| c.dir("assets"))` in `whisker.rs`.
/// See [`plugin`] for the full surface.
mod plugin;
pub use plugin::{WhiskerAsset, WhiskerAssetConfig};

/// The Android `assets/` URL prefix. Native WebViews load `file://` URLs and
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
    /// macOS: the app bundle's Contents/Resources/whisker_assets directory.
    MacosDir(String),
    /// Windows/Linux: a directory within the generated distribution tree.
    DesktopDir(String),
    /// Android: packaged assets. Resolves to
    /// `"file:///android_asset/whisker/{rel}"`. Carries no data because the
    /// prefix is fixed by the platform + Whisker's bundling convention.
    AndroidAssets,
    /// Web deployment URL prefix, such as `/` or `/app/`. Asset path segments
    /// are percent-encoded. The generated metadata normally supplies this base.
    WebUrl(String),
}

impl AssetBase {
    /// Compose the platform path/URL for an already-normalized `rel`.
    fn compose(&self, rel: &str) -> String {
        match self {
            AssetBase::IosDir(dir) | AssetBase::MacosDir(dir) | AssetBase::DesktopDir(dir) => {
                let dir = dir.strip_suffix('/').unwrap_or(dir);
                format!("{dir}/{rel}")
            }
            AssetBase::AndroidAssets => format!("{ANDROID_PREFIX}{rel}"),
            AssetBase::WebUrl(prefix) => {
                use std::fmt::Write;
                let mut url = format!("{}/", prefix.trim_end_matches('/'));
                for byte in rel.bytes() {
                    if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                        url.push(byte as char);
                    } else {
                        write!(url, "%{byte:02X}").unwrap();
                    }
                }
                url
            }
        }
    }
}

/// Explicit process-global base. Web can also read its generated metadata;
/// otherwise an unset base uses the relative fallback.
static BASE: RwLock<Option<AssetBase>> = RwLock::new(None);

/// Install the process-global asset resolution base.
///
/// Intended to be called once, early at startup, by native code. Safe to
/// call again (it overwrites). Thread-safe: renders may read concurrently.
pub fn set_base(base: AssetBase) {
    *BASE.write().expect("whisker-asset BASE lock poisoned") = Some(base);
}

/// Whether an explicit base, generated Web metadata, or macOS bundle base is available.
/// When `false`, [`resolve`] uses the normalized relative fallback.
pub fn base_is_set() -> bool {
    BASE.read()
        .expect("whisker-asset BASE lock poisoned")
        .is_some()
        || platform_base().is_some()
}

#[cfg(target_arch = "wasm32")]
fn platform_base() -> Option<AssetBase> {
    let document = web_sys::window()?.document()?;
    let element = document
        .query_selector(&format!("meta[name='{}']", plugin::WEB_BASE_META))
        .ok()??;
    Some(AssetBase::WebUrl(element.get_attribute("content")?))
}
#[cfg(target_os = "macos")]
fn platform_base() -> Option<AssetBase> {
    static BUNDLE: std::sync::OnceLock<Option<AssetBase>> = std::sync::OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let exe = std::env::current_exe().ok()?;
            let macos = exe.parent()?;
            if macos.file_name()? != "MacOS" {
                return None;
            }
            let contents = macos.parent()?;
            if contents.file_name()? != "Contents" {
                return None;
            }
            let resources = contents.join("Resources/whisker_assets");
            resources
                .is_dir()
                .then(|| AssetBase::MacosDir(resources.to_string_lossy().into_owned()))
        })
        .clone()
}
// Keep path derivation independent of the host OS so distribution layouts can
// be tested by generators on any build machine.
#[cfg(any(target_os = "windows", target_os = "linux", test))]
fn desktop_assets(exe: &std::path::Path, linux: bool) -> Option<std::path::PathBuf> {
    let directory = exe.parent()?;
    if linux {
        if directory.file_name()? != "bin" {
            return None;
        }
        Some(
            directory
                .parent()?
                .join("share")
                .join(exe.file_name()?)
                .join("whisker_assets"),
        )
    } else {
        Some(directory.join("whisker_assets"))
    }
}
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn platform_base() -> Option<AssetBase> {
    static BASE: std::sync::OnceLock<Option<AssetBase>> = std::sync::OnceLock::new();
    BASE.get_or_init(|| {
        let assets = desktop_assets(&std::env::current_exe().ok()?, cfg!(target_os = "linux"))?;
        assets
            .is_dir()
            .then(|| AssetBase::DesktopDir(assets.to_string_lossy().replace('\\', "/")))
    })
    .clone()
}
#[cfg(not(any(
    target_arch = "wasm32",
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
)))]
fn platform_base() -> Option<AssetBase> {
    None
}

/// Strip a leading `/` and drop any `..`/`.` components so a normalized,
/// base-relative path can never escape the base. Mirrors the compile-time
/// validation in `whisker-asset-macros`.
fn normalize(rel: &str) -> String {
    // Windows treats both separators as traversal delimiters. Logical asset
    // declarations already reject backslashes; runtime callers need the same boundary.
    let portable = rel.replace('\\', "/");
    let trimmed = portable.trim_start_matches('/');
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
/// - On macOS: the discovered bundle resource directory plus the logical path.
/// - On Web: the deployment prefix plus the percent-encoded logical path.
/// - With no explicit or automatic base: the normalized relative path, unchanged.
///
/// The input is normalized first (leading `/` stripped, `..` traversal
/// removed), so the base can never be escaped.
pub fn resolve(rel: &str) -> String {
    let rel = normalize(rel);
    match BASE
        .read()
        .expect("whisker-asset BASE lock poisoned")
        .clone()
        .or_else(platform_base)
    {
        Some(base) => base.compose(&rel),
        None => rel,
    }
}

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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn whisker_asset_set_android() {
    set_base(AssetBase::AndroidAssets);
}

// The Android base is a compile-time constant, so nothing has to cross the
// Kotlin↔Rust boundary to install it — the `.init_array` constructor below
// does it at `.so` load, long before the first render, and `resolve` returns
// the `file:///android_asset/whisker/…` form on the very first call. The
// `whisker_asset_set_android()` C-ABI entry stays exported so a host that
// wants to set it explicitly still can.

/// Install the fixed Android base. Shared body for the `.init_array`
/// constructor (Android) and the unit test (host) so the exact
/// install path is exercised off-device.
///
/// On a non-Android host outside of tests this is never called (the
/// `.init_array` registration is Android-only), so silence the
/// dead-code lint there rather than dropping the shared seam.
#[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
fn install_android_base() {
    set_base(AssetBase::AndroidAssets);
}

#[cfg(target_os = "android")]
mod android_init {
    /// `extern "C"` trampoline the linker can call. Wraps the plain
    /// Rust [`install_android_base`](super::install_android_base) so
    /// the same logic is shared with the host unit test.
    extern "C" fn ctor() {
        super::install_android_base();
    }

    /// Register `ctor` in `.init_array` so the dynamic linker calls it
    /// when the `.so` is loaded. `#[used]` keeps the static from being
    /// dead-stripped (it is never referenced by Rust code); the
    /// explicit `.init_array` section is what the linker scans for
    /// constructor function pointers.
    #[used]
    #[unsafe(link_section = ".init_array")]
    static INIT_ANDROID_BASE: extern "C" fn() = ctor;
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
    fn resolve_platform_base_encodes_paths_and_preserves_deployment_prefix() {
        let _g = GUARD.lock().unwrap();
        reset();
        set_base(AssetBase::WebUrl("/app/".into()));
        assert_eq!(
            resolve("photos/日本 #1%.jpg"),
            "/app/photos/%E6%97%A5%E6%9C%AC%20%231%25.jpg"
        );
        assert_eq!(resolve("../photos/a.jpg"), "/app/photos/a.jpg");
        assert!(base_is_set());
        set_base(AssetBase::WebUrl("/".into()));
        assert_eq!(resolve("photos/a.jpg"), "/photos/a.jpg");
        reset();
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
    fn install_android_base_sets_android_form() {
        let _g = GUARD.lock().unwrap();
        reset();
        install_android_base();
        assert!(base_is_set());
        assert_eq!(resolve("a/b.png"), "file:///android_asset/whisker/a/b.png");
        reset();
    }

    #[test]
    fn normalize_strips_leading_slash() {
        let _g = GUARD.lock().unwrap();
        reset();
        set_base(AssetBase::IosDir("/base".into()));
        assert_eq!(resolve("/images/logo.png"), "/base/images/logo.png");
        reset();
    }

    #[test]
    fn normalize_drops_parent_traversal() {
        let _g = GUARD.lock().unwrap();
        reset();
        set_base(AssetBase::AndroidAssets);
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

#[cfg(test)]
mod desktop_layout_tests {
    use super::*;
    #[test]
    fn distribution_paths_are_independent_of_working_directory() {
        use std::path::{Path, PathBuf};
        assert_eq!(normalize(r"photos\..\..\logo.png"), "logo.png");
        assert_eq!(
            desktop_assets(Path::new("/opt/example/bin/example"), true),
            Some(PathBuf::from("/opt/example/share/example/whisker_assets"))
        );
        assert_eq!(
            desktop_assets(Path::new("/portable/example.exe"), false),
            Some(PathBuf::from("/portable/whisker_assets"))
        );
        assert_eq!(
            desktop_assets(Path::new("/tmp/target/debug/example"), true),
            None
        );
        assert_eq!(
            AssetBase::DesktopDir("C:/Program Files/Example/whisker_assets".into())
                .compose("photos/a.jpg"),
            "C:/Program Files/Example/whisker_assets/photos/a.jpg"
        );
    }
}
