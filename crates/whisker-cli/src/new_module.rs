//! `whisker new-module <name>` — scaffold a Whisker module crate.
//!
//! Creates a directory matching the supplied crate name with a
//! complete module skeleton: `Cargo.toml`, `whisker.module.toml`,
//! `Package.swift`, `build.gradle.kts`, `src/lib.rs`, an iOS Swift
//! source, and an Android Kotlin source. The skeleton compiles
//! standalone — the consumer just runs `cargo build` and adds the
//! crate as a dep to their Whisker app.
//!
//! Naming convention: input is the cargo crate name (kebab-case,
//! `whisker-foo`). The PascalCase tag (`Foo`) and the platform-side
//! class name (`WhiskerFooComponent`) are derived. Lynx registers
//! the module under `<crate-name>:<tag>` (`whisker-foo:Foo`).
//!
//! This is a minimal scaffolder — it copies a small set of inline
//! templates and substitutes a handful of variables. For a richer
//! template story (multiple module types, custom dirs, …) the
//! `cargo whisker new-module` subcommand can grow later without
//! breaking the contract documented in
//! `docs/module-author-guide.md`.

use anyhow::{anyhow, bail, Context, Result};
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
    /// `#[whisker::platform_component]` shim + the Kotlin / Swift
    /// `WhiskerUI<View>` subclasses. `function-only` generates a
    /// `#[whisker::platform_module]` proxy without a `View(...)` —
    /// for modules that only expose function calls (e.g.
    /// `whisker-local-store`-style key-value stores).
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
    let class_name = format!("Whisker{tag}Component");

    let v = Vars {
        crate_name: &args.name,
        tag: &tag,
        class_name: &class_name,
    };

    std::fs::create_dir_all(target_dir.join("src/android"))
        .with_context(|| format!("create {}/src/android", target_dir.display()))?;
    std::fs::create_dir_all(target_dir.join("src/ios"))
        .with_context(|| format!("create {}/src/ios", target_dir.display()))?;

    write(&target_dir, "Cargo.toml", &cargo_toml(&v))?;
    write(&target_dir, "README.md", &readme(&v))?;
    write(&target_dir, "whisker.module.toml", &module_manifest(&v))?;
    write(&target_dir, "Package.swift", &package_swift(&v))?;
    write(&target_dir, "build.gradle.kts", &build_gradle(&v))?;

    match args.shape {
        ModuleShape::ViewBearing => {
            write(&target_dir, "src/lib.rs", &lib_rs_view(&v))?;
            write(
                &target_dir,
                &format!("src/ios/{class_name}.swift"),
                &swift_view(&v),
            )?;
            write(
                &target_dir,
                &format!("src/android/{class_name}.kt"),
                &kotlin_view(&v),
            )?;
        }
        ModuleShape::FunctionOnly => {
            write(&target_dir, "src/lib.rs", &lib_rs_module(&v))?;
            // For function-only, the file holds the `@WhiskerModule`-
            // tagged class. Strip the `Component` suffix from the class
            // name so it reads `WhiskerFoo` rather than
            // `WhiskerFooComponent` for non-View modules.
            let module_class = format!("Whisker{tag}");
            write(
                &target_dir,
                &format!("src/ios/{module_class}Impl.swift"),
                &swift_module(&v, &module_class),
            )?;
            write(
                &target_dir,
                &format!("src/android/{module_class}Impl.kt"),
                &kotlin_module(&v, &module_class),
            )?;
        }
    }

    eprintln!(
        "Created Whisker module skeleton at {}\n\
         \n\
         Next steps:\n  \
         1. cd {}\n  \
         2. Implement the platform-side logic in src/ios/ and src/android/.\n  \
         3. From your Whisker app: `cargo add --path {}` (or publish to crates.io).\n  \
         4. See docs/module-author-guide.md for the full reference.",
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
    crate_name: &'a str,
    tag: &'a str,
    class_name: &'a str,
}

