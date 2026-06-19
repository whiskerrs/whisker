//! `whisker new-module <name>` — scaffold a Whisker module crate.
//!
//! Creates a directory matching the supplied crate name with a
//! complete module skeleton: `Cargo.toml` (carrying the
//! `[package.metadata.whisker]` discovery marker), `Package.swift`,
//! `build.gradle.kts`, `src/lib.rs`, and the platform sources under
//! `ios/` and `android/` (Expo-style layout). The skeleton compiles
//! standalone — the consumer just runs `cargo build` and adds the
//! crate as a dep to their Whisker app.
//!
//! Naming convention: input is the cargo crate name (kebab-case,
//! `whisker-foo`). The PascalCase tag (`Foo`), the module class
//! (`FooModule`), and (for view-bearing modules) the view class
//! (`FooView`) are derived. Lynx registers a view-bearing module's
//! element under `<crate-name>:<tag>` (`whisker-foo:Foo`).
//!
//! Modules are authored with the ModuleDefinition DSL: a class
//! subclasses `Module` and overrides `definition()`. Subclassing
//! the base IS the registration trigger — the per-platform
//! codegen (SwiftPM build plugin / KSP) finds every concrete
//! `Module` subclass and emits the Lynx registration. Phase M
//! (Issue #59) dropped the previously-companion `@WhiskerModule`
//! marker annotation.
//!
//! This is a minimal scaffolder — it copies a small set of inline
//! templates and substitutes a handful of variables. For a richer
//! template story (multiple module types, custom dirs, …) the
//! `whisker new-module` subcommand can grow later without breaking
//! the contract documented at <https://whisker.rs/docs/authoring-a-module>.

use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use std::path::{Path, PathBuf};

/// `whisker new-module` CLI arguments.
#[derive(Args, Debug)]
pub struct NewModuleArgs {
    /// The cargo crate name. Convention: kebab-case, prefixed with
    /// `whisker-` (e.g. `whisker-camera`, `whisker-blur-view`). Must
    /// be a valid cargo package name — letters / digits / `-` / `_`,
    /// must start with a letter.
    pub name: String,

    /// Optional parent directory. Defaults to the current working
    /// directory. The new crate lands at `<parent>/<name>/`.
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// Module shape. `view-bearing` (the default) generates a
    /// `#[whisker::module_component]` shim + a DSL module with a
    /// `View(...)` block and a `WhiskerUI<View>` subclass.
    /// `function-only` generates a `#[whisker::platform_module]`
    /// proxy + a DSL module with module-level `Function`s and no
    /// `View(...)` — for modules that only expose function calls
    /// (e.g. `whisker-local-store`-style key-value stores).
    #[arg(long, value_enum, default_value_t = ModuleShape::ViewBearing)]
    pub shape: ModuleShape,
}

#[derive(clap::ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum ModuleShape {
    /// View-bearing — renders a native view + supports prop / method
    /// dispatch via `ElementRef<T>`.
    #[value(name = "view-bearing")]
    ViewBearing,
    /// Function-only — Rust calls platform-side functions; no UI.
    #[value(name = "function-only")]
    FunctionOnly,
}

