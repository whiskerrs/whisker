# Whisker Module Author Guide

This guide walks through writing a Whisker Module — a cargo crate that
ships Rust + Kotlin + Swift sources together and publishes to
**crates.io alone** (no separate Maven Central / SwiftPM Registry /
CocoaPods steps).

Inspired by Expo Modules, which ship iOS + Android sources inside an
npm package; Whisker bundles them inside a cargo crate the same way.

## What a Whisker Module is

A Whisker Module is one of two flavors:

- **Function-only module** — exposes typed Rust → Kotlin/Swift function
  calls that don't render any UI. `whisker-local-store` is the
  reference (wraps `UserDefaults` / `SharedPreferences`).
- **View-bearing component** — renders a platform-native view that
  Whisker apps can place inside a `render! { … }` tree.
  `whisker-video` is the reference (Big Buck Bunny player on
  `AVPlayer` / `Media3 ExoPlayer`).

Both flavors live in the same Module DSL — a View is just a feature
of the Module definition.

## Distribution model — cargo only

A published Whisker Module is **just a crate on crates.io**.
The `.crate` artifact (the tarball cargo uploads) contains:

```
whisker-foo-0.1.0/
├── Cargo.toml
├── README.md
├── whisker.module.toml
├── Package.swift        ← SPM manifest for the iOS half
├── build.gradle.kts     ← Gradle subproject for the Android half
└── src/
    ├── lib.rs           ← Rust-side `#[whisker::platform_component]` proxy
    ├── ios/             ← Swift sources
    │   └── WhiskerFooComponent.swift
    └── android/         ← Kotlin sources
        └── WhiskerFooComponent.kt
```

When the user app builds, `whisker-build` does the following:

1. `cargo metadata` enumerates every transitive dependency of the app.
2. For each dep, look for `whisker.module.toml` next to its
   `Cargo.toml`. If present, the dep is a Whisker Module.
3. Read `[ios].swift_sources` + `[android].kotlin_sources` from the
   module's `whisker.module.toml` — these are paths *relative to the
   crate root* that list the platform sources to surface to the host
   project.
4. Stage them into the user's `gen/ios/whisker_modules/` and
   `gen/android/` source trees. SwiftPM and Gradle pick them up
   exactly like any other source under the host project's tree.

Step 3 works identically whether the dep was resolved from a local
`path = "..."`, a git ref, or `~/.cargo/registry/src/index.crates.io-*/`.
The cargo crate's tarball contents are the contract — no separate
package registries are involved.

## Authoring a module

### 1. Create the cargo crate

```toml
# Cargo.toml
[package]
name = "whisker-foo"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
description = "Whisker module — short tagline that shows up on crates.io."

# Explicit include so `cargo publish` ships all the non-Rust files.
# Default would include them too, but being explicit prevents
# accidental Gradle / Xcode build-artifact leaks if a stale build
# dir lingers next to the manifest.
include = [
    "Cargo.toml",
    "Package.swift",
    "build.gradle.kts",
    "whisker.module.toml",
    "src/lib.rs",
    "src/android/**/*.kt",
    "src/ios/**/*.swift",
    "README.md",
]

[lib]
crate-type = ["rlib"]

[dependencies]
# Rename `whisker-modules-api` → `whisker` so the proc macros' emit
# paths (`::whisker::ElementRef`, `::whisker::platform_module::WhiskerValue`,
# …) resolve. Cargo doesn't allow `package = ...` with
# `workspace = true`, so the version + path are inlined here. Drop
# the `path = "..."` when you publish — `version = "x.y.z"` alone is
# what consumers see from crates.io.
whisker = { package = "whisker-modules-api", version = "0.1" }
```

### 2. Declare platform sources in `whisker.module.toml`

```toml
# whisker.module.toml — at the crate root, sibling of Cargo.toml.

[ios]
# Swift sources to stage into the host app's
# gen/ios/whisker_modules/<crate>/. Paths are relative to this manifest.
swift_sources = ["src/ios/WhiskerFooComponent.swift"]

[android]
# Kotlin sources for the host app's gen/android/<crate>/ subproject.
kotlin_sources = ["src/android/WhiskerFooComponent.kt"]
```

### 3. Add per-platform `Package.swift` and `build.gradle.kts`

These are SwiftPM + Gradle subproject manifests for the *consuming*
app's host build. They depend on `WhiskerModuleApi` / `:module-api`
respectively (provided by Whisker itself), plus whatever the module
needs (third-party SwiftPM dep, AndroidX library, etc.).

See [`packages/whisker-video/Package.swift`](../packages/whisker-video/Package.swift)
and [`packages/whisker-video/build.gradle.kts`](../packages/whisker-video/build.gradle.kts)
for the reference shapes.

### 4. Write the Rust shim

```rust
// src/lib.rs
use whisker::platform_module::WhiskerValue;
use whisker::{ElementRef, Signal};

#[whisker::platform_component("Foo")]
pub fn foo(src: Signal<String>, style: Signal<String>) {}

#[whisker::element_methods(FooProps)]
pub trait FooSys {
    fn play(&self, args: Vec<WhiskerValue>) -> WhiskerValue;
}

pub trait FooControls {
    fn play(&self);
}

