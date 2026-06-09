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
├── Cargo.toml           ← carries the `[package.metadata.whisker]` marker
├── README.md
├── Package.swift        ← SwiftPM manifest (MUST sit at the package
│                          root — SwiftPM derives the package identity
│                          from the root dir name, so it can't live in
│                          a per-module `ios/` dir without colliding
│                          with every other module's `ios/`)
├── build.gradle.kts     ← Gradle library manifest (kept at the root
│                          too, for symmetry; its source set points at
│                          `android/` below)
├── src/
│   └── lib.rs           ← Rust-side `#[whisker::platform_component]` proxy
├── ios/                 ← Swift sources (Expo-style: native code grouped
│   └── Sources/WhiskerFoo/   per-platform; Package.swift `path:` targets this)
│       ├── FooModule.swift    ← `@WhiskerModule` DSL module
│       └── FooView.swift      ← the `WhiskerUI<UIView>` subclass
└── android/             ← Kotlin sources (build.gradle.kts `srcDirs`
    └── src/main/kotlin/      points here; standard AGP nesting)
        └── rs/whisker/modules/foo/
            ├── FooModule.kt    ← `@WhiskerModule` DSL module
            └── FooView.kt      ← the `WhiskerUI<View>` subclass
```

The native code lives in `ios/` and `android/` at the package root
(grouped by platform, the way Expo Modules / most native libraries
organise it), while the three build manifests (`Cargo.toml`,
`Package.swift`, `build.gradle.kts`) stay at the root. `Package.swift`
*has* to be at the root — SwiftPM keys a local package's identity off
its directory name, so a `Package.swift` inside `ios/` would make the
package identity `ios` and collide with every other module's `ios/`
package (and with `platforms/ios`). The `path:` on its target points
into `ios/Sources/<Module>/`; the Gradle build's `srcDirs` points into
`android/src/main/kotlin/`.

When the user app builds, `whisker-build` does the following:

1. `cargo metadata` enumerates every transitive dependency of the app.
2. For each dep, check for a `[package.metadata.whisker]` table in its
   `Cargo.toml`. If present, the dep is a Whisker Module.
3. Discover the per-platform manifests at the crate root: `Package.swift`
   marks the iOS SwiftPM package, `build.gradle.kts` the Android Gradle
   library. Those manifests' own `path:` / `srcDirs` are the
   source-of-truth for which files compile — there's no source list in
   the cargo metadata.
4. Wire them into the user's host project: the iOS package is referenced
   via `.package(path: …)` in the generated SwiftPM aggregator; the
   Android library is included as a Gradle subproject. SwiftPM and
   Gradle compile them exactly like any other dependency.

Step 2 works identically whether the dep was resolved from a local
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
    "src/lib.rs",
    "android/**/*.kt",
    "ios/**/*.swift",
    "README.md",
]

[lib]
crate-type = ["rlib"]

# The marker that makes this crate a Whisker module (see step 2).
[package.metadata.whisker]

[dependencies]
# The umbrella `whisker` crate — the same dep app crates use. Module
# crates reach the proc macros + `ElementRef` / `Signal` /
# `platform_module::*` through it; the macros' emit paths
# (`::whisker::…`) resolve under the `whisker` name.
whisker = "0.1"
```

### 2. Mark the crate as a module — `[package.metadata.whisker]`

```toml
# In Cargo.toml. The bare table's *presence* marks the crate as a
# Whisker module. The per-platform build manifests are the
# source-of-truth for which sources compile, so no source-file list
# is needed here:
#   - Package.swift       discovers the iOS SwiftPM target
#   - build.gradle.kts    discovers the Android Gradle library
[package.metadata.whisker]
```

`whisker-build` walks the app's cargo dep tree, picks out every dep
carrying this table, then discovers the iOS package via `Package.swift`
(at the crate root) and the Android library via `build.gradle.kts`
(also at the root); both manifests' own `path:` / `srcDirs` point at
`ios/` and `android/`. There's no source list to keep in sync — that
was a frequent stale-path foot-gun, so it's gone. (Storing the marker
in `Cargo.toml` rather than a separate `whisker.module.toml` keeps a
module's metadata in one file that `cargo` already validates.)

### 3. Add the per-platform `Package.swift` and `build.gradle.kts`

These are the SwiftPM + Gradle manifests the *consuming* app's host
build references (`.package(path: …)` for iOS, a Gradle subproject for
Android). They depend on `WhiskerModule` / `:module`
respectively (provided by Whisker itself), plus whatever the module
needs (third-party SwiftPM dep, AndroidX library, etc.). Both live at
the crate root; `Package.swift`'s target `path:` points at
`ios/Sources/<Module>/` and `build.gradle.kts`'s `srcDirs` points at
`android/src/main/kotlin/`.

See [`packages/whisker-video/Package.swift`](../packages/whisker-video/Package.swift)
and [`packages/whisker-video/build.gradle.kts`](../packages/whisker-video/build.gradle.kts)
for the reference shapes.

### 4. Write the Rust shim

A **view module** (element + imperative methods):

```rust
// src/lib.rs
use whisker::platform_module::WhiskerValue;
use whisker::{ElementRef, Signal};

// The element for `render!`. Methods on a mounted instance dispatch
// through its `ElementRef` (the `ref:` prop, bound on mount).
#[whisker::module_component("Foo")]
pub fn foo(src: Signal<String>, style: Signal<String>) {}

// Typed imperative handle end-users hold. Wraps an `ElementRef`;
// each method dispatches via `ElementRef::invoke(method, args)` over
// the raw `Vec<WhiskerValue>` wire.
#[derive(Copy, Clone)]
pub struct FooHandle { r: ElementRef }
impl FooHandle {
    pub fn new() -> Self { Self { r: ElementRef::new() } }
    pub fn r(&self) -> ElementRef { self.r }   // pass to `Foo(ref: …)`
    pub fn play(&self) { let _ = self.r.invoke("play", vec![]); }
    pub fn seek(&self, t: f64) { let _ = self.r.invoke("seek", vec![WhiskerValue::Float(t)]); }
}
```

A **function-only module** (service, no element):

```rust
use whisker::platform_module::{WhiskerModuleError, WhiskerValue};

pub struct FooStore;
impl FooStore {
    pub fn save(key: String, value: String) -> Result<bool, WhiskerModuleError> {
        // `module!` prepends this crate's name → `<crate>:FooStore`,
        // dispatching to the native module registered under that key.
        match whisker::module!("FooStore").invoke(
            "save", vec![WhiskerValue::String(key), WhiskerValue::String(value)],
        ) {
            WhiskerValue::Bool(b)  => Ok(b),
            WhiskerValue::Error(m) => Err(WhiskerModuleError(m)),
            o => Err(WhiskerModuleError(format!("expected Bool, got {o:?}"))),
        }
    }
}
```

`#[whisker::module_component]` auto-prepends `env!("CARGO_PKG_NAME")` to
the tag (`whisker-foo:Foo`), and `whisker::module!` does the same for
function-only module names (`whisker-foo:FooStore`) — so two unrelated
crates can ship same-named elements/modules without colliding. The
platform-side DSL `Name("Foo")` is namespaced the same way by the
per-platform codegen.

There are no binding-generating macros: the wire is raw
`Vec<WhiskerValue>`, and the typed handle / wrapper is hand-written so
a conversion mistake surfaces as a loggable `WhiskerValue` rather than
silently producing nothing.

### 5. Write the Swift + Kotlin module

Modules are authored with the ModuleDefinition DSL (modeled on Expo
Modules). A class annotated `@WhiskerModule` subclasses `Module` and
overrides `definition()`. The `@WhiskerModule` attribute is the
registration trigger — the SwiftPM codegen plugin (iOS) and the KSP
processor (Android) discover it and emit the Lynx registration. A
view-bearing module declares a `View(...)` block referencing a
`WhiskerUI<View>` subclass; a function-only module omits it and
declares module-level `Function`s instead.

```swift
// ios/Sources/WhiskerFoo/FooModule.swift
import WhiskerModuleMacros   // @WhiskerModule
import WhiskerModule    // Module, ModuleDefinition, DSL

@WhiskerModule
public final class FooModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Foo")
            View(FooView.self) {
                Prop("src") { (view: FooView, value: String) in view.setSrc(value) }
                Function("play") { (view: FooView) in view.play() }
            }
        }
    }
}

// ios/Sources/WhiskerFoo/FooView.swift
import UIKit
import WhiskerModule

@objc(FooView)
public final class FooView: WhiskerUI<UIView> {
    @objc public override func createView() -> UIView { UIView() }
    func setSrc(_ value: String) { /* … */ }
    func play() { /* … */ }
}
```

```kotlin
// android/src/main/kotlin/rs/whisker/modules/foo/FooModule.kt
package rs.whisker.modules.foo

import rs.whisker.annotations.WhiskerModule
import rs.whisker.runtime.Module        // NB: explicit — else java.lang.Module shadows
import rs.whisker.runtime.ModuleDefinition

@WhiskerModule
class FooModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Foo")
        View(FooView::class.java) {
            Prop("src") { view: FooView, value: String -> view.setSrc(value) }
            Function("play") { view: FooView -> view.play() }
        }
    }
}

// android/src/main/kotlin/rs/whisker/modules/foo/FooView.kt
package rs.whisker.modules.foo

import android.content.Context
import android.view.View
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI

open class FooView(context: WhiskerContext) : WhiskerUI<View>(context) {
    override fun createView(context: Context): View = View(context)
    fun setSrc(value: String) { /* … */ }
    fun play() { /* … */ }
}
```

> `whisker new-module <name>` scaffolds this whole skeleton — Cargo.toml
> with the marker, both manifests, the Rust shim, and the DSL module +
> view stubs. Pass `--shape function-only` for a view-less module.

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
in the consuming app. `whisker run ios` / `whisker run android` picks
up the new dep automatically through cargo metadata + the host-project
staging step.

## Directory layout reference

```
whisker-foo/
├── Cargo.toml              ← cargo manifest (carries [package.metadata.whisker])
├── Package.swift           ← SwiftPM subproject for the host app
├── build.gradle.kts        ← Gradle subproject for the host app
├── README.md
├── src/
│   └── lib.rs              ← Rust shim
├── ios/                    ← Swift platform sources (Expo-style)
│   └── Sources/WhiskerFoo/
│       ├── FooModule.swift
│       └── FooView.swift
└── android/                ← Kotlin platform sources (standard AGP nesting)
    └── src/main/kotlin/rs/whisker/modules/foo/
        ├── FooModule.kt
        └── FooView.kt
```

The platform code lives under top-level `ios/` and `android/` dirs
(the way Expo Modules / most native libraries group it), each openable
directly in Xcode / Android Studio. The three build manifests
(`Cargo.toml`, `Package.swift`, `build.gradle.kts`) stay at the root;
`Package.swift` *must* be there because SwiftPM keys a local package's
identity off its directory name.

## What goes where — quick reference

| Symbol | Crate / module | Used by |
|---|---|---|
| `#[whisker::platform_component("Tag")]` | `whisker` (proc macro) | View-bearing modules |
| `#[whisker::platform_module(name = "X")]` | `whisker` (proc macro) | Function-only modules |
| `#[whisker::element_methods(Props)]` | `whisker` (proc macro) | Typed `ElementRef<T>::method()` dispatch |
| `WhiskerValue`, `WhiskerModuleError` | `whisker::platform_module` | Both flavors |
| `Signal<T>`, `ElementRef<T>`, `element_ref()` | `whisker` (top-level) | View-bearing modules' shim |
| `@WhiskerModule` (marker) | `WhiskerModuleMacros` SPM target / `rs.whisker.annotations.WhiskerModule` | iOS / Android DSL module classes |
| `Module`, `ModuleDefinition`, `Name`/`View`/`Prop`/`Function`/`Events`/`Constants` | `WhiskerModule` SPM target / `rs.whisker.runtime` Kotlin package | DSL `definition()` body |
| `WhiskerUI<View>` / `WhiskerContext` / `WhiskerValue` | `WhiskerModule` SPM target / `rs.whisker.runtime` Kotlin package | iOS / Android view classes |

## Future direction

The Expo-style `ModuleDefinition` DSL (Phase L, #58) is the sole
authoring surface for both view-bearing and function-only modules —
they share the same `definition() -> ModuleDefinition` entry point and
`View(...) { Prop(...) … }` is a feature of the DSL. The older
`@WhiskerComponent` / `@WhiskerProp` / `@WhiskerUIMethod` annotation
set was removed in Phase M (#212).

See the Whisker Module epic (#55) for the broader roadmap.