pub fn run(args: NewModuleArgs) -> Result<()> {
    validate_crate_name(&args.name)?;
    let parent = args.path.unwrap_or_else(|| PathBuf::from("."));
    let target_dir = parent.join(&args.name);
    if target_dir.exists() {
        bail!(
            "{}: directory already exists. Pick a different name or remove it.",
            target_dir.display(),
        );
    }

    let tag = pascal_case_tag(&args.name);
    let spm = crate_to_spm_target(&args.name);
    let ns = args.name.replace('-', "_");
    let ident = args
        .name
        .replace('-', "_")
        .trim_start_matches("whisker_")
        .to_string();
    let module_class = format!("{tag}Module");
    let view_class = format!("{tag}View");

    let v = Vars {
        crate_name: &args.name,
        tag: &tag,
        spm: &spm,
        ns: &ns,
        ident: &ident,
        module_class: &module_class,
        view_class: &view_class,
    };

    // Expo-style layout — platform code under `ios/` and `android/`,
    // each openable directly in Xcode / Android Studio.
    let ios_src = format!("ios/Sources/{spm}");
    let android_src = format!("android/src/main/kotlin/rs/whisker/modules/{ns}");
    std::fs::create_dir_all(target_dir.join(&ios_src))
        .with_context(|| format!("create {}/{ios_src}", target_dir.display()))?;
    std::fs::create_dir_all(target_dir.join(&android_src))
        .with_context(|| format!("create {}/{android_src}", target_dir.display()))?;

    write(&target_dir, "Cargo.toml", &cargo_toml(&v))?;
    write(&target_dir, "README.md", &readme(&v))?;
    write(&target_dir, "Package.swift", &package_swift(&v))?;
    write(&target_dir, "build.gradle.kts", &build_gradle(&v))?;

    match args.shape {
        ModuleShape::ViewBearing => {
            write(&target_dir, "src/lib.rs", &lib_rs_view(&v))?;
            write(
                &target_dir,
                &format!("{ios_src}/{module_class}.swift"),
                &swift_view_module(&v),
            )?;
            write(
                &target_dir,
                &format!("{ios_src}/{view_class}.swift"),
                &swift_view(&v),
            )?;
            write(
                &target_dir,
                &format!("{android_src}/{module_class}.kt"),
                &kotlin_view_module(&v),
            )?;
            write(
                &target_dir,
                &format!("{android_src}/{view_class}.kt"),
                &kotlin_view(&v),
            )?;
        }
        ModuleShape::FunctionOnly => {
            write(&target_dir, "src/lib.rs", &lib_rs_module(&v))?;
            write(
                &target_dir,
                &format!("{ios_src}/{module_class}.swift"),
                &swift_function_module(&v),
            )?;
            write(
                &target_dir,
                &format!("{android_src}/{module_class}.kt"),
                &kotlin_function_module(&v),
            )?;
        }
    }

    eprintln!(
        "Created Whisker module skeleton at {}\n\
         \n\
         Next steps:\n  \
         1. cd {}\n  \
         2. Implement the platform-side logic in ios/ and android/.\n  \
         3. From your Whisker app: `cargo add --path {}` (or publish to crates.io).\n  \
         4. See https://whisker.rs/docs/authoring-a-module for the full reference.",
        target_dir.display(),
        target_dir.display(),
        target_dir.display(),
    );
    Ok(())
}

// ============================================================================
// Template variables + rendering
// ============================================================================

struct Vars<'a> {
    /// Cargo crate name, e.g. `whisker-foo`.
    crate_name: &'a str,
    /// PascalCase local tag, e.g. `Foo`.
    tag: &'a str,
    /// SwiftPM target name == PascalCased full crate name, e.g.
    /// `WhiskerFoo`.
    spm: &'a str,
    /// Android package leaf == crate name with `-` → `_`, e.g.
    /// `whisker_foo`.
    ns: &'a str,
    /// Rust fn identifier == crate name minus the `whisker_` prefix,
    /// e.g. `foo`.
    ident: &'a str,
    /// DSL module class, e.g. `FooModule`.
    module_class: &'a str,
    /// View-bearing Lynx UI subclass, e.g. `FooView`.
    view_class: &'a str,
}

fn write(root: &Path, rel: &str, content: &str) -> Result<()> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// The `MAJOR.MINOR` version requirement the scaffolded crate should
/// pin `whisker` to. Derived from whisker-cli's own (workspace-shared)
/// version so a freshly-scaffolded module unifies with the toolchain
/// that generated it — e.g. cli `0.2.5` → `"0.2"`. An app on `0.2.x`
/// can't unify a module that asks for `^0.1`, so a hardcoded `"0.1"`
/// would break every scaffold after the 0.2 bump.
fn whisker_dep_version() -> String {
    let v = env!("CARGO_PKG_VERSION");
    let mut parts = v.split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("0");
    format!("{major}.{minor}")
}