impl FooControls for ElementRef<FooProps> {
    fn play(&self) {
        let _ = FooSys::play(self, vec![]);
    }
}
```

The proc macro auto-prepends `env!("CARGO_PKG_NAME")` to the local
tag name so the registration string becomes `whisker-foo:Foo` — two
unrelated modules can both declare a `Foo` component without colliding
in Lynx's behavior registry. The platform-side `@WhiskerComponent`
annotations do the same prefixing.

### 5. Write the Swift + Kotlin component classes

```swift
// src/ios/WhiskerFooComponent.swift
import UIKit
import WhiskerComponents
import WhiskerModuleApi

@WhiskerComponent("Foo")
@objc(WhiskerFooComponent)
public final class WhiskerFooComponent: WhiskerUI<UIView> {
    @objc public override func createView() -> UIView { UIView() }

    @WhiskerProp("src")
    @objc public func setSrc(_ value: NSString, requestReset: Bool) { … }

    @WhiskerUIMethod
    public func play(_ args: [WhiskerValue]) -> WhiskerValue {
        // …
        return .null
    }
}
```

```kotlin
// src/android/WhiskerFooComponent.kt
package rs.whisker.modules.foo

import android.content.Context
import android.view.View
import rs.whisker.annotations.WhiskerComponent
import rs.whisker.annotations.WhiskerProp
import rs.whisker.annotations.WhiskerUIMethod
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI
import rs.whisker.runtime.WhiskerValue

@WhiskerComponent("Foo")
open class WhiskerFooComponent(context: WhiskerContext) : WhiskerUI<View>(context) {

    override fun createView(context: Context): View = View(context)

    @WhiskerProp("src")
    open fun setSrc(value: String) { /* … */ }

    @WhiskerUIMethod
    open fun play(args: List<WhiskerValue>): WhiskerValue {
        // …
        return WhiskerValue.Null
    }
}
```

### 6. Publish

```sh
cargo publish -p whisker-foo
```

That's it. Consumers add `cargo add whisker-foo` and Whisker's build
pipeline finds the platform sources via `cargo metadata` against the
registry cache.

## Consumer side

A Whisker app that wants to use `whisker-foo`:

```toml
# app/Cargo.toml
[dependencies]
whisker-foo = "0.1"
```

```rust
// app/src/lib.rs
use whisker::prelude::*;
use whisker_foo::{Foo, FooControls};

#[whisker::main]
fn app() -> Element {
    let foo_ref = element_ref::<FooProps>();
    render! {
        Foo(ref: foo_ref, src: "https://example.com/clip.mp4")
        view(on_tap: move || foo_ref.play()) {
            text(value: "Play")
        }
    }
}
```

No separate `Podfile`, `Package.swift`, `build.gradle` change required
in the consuming app. `whisker run --target ios` / `--target android`
picks up the new dep automatically through cargo metadata + the
host-project staging step.

## Directory layout reference

```
whisker-foo/
├── Cargo.toml              ← the cargo manifest
├── whisker.module.toml     ← declares which platform sources exist
├── Package.swift           ← SwiftPM subproject for the host app
├── build.gradle.kts        ← Gradle subproject for the host app
├── README.md
└── src/
    ├── lib.rs              ← Rust shim
    ├── android/            ← Kotlin platform sources
    │   └── …Component.kt
    └── ios/                ← Swift platform sources
        └── …Component.swift
```

The Rust `src/lib.rs` and the platform `src/{android,ios}/` siblings
share the `src/` parent intentionally — cargo's default crate layout
puts the Rust root there, and we co-locate the platform code alongside
rather than separating into top-level `android/` / `ios/` directories.
Keeps a single `src/` to glance at when navigating the crate.

## What goes where — quick reference

| Symbol | Crate / module | Used by |
|---|---|---|
| `#[whisker::platform_component("Tag")]` | `whisker-modules-api` (proc macro) | View-bearing modules |
| `#[whisker::platform_module(name = "X")]` | `whisker-modules-api` (proc macro) | Function-only modules |
| `#[whisker::element_methods(Props)]` | `whisker-modules-api` (proc macro) | Typed `ElementRef<T>::method()` dispatch |
| `WhiskerValue`, `WhiskerModuleError` | `whisker::platform_module` | Both flavors |
| `Signal<T>`, `ElementRef<T>`, `element_ref()` | `whisker` (top-level) | View-bearing modules' shim |
| `@WhiskerComponent("Tag")` | `WhiskerComponents` SPM target / `rs.whisker.annotations.WhiskerComponent` | iOS / Android view classes |
| `@WhiskerModule("Name")` | same | iOS / Android function-only classes |
| `@WhiskerProp("name")` / `@WhiskerUIMethod` | same | iOS / Android dispatch annotations |
| `WhiskerUI<View>` / `WhiskerContext` / `WhiskerValue` | `WhiskerModuleApi` SPM target / `rs.whisker.runtime` Kotlin package | iOS / Android view classes |

## Future direction

Phase L (#58) replaces the `@WhiskerComponent` / `@WhiskerProp` /
`@WhiskerUIMethod` annotation surface with an Expo-style
`ModuleDefinition` DSL. View-bearing and function-only modules will
both use the same `definition() -> ModuleDefinition` entry point;
`View(...) { Prop(...) ... }` becomes a feature of the DSL rather
than a separate annotation set.

See the Whisker Modules epic (#55) for the broader roadmap.
