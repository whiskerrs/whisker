# `whisker.module.toml` — schema reference

The manifest file every Whisker native module / native element crate
ships at its root. Read by `whisker-build` during autolink; consumed
by `whisker-cng` to materialise the module into the user app's
`gen/android/` and `gen/ios/` projects.

Status: **schema v1** (Phase 7-A). Deliberately minimal — see
[Why so minimal?](#why-so-minimal) below. Adding new fields is
non-breaking; removing fields or changing semantics is breaking and
will gate on a top-level `schema = 2` migration.

## File location

Lives **at the crate root, next to `Cargo.toml`**:

```
whisker-module-camera/
├── Cargo.toml
├── whisker.module.toml      ← this file
├── src/lib.rs
├── android/
│   ├── build.gradle.kts
│   └── src/main/kotlin/.../Camera.kt
└── ios/
    ├── Package.swift
    └── Sources/Camera/Camera.swift
```

### Why `whisker.module.toml`, not `whisker.toml`

`whisker.toml` is reserved for **app-level configuration** (target
selection, signing identity, bundle id, etc.) that a Whisker
*application* crate will eventually need. The dot-separated
namespace lets future siblings — `whisker.app.toml`,
`whisker.workspace.toml` — slot in without renaming this one.

## Full schema

```toml
[android]
gradle_project = "android"            # path to gradle module (relative to crate root)

[ios]
swift_package = "ios"                 # path to SPM package (relative to crate root)
```

That's the entire v1 schema. **No `[module]`, no `kind`, no `name`,
no `version`, no module / element listings.** See
[Why so minimal?](#why-so-minimal).

### `[android]` section

Optional. Omit when the module is iOS-only.

| Field | Type | Required | Notes |
|-------|------|---------|-------|
| `gradle_project` | path | yes | Path (relative to crate root) to the module's gradle project. `whisker-cng` adds this via `includeBuild()` to the user app's `settings.gradle.kts`. The path's contents are a **real** gradle module — open in Android Studio for full IDE autocomplete. Dependencies, AndroidManifest entries, ProGuard rules all live inside the gradle project itself. |

### `[ios]` section

Optional. Omit when the module is Android-only.

| Field | Type | Required | Notes |
|-------|------|---------|-------|
| `swift_package` | path | yes | Path (relative to crate root) to the module's Swift Package directory (the one containing `Package.swift`). `whisker-cng` adds it as a local package reference to the user app's xcodeproj. As with Android, the path's contents are a **real** SPM package — open `Package.swift` in Xcode for full IDE autocomplete. Native deps live in `Package.swift`. |

### Platform expression

The presence / absence of sections expresses platform support:

| `[android]` | `[ios]` | Means |
|---|---|---|
| ✓ | ✓ | Cross-platform module |
| ✓ | ✗ | Android-only |
| ✗ | ✓ | iOS-only |
| ✗ | ✗ | No `whisker.module.toml` needed — this is just a normal Rust crate |

## Why so minimal?

Every piece of information you might expect in this file already
lives elsewhere — `Cargo.toml` for crate identity, the native config
files for dependencies, the source annotations for module/element
identifiers. Duplicating any of it would create two sources of
truth that can drift apart.

| Question | Authoritative source |
|----------|---------------------|
| What's this crate called? | `Cargo.toml` `[package].name` |
| What version? | `Cargo.toml` `[package].version` |
| What native modules does it register? | `@WhiskerModule(name = "Camera")` in Kotlin / `@WhiskerModule("Camera")` in Swift / `#[whisker::native_module("Camera")]` in Rust |
| What native elements? Which tags? | `@WhiskerElement(tag = "x-input")` in Kotlin / Swift / `#[whisker::native_element("x-input")]` in Rust |
| Android Gradle dependencies? | The module's own `build.gradle.kts` |
| AndroidManifest permissions, services, activities? | The module's own `AndroidManifest.xml` |
| iOS SPM dependencies? | The module's own `Package.swift` |
| Info.plist additions? | The module's own `Info.plist` |
| Whisker version compatibility? | `Cargo.toml` `[dependencies].whisker = "0.x"` — cargo's semver resolution handles it |

The role of `whisker.module.toml` is purely: **"this crate has
native code that should be autolinked, and here are the paths to
the gradle / SPM projects"**. Everything else is inferred.

## Multiple modules / elements per crate

Supported, with no schema change. A single crate can register any
number of modules and / or elements — the count is decided by how
many `@Whisker*` annotations live in the native sources:

```kotlin
// android/src/main/kotlin/com/example/Essentials.kt

@WhiskerElement(tag = "x-input")
class InputElement(ctx: WhiskerContext) : WhiskerView<EditText>(ctx) { … }

@WhiskerElement(tag = "x-refresh")
class RefreshElement(ctx: WhiskerContext) : WhiskerView<SwipeRefreshLayout>(ctx) { … }

@WhiskerModule(name = "FileSystem")
class FileSystemModule(ctx: WhiskerContext) { … }
```

```rust
// src/lib.rs

#[whisker::native_element("x-input")]
pub struct Input { … }

#[whisker::native_element("x-refresh")]
pub struct Refresh { … }

#[whisker::native_module("FileSystem")]
pub trait FileSystem { … }
```

`whisker.module.toml` stays the same — just `[android]` +
`[ios]`. KSP / Swift Macros pick up every `@Whisker*` annotation
in the gradle / SPM project; the Rust proc-macros emit one client
per `#[whisker::*]` invocation.

### Naming conventions (no enforcement)

| Pattern | Example | Used for |
|---------|---------|---------|
| `whisker-module-<name>` | `whisker-module-camera` | A single native module |
| `whisker-element-<name>` | `whisker-element-input` | A single native element |
| `whisker-elements-<group>` | `whisker-elements-essentials` | A bundle of related elements |
| `whisker-<feature>` | `whisker-camera` | Mixed module + element bundle |

These are conventions only. `whisker-build` doesn't enforce any
pattern.

## Examples

### Native module (Android + iOS)

```toml
[android]
gradle_project = "android"

[ios]
swift_package = "ios"
```

```
whisker-module-localstorage/
├── Cargo.toml
├── whisker.module.toml
├── src/lib.rs                          # #[whisker::native_module("LocalStorage")] trait
├── android/
│   ├── build.gradle.kts
│   └── src/main/kotlin/.../LocalStorageModule.kt
└── ios/
    ├── Package.swift
    └── Sources/LocalStorage/LocalStorageModule.swift
```

### iOS-only element

```toml
[ios]
swift_package = "ios"
```

`[android]` is absent → `whisker-build` skips this crate when
building for Android.

### Local module in user app

```
my-app/
├── Cargo.toml
├── src/main.rs
└── modules/
    └── my-analytics/
        ├── Cargo.toml
        ├── whisker.module.toml         # ← same schema as published modules
        ├── src/lib.rs
        ├── android/…
        └── ios/…
```

```toml
# my-app/Cargo.toml
[dependencies]
my-analytics = { path = "modules/my-analytics" }
```

The autolink discovery walks the user's `cargo metadata` and finds
`my-analytics`'s `whisker.module.toml` via the path dep — no
distinction from a crates.io-resolved module.

## Discovery flow (informative)

`whisker-build::modules::discover_modules(workspace_root, package)`:

1. Run `cargo metadata --manifest-path <pkg>/Cargo.toml --format-version 1`.
2. For every resolved package in the dependency graph (including
   path deps): check `<crate_root>/whisker.module.toml`.
3. Parse each found file into a `ModuleManifest` struct (the type
   lives in `whisker-cng::module_manifest`).
4. Return the vector.

`whisker-cng` then walks the vector to materialise `gen/android/`
and `gen/ios/`.

## Conflict handling (cross-reference)

The autolink layer enforces the following at gen-time or surfaces
errors at native compile / runtime:

- **`gradle_project` / `swift_package` path doesn't exist** →
  hard error at autolink with the manifest line.
- **Two modules registering the same `@WhiskerModule(name = "X")`** →
  Lynx-side registration error at runtime startup.
- **Two elements registering the same tag** → Lynx-side
  registration error at runtime startup (LazyRegister duplicate
  detection).
- **Native dependency version mismatches across modules** → Gradle's
  resolution strategy picks one + emits a warning; SPM ditto.

For comprehensive pre-flight checking of these (so the user sees
them at `whisker build` time instead of at runtime), a `whisker
doctor` extension can scan `@Whisker*` annotations / proc-macro
invocations across all modules — deferred until Phase 8+ if it
matters.

## Future fields (not in v1)

Designed-but-not-implemented fields that v2 may add when concrete
demand appears. Listed here so v1 manifests survive the migration:

| Field | Purpose | Trigger to add |
|-------|--------|---------------|
| `[[elements]] tag_names = [...]` | Pre-flight tag conflict detection without parsing Kotlin/Swift | When duplicate-tag errors at runtime become a frequent papercut |
| `[[modules]] name = "..."` | Pre-flight module-name conflict detection | Same as above for modules |
| `[android.prebuilt] maven = "..."` | Binary cache (skip Kotlin compile) | When build times for popular modules become a bottleneck |
| `[ios.prebuilt] xcframework_url = "..."` | Same on iOS | Same as above |
| `[requires] whisker = ">=0.8"` | Min host-Whisker version | When `Cargo.toml` semver isn't enough (rare) |
| `[supports] platforms = ["ios"]` | Per-item platform exclusion within a multi-item crate | When a single crate needs different platform sets per item |

Adding any of these is non-breaking — v1 manifests stay valid.

## Filename precedent

| Prior art | Filename | Decision |
|-----------|---------|---------|
| `tsconfig.json` | TypeScript compiler config — short, dot-named | too generic for a per-module manifest |
| `expo-module.config.json` | Expo modules | rejected: long, JSON not TOML, doesn't follow the host package's filename style |
| `Cargo.toml` | Rust packages | rejected: clash with cargo itself |
| `whisker.toml` | considered | rejected: reserved for app-level config |
| `whisker.module.toml` | **chosen** | dot-separated namespace lets future `whisker.app.toml` / `whisker.workspace.toml` slot in without rename |

## Stability

Schema v1 is stable for the Phase 7 / Whisker 0.x line. Additions
that preserve v1 behaviour are non-breaking and don't require a
manifest version bump. Breaking changes (removing fields, changing
semantics) introduce a `schema = 2` top-level field with a
migration guide.