fn cargo_toml(v: &Vars) -> String {
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
description = "Whisker module — short tagline shown on crates.io."

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

# Module-system opt-in marker — the bare table identifies this cargo
# crate as a Whisker module, so `whisker-build` wires its `android/`
# Gradle subproject + `ios/` SwiftPM package into the host build.
[package.metadata.whisker]

[dependencies]
# The umbrella `whisker` crate. The proc macros' emit paths
# (::whisker::ElementRef, ::whisker::platform_module::WhiskerValue, ...)
# resolve under the `whisker` name — the same dep app crates use.
whisker = "{dep_version}"
"#,
        name = v.crate_name,
        dep_version = whisker_dep_version(),
    )
}

fn readme(v: &Vars) -> String {
    format!(
        r#"# {name}

A Whisker module — registers the `{name}:{tag}` element under Lynx
and exposes `{tag}` for use in Whisker app `render!` trees.

## Usage

```toml
[dependencies]
{name} = "{dep_version}"
```

```rust
use whisker::prelude::*;
use {ident}::*;

#[whisker::main]
fn app() -> Element {{
    render! {{
        {tag}()
    }}
}}
```

See [the Whisker Module Author Guide](https://whisker.rs/docs/authoring-a-module)
for the full reference.
"#,
        name = v.crate_name,
        tag = v.tag,
        ident = v.ident,
        dep_version = whisker_dep_version(),
    )
}

fn package_swift(v: &Vars) -> String {
    format!(
        r#"// swift-tools-version:5.9
//
// SwiftPM manifest for the `{name}` module's iOS half. The consumer
// app's `whisker-build`-generated aggregator depends on the library
// product below via `.product(name: "{spm}", package: "{name}")`.
//
// Package.swift lives at the package root (SwiftPM requires it
// there); the Swift sources live under the `ios/` subdir alongside
// `android/` + `src/`.
//
// The module resolves Whisker's iOS runtime + macros via the published
// `whisker` SwiftPM package (the same remote-git dependency every
// first-party module uses) — `WhiskerModule` re-exports Lynx, and
// `WhiskerRuntime` pulls in the `WhiskerView` / driver symbols. The
// `WhiskerModuleCodegenPlugin` build-tool plugin walks `Module`
// subclasses at build time and emits the Lynx registration.

import PackageDescription

let package = Package(
    name: "{name}",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "{spm}", targets: ["{spm}"]),
    ],
    dependencies: [
        .package(url: "https://github.com/whiskerrs/whisker.git", exact: "{ios_tag}"),
    ],
    targets: [
        .target(
            name: "{spm}",
            dependencies: [
                .product(name: "WhiskerModule", package: "whisker"),
                .product(name: "WhiskerRuntime", package: "whisker"),
            ],
            path: "ios/Sources/{spm}",
            plugins: [
                .plugin(name: "WhiskerModuleCodegenPlugin", package: "whisker"),
            ]
        ),
    ]
)
"#,
        name = v.crate_name,
        spm = v.spm,
        ios_tag = WHISKER_IOS_SPM_TAG,
    )
}

/// The exact iOS SwiftPM tag the scaffolded `Package.swift` pins for
/// the `whisker` git dependency. This is the iOS SPM release tag, which
/// is versioned independently from the cargo crate version — it must
/// match whatever the first-party modules pin (see
/// `packages/whisker-webview/Package.swift`, currently `exact: "0.1.0"`).
const WHISKER_IOS_SPM_TAG: &str = "0.1.0";

