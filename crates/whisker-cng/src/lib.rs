//! Whisker CNG (Continuous Native Generation).
//!
//! Renders platform Host projects under `gen/<platform>/`
//! from the user's `whisker.rs` (= [`whisker_config::Config`]).
//! Drift between the in-tree files and the current config is detected
//! via a content-hashed fingerprint stored alongside each generated
//! tree (`gen/<platform>/.whisker-fingerprint`).
//!
//! Modelled on Expo's CNG: the declarative config is the source of
//! truth and `gen/` is a build artifact, never committed. Unlike Expo
//! there is no separate `whisker generate` command — every command
//! that needs the native tree (`whisker run`, `whisker build`) calls
//! [`sync_android`] / [`sync_ios`] first, and the fingerprint-match
//! fast path is a single file read.
//!
//! ## Public entry points
//!
//! - [`sync_android`] / [`sync_ios`] / [`sync_macos`] — render-or-skip for one
//!   platform. Returns whether files were actually rewritten.
//! - [`AndroidInputs`] / [`IosInputs`] — the renderer's input bundle.
//!   Build them yourself for full control, or use
//!   [`android::inputs_from`] / [`ios::inputs_from`] for the
//!   "extract from Config + defaults" path.
//!
//! The crate has no CLI surface and shells out to nothing —
//! `whisker-cli` runs `xcodegen`, `gradle`, etc. after a sync
//! completes, which keeps the renderer unit-testable against tempdirs.

pub mod android;
pub mod compose;
pub mod discovery;
mod fingerprint;
pub mod ios;
pub mod macos;
pub mod plugins;
mod render;
pub mod web;

pub use android::{AndroidInputs, sync as sync_android};
pub use compose::{EnabledTargets, Engine, SubprocessPlugin};
pub use discovery::{DiscoveredPlugin, discover_plugins};
pub use ios::{IosInputs, sync as sync_ios};
pub use macos::{MacosInputs, sync as sync_macos};
pub use web::{WebInputs, sync as sync_web};
pub use whisker_config::Config;

/// One Cargo module crate and target definition wired into a generated Rust Host.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RustElementModuleInput {
    /// Cargo package name of the platform-neutral element crate.
    pub package: String,
    /// Absolute platform-neutral module crate directory.
    pub crate_path: std::path::PathBuf,
    /// Cargo package name of the target Host library.
    pub host_package: String,
    /// Cargo source of the target Host library.
    pub host_dependency: RustHostDependency,
}

/// Cargo dependency source selected for a Rust Host contribution.
#[derive(Clone, Debug, serde::Serialize)]
pub enum RustHostDependency {
    /// Nested package available in a local path or git checkout.
    Path(std::path::PathBuf),
    /// Separately published Host package at the common module's version.
    Registry { version: String },
}

fn rust_element_module_dependencies(modules: &[RustElementModuleInput]) -> String {
    modules
        .iter()
        .map(|module| {
            let host_dependency = match &module.host_dependency {
                RustHostDependency::Path(path) => {
                    format!("path = {:?}", path.display().to_string())
                }
                RustHostDependency::Registry { version } => {
                    format!("version = {:?}", format!("={version}"))
                }
            };
            format!(
                "{} = {{ package = {:?}, path = {:?} }}\n{} = {{ package = {:?}, {} }}",
                module.package,
                module.package,
                module.crate_path.display().to_string(),
                module.host_package,
                module.host_package,
                host_dependency,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn rust_element_module_config(modules: &[RustElementModuleInput]) -> String {
    modules
        .iter()
        .map(|module| {
            format!(
                "\n            .with_element_module({}::__whisker_element_module_definition())\
                 \n            .with_module_definition({}::__whisker_module_definition())",
                rust_crate_name(&module.package),
                rust_crate_name(&module.host_package),
            )
        })
        .collect()
}

fn rust_crate_name(package: &str) -> String {
    package
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod rust_host_dependency_tests {
    use super::*;

    #[test]
    fn registry_host_dependency_is_pinned_to_the_common_module_version() {
        let dependencies = rust_element_module_dependencies(&[RustElementModuleInput {
            package: "whisker-toggle".into(),
            crate_path: "/cargo/registry/whisker-toggle-1.2.3".into(),
            host_package: "whisker-toggle-web-host".into(),
            host_dependency: RustHostDependency::Registry {
                version: "1.2.3".into(),
            },
        }]);

        assert!(dependencies.contains(
            "whisker-toggle-web-host = { package = \"whisker-toggle-web-host\", version = \"=1.2.3\" }"
        ));
        assert!(
            !dependencies.contains(
                "whisker-toggle-web-host = { package = \"whisker-toggle-web-host\", path ="
            )
        );
    }
}
