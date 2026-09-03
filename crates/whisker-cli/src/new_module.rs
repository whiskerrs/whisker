//! `whisker new-module <name>` — scaffold a Whisker module crate.
//!
//! Creates a directory matching the supplied crate name with a
//! complete module skeleton: `Cargo.toml` (carrying the
//! `[package.metadata.whisker.module.platforms]` support map), `Package.swift`,
//! `build.gradle.kts`, `src/lib.rs`, and the platform sources under
//! `ios/`, `android/`, `desktop/`, and `web/` (Expo-style layout). The skeleton compiles
//! standalone — the consumer just runs `cargo build` and adds the
//! crate as a dep to their Whisker app.
//!
//! Naming convention: input is the cargo crate name (kebab-case,
//! `whisker-foo`). The PascalCase tag (`Foo`), the module class
//! (`FooModule`), and (for view-bearing modules) the view class
//! (`FooView`) are derived. A view-bearing module registers its element
//! under `<crate-name>:<tag>` (`whisker-foo:Foo`).
//!
//! Modules are authored with the ModuleDefinition DSL: a class
//! subclasses `Module`, applies `@WhiskerModule`, and overrides
//! `definition()`. The annotation is the explicit registration trigger.
//! Per-platform codegen (SwiftPM build plugin / KSP) finds each annotated
//! declaration and emits the Host registration.
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
    /// `#[whisker::module_element]` shim + a DSL module with a
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
    let element_name = format!("{}:{tag}", args.name);

    let v = Vars {
        crate_name: &args.name,
        tag: &tag,
        spm: &spm,
        ns: &ns,
        ident: &ident,
        module_class: &module_class,
        view_class: &view_class,
        element_name: &element_name,
    };

    // Expo-style layout — platform code under `ios/` and `android/`,
    // each openable directly in Xcode / Android Studio.
    let ios_src = format!("ios/Sources/{spm}");
    let android_src = format!("android/src/main/kotlin/rs/whisker/modules/{ns}");
    std::fs::create_dir_all(target_dir.join(&ios_src))
        .with_context(|| format!("create {}/{ios_src}", target_dir.display()))?;
    std::fs::create_dir_all(target_dir.join(&android_src))
        .with_context(|| format!("create {}/{android_src}", target_dir.display()))?;

    write(&target_dir, "Cargo.toml", &cargo_toml(&v, &args.shape))?;
    write(&target_dir, "README.md", &readme(&v))?;
    write(&target_dir, "Package.swift", &package_swift(&v))?;
    write(&target_dir, "build.gradle.kts", &build_gradle(&v))?;

    match args.shape {
        ModuleShape::ViewBearing => {
            write(&target_dir, "src/lib.rs", &lib_rs_view(&v))?;
            write(&target_dir, "desktop/Cargo.toml", &desktop_cargo_toml(&v))?;
            write(&target_dir, "desktop/src/lib.rs", &desktop_lib_rs(&v))?;
            write(&target_dir, "web/Cargo.toml", &web_cargo_toml(&v))?;
            write(&target_dir, "web/src/lib.rs", &web_lib_rs(&v))?;
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
         2. Implement the platform-side logic in ios/, android/, desktop/, and web/.\n  \
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
    /// View-bearing Host UI subclass, e.g. `FooView`.
    view_class: &'a str,
    /// Stable package-qualified element name shared by all Hosts.
    element_name: &'a str,
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

fn cargo_toml(v: &Vars, shape: &ModuleShape) -> String {
    let rust_hosts = if matches!(shape, ModuleShape::ViewBearing) {
        r#"
desktop = { manifest = "desktop/Cargo.toml" }
web = { manifest = "web/Cargo.toml" }
"#
        .to_string()
    } else {
        String::new()
    };
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
description = "Whisker module — short tagline shown on crates.io."

include = [
    "/Cargo.toml",
    "/Package.swift",
    "/build.gradle.kts",
    "/src/lib.rs",
    "/android/**/*.kt",
    "/ios/**/*.swift",
    "/README.md",
]

[lib]
crate-type = ["rlib"]

# Module support is explicit. An omitted platform is unsupported;
# `kind = "common"` means the parent Rust crate needs no Host adapter.
[package.metadata.whisker.module.platforms]
android = {{ manifest = "build.gradle.kts" }}
ios = {{ manifest = "Package.swift" }}
{rust_hosts}

[dependencies]
# The umbrella `whisker` crate. The proc macros' emit paths
# (::whisker::ElementRef, ::whisker::platform_module::WhiskerValue, ...)
# resolve under the `whisker` name — the same dep app crates use.
whisker = "{dep_version}"
"#,
        name = v.crate_name,
        dep_version = whisker_dep_version(),
        rust_hosts = rust_hosts,
    )
}

fn readme(v: &Vars) -> String {
    format!(
        r#"# {name}

A Whisker module — registers the `{element_name}` Host element and exposes
`{tag}` for use in Whisker app `render!` trees.

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
        element_name = v.element_name,
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
// first-party module uses). `WhiskerRuntime` supplies the `WhiskerView` /
// driver symbols. The
// `WhiskerModuleCodegenPlugin` build-tool plugin walks `Module`
// subclasses at build time and emits the Host registration.

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
/// match [`whisker_build::ios::WHISKER_IOS_SPM_VERSION`] and every
/// first-party module manifest.
const WHISKER_IOS_SPM_TAG: &str = whisker_build::ios::WHISKER_IOS_SPM_VERSION;

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
    // runtime classpath. The KSP processor discovers explicit
    // `@WhiskerModule` declarations. The `{android_tag}`
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
//! Registers an element under `{element_name}` and exposes the
//! `{tag}` symbol for use inside `render!`. Platform-side classes
//! live under `ios/`, `android/`, `desktop/`, and `web/`.

use whisker::Style;

/// View-bearing element shared by every Host implementation.
#[whisker::module_element(name = "{element_name}", measurement = None)]
pub fn {ident}(style: Style) {{}}

/// Element schemas exported by this package for surface bootstrap.
#[doc(hidden)]
pub fn __whisker_element_module_definition() -> whisker::ElementModuleDefinition {{
    whisker::ElementModuleDefinition::new(
        env!("CARGO_PKG_NAME"),
        [{ident}_schema::element_provider()],
    )
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        ident = v.ident,
        element_name = v.element_name,
    )
}

fn desktop_cargo_toml(v: &Vars) -> String {
    format!(
        r#"[package]
name = "{name}-desktop"
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
description = "Desktop Host implementation for {name}."

[dependencies]
whisker-desktop = "{dep_version}"
"#,
        name = v.crate_name,
        dep_version = whisker_dep_version(),
    )
}

fn desktop_lib_rs(v: &Vars) -> String {
    format!(
        r#"//! Desktop Host implementation for `{name}`.

use whisker_desktop::{{DesktopViewDefinition, ModuleDefinition, WhiskerModule}};

struct {tag}Module;

#[WhiskerModule]
impl WhiskerModule for {tag}Module {{
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {{
        ModuleDefinition::new()
            .name("{element_name}")
            .view(DesktopViewDefinition::new("{element_name}", || ()))
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        element_name = v.element_name,
    )
}

fn web_cargo_toml(v: &Vars) -> String {
    format!(
        r#"[package]
name = "{name}-web"
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
description = "Web Host implementation for {name}."

[dependencies]
whisker-web = "{dep_version}"
"#,
        name = v.crate_name,
        dep_version = whisker_dep_version(),
    )
}

fn web_lib_rs(v: &Vars) -> String {
    format!(
        r#"//! Web Host implementation for `{name}`.

use whisker_web::{{ModuleDefinition, WebViewDefinition, WhiskerModule}};

struct {tag}Module;

#[WhiskerModule]
impl WhiskerModule for {tag}Module {{
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {{
        ModuleDefinition::new()
            .name("{element_name}")
            .view(WebViewDefinition::new(
                "{element_name}",
                |document, _| document.create_element("div"),
                Clone::clone,
            ))
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        element_name = v.element_name,
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
        r#"// `{module_class}` — iOS side of the `{element_name}` Whisker module.
//
// Declares the Host element via the ModuleDefinition DSL. `@WhiskerModule`
// is the explicit registration signal. The `{view_class}` lives
// in `{view_class}.swift`.

import WhiskerModule    // Module, ModuleDefinition, DSL

@WhiskerModule
public final class {module_class}: Module {{
    public override func definition() -> ModuleDefinition {{
        ModuleDefinition {{
            Name("{tag}")
            View("{element_name}", {view_class}.self) {{
                // Declare Prop / Command entries here, e.g.:
                //   Prop("title") {{ (view: {view_class}, value: WhiskerValue) in
                //       view.setTitle(value.asString ?? "")
                //   }}
                //   Command("focus") {{ (view: {view_class}, _: WhiskerValue) in
                //       view.focus()
                //   }}
            }}
        }}
    }}
}}
"#,
        element_name = v.element_name,
        tag = v.tag,
        module_class = v.module_class,
        view_class = v.view_class,
    )
}

fn swift_view(v: &Vars) -> String {
    format!(
        r#"// `{view_class}` — the Host UI subclass backing `{name}:{tag}`.
// Instantiated through the View declaration in `{module_class}.definition()`.
// `@objc({view_class})` pins the Obj-C class name so the
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
        r#"// `{module_class}` -- Android side of the `{element_name}` Whisker module.
//
// `@WhiskerModule` is the explicit registration signal. The `{view_class}` lives in
// `{view_class}.kt`.
//
// Note the explicit `import rs.whisker.runtime.Module` — without it
// the unqualified `Module` resolves to `java.lang.Module` (a Kotlin
// JVM default import).

package rs.whisker.modules.{ns}

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerValue

@WhiskerModule
class {module_class} : Module() {{
    override fun definition() = ModuleDefinition {{
        Name("{tag}")
        View("{element_name}", {view_class}::class.java) {{
            // Declare Prop / Command entries here, e.g.:
            //   Prop("title") {{ view: {view_class}, value: WhiskerValue ->
            //       view.setTitle(value.asString() ?: "")
            //   }}
            //   Command("focus") {{ view: {view_class}, _: WhiskerValue ->
            //       view.focus()
            //   }}
        }}
    }}
}}
"#,
        element_name = v.element_name,
        tag = v.tag,
        ns = v.ns,
        module_class = v.module_class,
        view_class = v.view_class,
    )
}

fn kotlin_view(v: &Vars) -> String {
    format!(
        r#"// `{view_class}` -- the Host UI subclass backing `{name}:{tag}`.
// Instantiated through the View declaration in `{module_class}.definition()`.
// The single-arg `(WhiskerContext)` constructor matches
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
// just module-level `Function`s. `@WhiskerModule` is the explicit
// registration signal — the SwiftPM codegen plugin emits a dispatch
// shim registered under the `Name("...")`, so
// `Whisker{tag}::placeholder()` on the Rust side routes here.

import WhiskerModule    // Module, ModuleDefinition, DSL

@WhiskerModule
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
// `@WhiskerModule` is the explicit registration signal. See the note in
// the view-bearing template re: the explicit `Module` import.

package rs.whisker.modules.{ns}

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule

@WhiskerModule
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
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tempdir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "whisker-new-module-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

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

    #[test]
    fn view_scaffold_separates_common_desktop_and_web_crates() {
        let root = tempdir();
        run(NewModuleArgs {
            name: "whisker-switch".into(),
            path: Some(root.clone()),
            shape: ModuleShape::ViewBearing,
        })
        .unwrap();
        let module = root.join("whisker-switch");
        for path in [
            "Cargo.toml",
            "src/lib.rs",
            "desktop/Cargo.toml",
            "desktop/src/lib.rs",
            "web/Cargo.toml",
            "web/src/lib.rs",
        ] {
            assert!(module.join(path).is_file(), "missing {path}");
        }
        let common = std::fs::read_to_string(module.join("src/lib.rs")).unwrap();
        assert!(common.contains("whisker-switch:Switch"));
        assert!(!common.contains("whisker_desktop"));
        assert!(!common.contains("whisker_web"));
        let manifest = std::fs::read_to_string(module.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("[package.metadata.whisker.module.platforms]"));
        assert!(manifest.contains("web = { manifest = \"web/Cargo.toml\" }"));
        assert!(manifest.contains("desktop = { manifest = \"desktop/Cargo.toml\" }"));
        let desktop = std::fs::read_to_string(module.join("desktop/Cargo.toml")).unwrap();
        assert!(desktop.contains("name = \"whisker-switch-desktop\""));
        let desktop_source = std::fs::read_to_string(module.join("desktop/src/lib.rs")).unwrap();
        assert!(desktop_source.contains("whisker-switch:Switch"));
        assert!(desktop_source.contains(".name(\"whisker-switch:Switch\")"));
        let web = std::fs::read_to_string(module.join("web/Cargo.toml")).unwrap();
        assert!(web.contains("name = \"whisker-switch-web\""));
        let web_source = std::fs::read_to_string(module.join("web/src/lib.rs")).unwrap();
        assert!(web_source.contains("whisker-switch:Switch"));
        assert!(web_source.contains(".name(\"whisker-switch:Switch\")"));
        let android = std::fs::read_to_string(
            module
                .join("android/src/main/kotlin/rs/whisker/modules/whisker_switch/SwitchModule.kt"),
        )
        .unwrap();
        assert!(android.contains("Name(\"Switch\")"));
        assert!(android.contains("View(\"whisker-switch:Switch\""));
        let ios =
            std::fs::read_to_string(module.join("ios/Sources/WhiskerSwitch/SwitchModule.swift"))
                .unwrap();
        assert!(ios.contains("Name(\"Switch\")"));
        assert!(ios.contains("View(\"whisker-switch:Switch\""));
        std::fs::remove_dir_all(root).ok();
    }
}