fn build_gradle(v: &Vars) -> String {
    format!(
        r#"// Gradle subproject for the `{name}` Whisker module on Android.
// Wired into the consumer app's settings.gradle.kts by whisker-build.
// build.gradle.kts sits at the package root, alongside Package.swift
// + Cargo.toml; the Kotlin source set points at the `android/` subdir.

plugins {{
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}}

android {{
    namespace = "rs.whisker.modules.{ns}"
    compileSdk = 34

    defaultConfig {{
        minSdk = 21
    }}

    compileOptions {{
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }}
    kotlinOptions {{
        jvmTarget = "17"
    }}

    sourceSets {{
        getByName("main") {{
            kotlin.srcDirs("android/src/main/kotlin")
        }}
    }}
}}

ksp {{
    arg("whisker.moduleName", "{spm}")
    arg("whisker.crateName", "{name}")
}}

dependencies {{
    // Published Whisker runtime + KSP processor — the same Maven
    // coordinates every first-party module uses. ksp(rs.whisker:ksp)
    // stays separate because it's a build-time processor, not on the
    // runtime classpath. The KSP processor discovers Module subclasses
    // by inheritance (no marker annotation needed). The `{android_tag}`
    // tag is the Android (Maven) release, versioned independently from
    // the cargo crate.
    implementation("rs.whisker:whisker-module-android:{android_tag}")
    ksp("rs.whisker:ksp:{android_tag}")
}}
"#,
        name = v.crate_name,
        ns = v.ns,
        spm = v.spm,
        android_tag = WHISKER_ANDROID_MAVEN_TAG,
    )
}

/// The Maven release tag the scaffolded `build.gradle.kts` pins for the
/// Whisker Android runtime + KSP processor. Like the iOS SPM tag, the
/// Android Maven release is versioned independently from the cargo
/// crate — must match first-party (see
/// `packages/whisker-webview/build.gradle.kts`, currently `0.1.0`).
const WHISKER_ANDROID_MAVEN_TAG: &str = "0.1.0";

fn lib_rs_view(v: &Vars) -> String {
    format!(
        r#"//! `{name}` — Whisker view-bearing module.
//!
//! Registers a Lynx element under `{name}:{tag}` and exposes the
//! `{tag}` symbol for use inside `render!`. Platform-side classes
//! live under `ios/` and `android/`.

use whisker::Signal;

/// View-bearing element. The Lynx tag the bridge registers against
/// is `{name}:{tag}` — the crate name namespace is auto-prepended by
/// `#[whisker::module_component]`. Imperative methods on a mounted
/// instance go through an `ElementRef` (the `ref:` prop) — wrap one
/// in a typed `{tag}Handle` struct for the public API.
#[whisker::module_component("{tag}")]
pub fn {ident}(style: Signal<String>) {{}}
"#,
        name = v.crate_name,
        tag = v.tag,
        ident = v.ident,
    )
}

