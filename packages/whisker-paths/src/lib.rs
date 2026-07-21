//! `whisker-paths` — resolve the OS's per-app directories.
//!
//! Whisker apps run their Rust on-device, so [`std::fs`] already does
//! every file operation (read, write, `create_dir_all`, `copy`,
//! `rename`, `read_dir`, …) directly. The one thing `std` *can't* do is
//! find the platform's per-app directories, whose absolute paths the OS
//! only hands out at runtime — there is no portable "app cache dir" in
//! `std`. This crate supplies exactly those paths and nothing else; you
//! use ordinary `std::fs` against them.
//!
//! ```ignore
//! let dir = whisker_paths::cache_dir().join("thumbnails");
//! std::fs::create_dir_all(&dir)?;
//! std::fs::write(dir.join("cover.jpg"), &bytes)?;
//! ```
//!
//! ## The four directories
//!
//! | fn | lifetime | iOS | Android |
//! |----|----------|-----|---------|
//! | [`cache_dir`]    | evictable under storage pressure | `NSCachesDirectory` | `Context.getCacheDir()` |
//! | [`document_dir`] | persistent, user data, backed up  | `NSDocumentDirectory` | `Context.getFilesDir()` |
//! | [`support_dir`]  | persistent, non-user, backed up   | `NSApplicationSupportDirectory` | `filesDir/support` |
//! | [`temp_dir`]     | cleared aggressively by the OS    | `NSTemporaryDirectory()` | `cacheDir/tmp` |
//!
//! Pick [`cache_dir`] for regenerable data (downloaded thumbnails,
//! HTTP response caches), [`document_dir`] for data the user would be
//! upset to lose, [`support_dir`] for app-managed persistent state that
//! isn't user content, and [`temp_dir`] for short-lived scratch.
//!
//! The returned directory is guaranteed to be a valid path but **may
//! not exist yet** — create it with [`std::fs::create_dir_all`] before
//! writing. All four are resolved once from the native module and
//! cached for the process lifetime (an app's sandbox paths never move).
//!
//! ## Native source
//!
//! The matching platform module lives at
//!
//! - iOS: `packages/whisker-paths/ios/Sources/WhiskerPaths/PathsModule.swift`
//!   (resolver: `Paths.swift`)
//! - Android: `packages/whisker-paths/android/src/main/kotlin/rs/whisker/modules/paths/PathsModule.kt`
//!   (resolver: `Paths.kt`)

use std::path::PathBuf;
use std::sync::OnceLock;

use whisker::platform_module::WhiskerValue;

/// The four resolved directories. Populated once via the native module.
struct Directories {
    cache: PathBuf,
    document: PathBuf,
    support: PathBuf,
    temp: PathBuf,
}

fn directories() -> &'static Directories {
    static DIRS: OnceLock<Directories> = OnceLock::new();
    DIRS.get_or_init(resolve)
}

fn resolve() -> Directories {
    let result = whisker::module!("WhiskerPaths").invoke("directories", Vec::new());

    if let WhiskerValue::Map(map) = &result {
        let get = |k: &str| match map.get(k) {
            Some(WhiskerValue::String(s)) => Some(PathBuf::from(s)),
            _ => None,
        };
        if let (Some(cache), Some(document), Some(support), Some(temp)) =
            (get("cache"), get("document"), get("support"), get("temp"))
        {
            return Directories {
                cache,
                document,
                support,
                temp,
            };
        }
    }

    // The native module isn't registered (or returned an unexpected
    // shape). Degrade to temp-based paths so file I/O keeps working, but
    // warn — the app almost certainly forgot to add whisker-paths to its
    // build.
    eprintln!(
        "whisker-paths: `WhiskerPaths.directories()` unavailable ({result:?}); \
         falling back to std::env::temp_dir(). Ensure the whisker-paths native \
         module is included in your app build."
    );
    let base = std::env::temp_dir().join("whisker-paths-fallback");
    Directories {
        cache: base.join("cache"),
        document: base.join("document"),
        support: base.join("support"),
        temp: base.join("temp"),
    }
}

/// App cache directory. May be evicted by the OS under storage
/// pressure, so store only regenerable data here.
///
/// iOS: `NSCachesDirectory`. Android: `Context.getCacheDir()`.
pub fn cache_dir() -> PathBuf {
    directories().cache.clone()
}

/// Persistent user-document directory, included in device backups. Use
/// for data the user would be upset to lose.
///
/// iOS: `NSDocumentDirectory`. Android: `Context.getFilesDir()`.
pub fn document_dir() -> PathBuf {
    directories().document.clone()
}

/// Persistent application-support directory (non-user data), included
/// in device backups. Use for app-managed state that isn't user
/// content.
///
/// iOS: `NSApplicationSupportDirectory`. Android: a `support`
/// subdirectory of `Context.getFilesDir()`.
pub fn support_dir() -> PathBuf {
    directories().support.clone()
}

/// Temporary directory, cleared aggressively by the OS. Use for
/// short-lived scratch files only.
///
/// iOS: `NSTemporaryDirectory()`. Android: a `tmp` subdirectory of
/// `Context.getCacheDir()`.
pub fn temp_dir() -> PathBuf {
    directories().temp.clone()
}