fn write(root: &Path, rel: &str, content: &str) -> Result<()> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn cargo_toml(v: &Vars) -> String {
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
description = "Whisker module — short tagline shown on crates.io."

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
# Rename `whisker-modules-api` -> `whisker` so the proc macros' emit
# paths (::whisker::ElementRef, ::whisker::platform_module::WhiskerValue,
# ...) resolve. Cargo doesn't allow `package = ...` with
# `workspace = true`, so the version is inlined here.
whisker = {{ package = "whisker-modules-api", version = "0.1" }}
"#,
        name = v.crate_name,
    )
}

fn readme(v: &Vars) -> String {
    format!(
        r#"# {name}

A Whisker module — registers the `{name}:{tag}` element under Lynx
and exposes `{class}` for use in Whisker app `render!` trees.

## Usage

```toml
[dependencies]
{name} = "0.1"
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

See [the Whisker Module Author Guide](https://github.com/whiskerrs/whisker/blob/main/docs/module-author-guide.md)
for the full reference.
"#,
        name = v.crate_name,
        tag = v.tag,
        class = v.class_name,
        ident = v.crate_name.replace('-', "_"),
    )
}

fn module_manifest(_v: &Vars) -> String {
    r#"# Whisker module manifest. Tells `whisker-build` which platform
# sources to surface from this crate into the consuming app's
# generated host project. Paths are relative to this file.

[ios]
swift_sources = ["src/ios"]

[android]
kotlin_sources = ["src/android"]
"#
    .to_string()
}

fn package_swift(v: &Vars) -> String {
    format!(
        r#"// swift-tools-version:5.9
//
// SwiftPM manifest for the `{name}` module's iOS half. The consumer
// app's `whisker-build`-generated aggregator depends on the library
// product below via `.product(name: "{spm}", package: "{name}")`.

import PackageDescription

let package = Package(
    name: "{name}",
    platforms: [.iOS(.v13), .macOS(.v13)],
    products: [
        .library(name: "{spm}", targets: ["{spm}"]),
    ],
    dependencies: [
        .package(name: "macros", path: "../../platforms/ios/macros"),
        .package(name: "WhiskerRuntime", path: "../../platforms/ios"),
    ],
    targets: [
        .target(
            name: "{spm}",
            dependencies: [
                .product(name: "WhiskerComponents", package: "macros"),
                .product(name: "WhiskerModuleApi", package: "WhiskerRuntime"),
            ],
            path: "src/ios",
            plugins: [
                .plugin(name: "WhiskerComponentsCodegenPlugin", package: "macros"),
            ]
        ),
    ]
)
"#,
        name = v.crate_name,
        spm = crate_to_spm_target(v.crate_name),
    )
}

fn build_gradle(v: &Vars) -> String {
    format!(
        r#"// Gradle subproject for the `{name}` Whisker module on Android.
// Wired into the consumer app's settings.gradle.kts by whisker-build.

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
            kotlin.srcDirs("src/android")
        }}
    }}
}}

ksp {{
    arg("whisker.moduleName", "{spm}")
    arg("whisker.crateName", "{name}")
}}

dependencies {{
    // Single Whisker runtime dep — :module-api re-exports
    // rs.whisker:annotations transitively. ksp(rs.whisker:ksp)
    // stays separate because it's build-time, not runtime.
    implementation(project(":module-api"))
    ksp("rs.whisker:ksp")
}}
"#,
        name = v.crate_name,
        ns = v.crate_name.replace('-', "_"),
        spm = crate_to_spm_target(v.crate_name),
    )
}

fn lib_rs_view(v: &Vars) -> String {
    format!(
        r#"//! `{name}` — Whisker view-bearing module.
//!
//! Registers a Lynx element under `{name}:{tag}` and exposes the
//! `{tag}` symbol for use inside `render!`. Platform-side classes
//! live under `src/ios/` and `src/android/`.

use whisker::Signal;

/// View-bearing platform component. The Lynx tag the bridge
/// registers against is `{name}:{tag}` — the crate name namespace
/// is auto-prepended by `#[whisker::platform_component]`.
#[whisker::platform_component("{tag}")]
pub fn {ident}(style: Signal<String>) {{}}
"#,
        name = v.crate_name,
        tag = v.tag,
        ident = v.crate_name.replace('-', "_").trim_start_matches("whisker_"),
    )
}