fn lib_rs_module(v: &Vars) -> String {
    format!(
        r#"//! `{name}` — Whisker function-only platform module.
//!
//! Exposes typed Rust -> Kotlin/Swift function calls without
//! rendering UI. Platform-side classes live under `ios/` and
//! `android/`.

use whisker::platform_module::{{WhiskerModuleError, WhiskerValue}};

/// Typed Rust API for the `Whisker{tag}` platform module.
///
/// Hand-written wrapper over the framework primitive: each method
/// builds the raw `Vec<WhiskerValue>` arg list, dispatches via
/// `whisker::module!("Whisker{tag}").invoke(method, args)`, and lifts
/// the returned `WhiskerValue` into a typed `Result`. The `module!`
/// name MUST match the `Name("...")` in the platform-side
/// `definition()`; `module!` auto-prepends this crate's name so two
/// crates can ship same-named modules without colliding.
pub struct Whisker{tag};
impl Whisker{tag} {{
    pub fn placeholder() -> Result<(), WhiskerModuleError> {{
        // Build args, dispatch, lift the WhiskerValue into a typed result.
        match whisker::module!("Whisker{tag}").invoke("_placeholder", vec![]) {{
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            _ => Ok(()),
        }}
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
    )
}

fn swift_view_module(v: &Vars) -> String {
    format!(
        r#"// `{module_class}` — iOS side of the `{name}:{tag}` Whisker module.
//
// Declares the Lynx element `{name}:{tag}` via the ModuleDefinition
// DSL. Subclassing `Module` is the registration signal — the SwiftPM
// codegen plugin walks every `Module` subclass and emits the Lynx
// behavior registration. The `{view_class}` Lynx UI subclass lives
// in `{view_class}.swift`.

import WhiskerModule    // Module, ModuleDefinition, DSL

public final class {module_class}: Module {{
    public override func definition() -> ModuleDefinition {{
        ModuleDefinition {{
            Name("{tag}")
            View({view_class}.self) {{
                // Declare Prop / Function entries here, e.g.:
                //   Prop("title") {{ (view: {view_class}, value: String) in
                //       view.setTitle(value)
                //   }}
                //   Function("focus") {{ (view: {view_class}) in view.focus() }}
            }}
        }}
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        module_class = v.module_class,
        view_class = v.view_class,
    )
}

fn swift_view(v: &Vars) -> String {
    format!(
        r#"// `{view_class}` — the Lynx UI subclass backing `{name}:{tag}`.
// Instantiated by Lynx via the behavior `{module_class}.definition()`
// registers. `@objc({view_class})` pins the Obj-C class name so the
// codegen plugin's `NSClassFromString` lookup resolves it.

import UIKit
import WhiskerModule

@objc({view_class})
public final class {view_class}: WhiskerUI<UIView> {{
    @objc public override func createView() -> UIView {{
        let v = UIView()
        v.backgroundColor = .systemPink
        return v
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        module_class = v.module_class,
        view_class = v.view_class,
    )
}

fn kotlin_view_module(v: &Vars) -> String {
    format!(
        r#"// `{module_class}` -- Android side of the `{name}:{tag}` Whisker module.
//
// Subclassing `Module` is the registration signal — the KSP processor
// walks every concrete subclass and emits the Lynx behavior
// registration. The `{view_class}` Lynx UI subclass lives in
// `{view_class}.kt`.
//
// Note the explicit `import rs.whisker.runtime.Module` — without it
// the unqualified `Module` resolves to `java.lang.Module` (a Kotlin
// JVM default import).

package rs.whisker.modules.{ns}

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition

class {module_class} : Module() {{
    override fun definition() = ModuleDefinition {{
        Name("{tag}")
        View({view_class}::class.java) {{
            // Declare Prop / Function entries here, e.g.:
            //   Prop("title") {{ view: {view_class}, value: String ->
            //       view.setTitle(value)
            //   }}
            //   Function("focus") {{ view: {view_class} -> view.focus() }}
        }}
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        ns = v.ns,
        module_class = v.module_class,
        view_class = v.view_class,
    )
}

fn kotlin_view(v: &Vars) -> String {
    format!(
        r#"// `{view_class}` -- the Lynx UI subclass backing `{name}:{tag}`.
// Instantiated by the Lynx behavior `{module_class}.definition()`
// registers. The single-arg `(WhiskerContext)` constructor matches
// the convention the KSP registration code expects.

package rs.whisker.modules.{ns}

import android.content.Context
import android.graphics.Color
import android.view.View
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI

open class {view_class}(context: WhiskerContext) : WhiskerUI<View>(context) {{
    override fun createView(context: Context): View {{
        val v = View(context)
        v.setBackgroundColor(Color.argb(0xff, 0xff, 0x80, 0xa0))
        return v
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        ns = v.ns,
        module_class = v.module_class,
        view_class = v.view_class,
    )
}

fn swift_function_module(v: &Vars) -> String {
    format!(
        r#"// `{module_class}` — iOS side of the `{name}` Whisker function-only module.
//
// A view-less DSL module: `definition()` has no `View(...)` block,
// just module-level `Function`s. Subclassing `Module` is the
// registration signal — the SwiftPM codegen plugin emits a dispatch
// shim registered under the `Name("...")`, so
// `Whisker{tag}::placeholder()` on the Rust side routes here.

import WhiskerModule    // Module, ModuleDefinition, DSL

public final class {module_class}: Module {{
    public override func definition() -> ModuleDefinition {{
        ModuleDefinition {{
            // The Name MUST match the Rust sys trait's
            // `#[whisker::platform_module(name = "...")]`.
            Name("Whisker{tag}")
            Function("_placeholder") {{
                // TODO: implement the function the Rust sys trait declares.
            }}
        }}
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        module_class = v.module_class,
    )
}

fn kotlin_function_module(v: &Vars) -> String {
    format!(
        r#"// `{module_class}` -- Android side of the `{name}` Whisker function-only module.
//
// A view-less DSL module: module-level `Function`s, no `View(...)`.
// Subclassing `Module` is the registration signal. See the note in
// the view-bearing template re: the explicit `Module` import.

package rs.whisker.modules.{ns}

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition

class {module_class} : Module() {{
    override fun definition() = ModuleDefinition {{
        // The Name MUST match the Rust sys trait's
        // `#[whisker::platform_module(name = "...")]`.
        Name("Whisker{tag}")
        Function("_placeholder") {{
            // TODO: implement the function the Rust sys trait declares.
        }}
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        ns = v.ns,
        module_class = v.module_class,
    )
}

// ============================================================================
// Name helpers
// ============================================================================

/// Validate a cargo crate name. Rejects empty / non-letter-prefixed /
/// non-`[a-z0-9_-]+` inputs with an actionable message.
fn validate_crate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("crate name must not be empty");
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphabetic() {
        bail!(
            "crate name must start with a letter, got {first:?}. Try \
             `whisker-{name}` instead."
        );
    }
    for ch in name.chars() {
        if !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
            return Err(anyhow!(
                "crate name {name:?} contains invalid character {ch:?}. \
                 Use only ASCII letters / digits / `-` / `_`."
            ));
        }
    }
    Ok(())
}

/// Derive the PascalCase tag from the crate name.
///
/// - `whisker-foo` -> `Foo`
/// - `whisker-blur-view` -> `BlurView`
/// - `foo-bar` -> `FooBar` (no `whisker-` prefix → tag is the whole name)
fn pascal_case_tag(crate_name: &str) -> String {
    let stripped = crate_name.strip_prefix("whisker-").unwrap_or(crate_name);
    let mut out = String::new();
    let mut upper = true;
    for ch in stripped.chars() {
        if ch == '-' || ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Same convention as `whisker_build::ios::crate_to_spm_target`:
/// `whisker-foo-bar` -> `WhiskerFooBar`.
fn crate_to_spm_target(crate_name: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in crate_name.chars() {
        if ch == '-' || ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_strips_whisker_prefix() {
        assert_eq!(pascal_case_tag("whisker-foo"), "Foo");
        assert_eq!(pascal_case_tag("whisker-blur-view"), "BlurView");
    }

    #[test]
    fn pascal_keeps_full_name_when_no_whisker_prefix() {
        assert_eq!(pascal_case_tag("foo-bar"), "FooBar");
    }

    #[test]
    fn spm_target_pascals_full_crate_name() {
        assert_eq!(crate_to_spm_target("whisker-foo"), "WhiskerFoo");
        assert_eq!(crate_to_spm_target("whisker-blur-view"), "WhiskerBlurView");
    }

    #[test]
    fn validate_rejects_invalid() {
        assert!(validate_crate_name("").is_err());
        assert!(validate_crate_name("1foo").is_err());
        assert!(validate_crate_name("whisker foo").is_err());
        assert!(validate_crate_name("whisker-foo").is_ok());
        assert!(validate_crate_name("whisker_foo").is_ok());
    }
}
