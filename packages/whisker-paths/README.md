# whisker-paths

Resolve the OS's per-app directories (cache, documents, application-support, temp) for a Whisker app.

Whisker apps run their Rust on-device, so [`std::fs`](https://doc.rust-lang.org/std/fs/) already performs every file operation directly. The one thing `std` can't do is find the platform's per-app directories, whose absolute paths the OS only hands out at runtime. `whisker-paths` supplies exactly those paths — you use ordinary `std::fs` against them.

```rust
let dir = whisker_paths::cache_dir().join("thumbnails");
std::fs::create_dir_all(&dir)?;
std::fs::write(dir.join("cover.jpg"), &bytes)?;
```

## API

| fn | lifetime | iOS | Android |
|----|----------|-----|---------|
| `cache_dir()`    | evictable under storage pressure | `NSCachesDirectory` | `Context.getCacheDir()` |
| `document_dir()` | persistent, user data, backed up  | `NSDocumentDirectory` | `Context.getFilesDir()` |
| `support_dir()`  | persistent, non-user, backed up   | `NSApplicationSupportDirectory` | `filesDir/support` |
| `temp_dir()`     | cleared aggressively by the OS    | `NSTemporaryDirectory()` | `cacheDir/tmp` |

Each returns a `PathBuf`. The directory is a valid path but **may not exist yet** — create it with `std::fs::create_dir_all` before writing. All four are resolved once from the native module and cached for the process lifetime.

### Backup exclusion

```rust
whisker_paths::set_excluded_from_backup(&downloads_dir, true)?;
```

Excludes a file/directory from device backup — on iOS it sets `NSURLIsExcludedFromBackupKey` (required for re-downloadable content under `document_dir`, or Apple rejects the app); on Android it's a no-op (backup exclusion is manifest-level). The flag lives on the inode, so calling it once on a directory covers all its children.

## Choosing a directory

- **`cache_dir()`** — regenerable data (downloaded thumbnails, HTTP caches). The OS may delete it under storage pressure.
- **`document_dir()`** — data the user would be upset to lose. Included in device backups.
- **`support_dir()`** — app-managed persistent state that isn't user content. Included in device backups.
- **`temp_dir()`** — short-lived scratch. Cleared aggressively.

## Setup

Add the dependency and the native module ships automatically via the Whisker module system:

```toml
[dependencies]
whisker-paths = "0.8"
```

If the native module isn't present at runtime, the accessors log a warning and fall back to `std::env::temp_dir()`-based paths so file I/O still works.

## Native source

- iOS: `ios/Sources/WhiskerPaths/PathsModule.swift` (resolver: `Paths.swift`)
- Android: `android/src/main/kotlin/rs/whisker/modules/paths/PathsModule.kt` (resolver: `Paths.kt`)