fn lib_rs_module(v: &Vars) -> String {
    format!(
        r#"//! `{name}` — Whisker function-only platform module.
//!
//! Exposes typed Rust -> Kotlin/Swift function calls without
//! rendering UI. Platform-side classes live under `src/ios/` and
//! `src/android/`.

use whisker::platform_module::{{WhiskerModuleError, WhiskerValue}};

/// Sys proxy — every method is a thin pass-through to the
/// platform-side `Whisker{tag}` class via the C bridge.
#[whisker::platform_module(name = "Whisker{tag}")]
pub trait Whisker{tag}Sys {{
    // Add your trait methods here. They MUST take
    // `args: Vec<WhiskerValue>` and return `WhiskerValue`.
    //
    // Example:
    //     fn hello(args: Vec<WhiskerValue>) -> WhiskerValue;
    fn _placeholder(args: Vec<WhiskerValue>) -> WhiskerValue;
}}

/// Typed Rust API — hand-written wrapper above the sys proxy. Build
/// the `Vec<WhiskerValue>` arg list, dispatch through the sys
/// trait, and lift the `WhiskerValue` return into the matching
/// typed result.
pub struct Whisker{tag};
impl Whisker{tag} {{
    pub fn placeholder() -> Result<(), WhiskerModuleError> {{
        let _ = Whisker{tag}Sys::_placeholder(vec![]);
        Ok(())
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
    )
}

fn swift_view(v: &Vars) -> String {
    format!(
        r#"// `{class}` — iOS side of the `{name}:{tag}` Whisker component.

import UIKit
import WhiskerComponents
import WhiskerModuleApi

@WhiskerComponent("{tag}")
@objc({class})
public final class {class}: WhiskerUI<UIView> {{
    @objc public override func createView() -> UIView {{
        let v = UIView()
        v.backgroundColor = .systemPink
        return v
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        class = v.class_name,
    )
}

fn kotlin_view(v: &Vars) -> String {
    format!(
        r#"// `{class}` -- Android side of the `{name}:{tag}` Whisker component.

package rs.whisker.modules.{ns}

import android.content.Context
import android.graphics.Color
import android.view.View
import rs.whisker.annotations.WhiskerComponent
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI

@WhiskerComponent("{tag}")
open class {class}(context: WhiskerContext) : WhiskerUI<View>(context) {{
    override fun createView(context: Context): View {{
        val v = View(context)
        v.setBackgroundColor(Color.argb(0xff, 0xff, 0x80, 0xa0))
        return v
    }}
}}
"#,
        name = v.crate_name,
        tag = v.tag,
        class = v.class_name,
        ns = v.crate_name.replace('-', "_"),
    )
}

fn swift_module(v: &Vars, module_class: &str) -> String {
    format!(
        r#"// `{class}Impl` — iOS side of the `{name}` Whisker function-only module.

import Foundation
import WhiskerComponents
import WhiskerModuleApi

@WhiskerModule("{class}")
@objc({class}Impl)
public final class {class}Impl: NSObject {{
    @objc public func _placeholder(_ args: [WhiskerValue]) -> WhiskerValue {{
        // TODO: implement the function the Rust-side sys trait declares.
        return .null
    }}
}}
"#,
        name = v.crate_name,
        class = module_class,
    )
}

fn kotlin_module(v: &Vars, module_class: &str) -> String {
    format!(
        r#"// `{class}Impl` -- Android side of the `{name}` Whisker function-only module.

package rs.whisker.modules.{ns}

import rs.whisker.annotations.WhiskerModule
import rs.whisker.runtime.WhiskerValue

@WhiskerModule("{class}")
class {class}Impl {{
    fun _placeholder(args: List<WhiskerValue>): WhiskerValue {{
        // TODO: implement the function the Rust-side sys trait declares.
        return WhiskerValue.Null
    }}
}}
"#,
        name = v.crate_name,
        class = module_class,
        ns = v.crate_name.replace('-', "_"),
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
    let stripped = crate_name
        .strip_prefix("whisker-")
        .unwrap_or(crate_name);
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
