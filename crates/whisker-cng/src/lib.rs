//! Generate platform projects by executing an application's `whisker.rs`.
//!
//! Call [`run`] from the configuration binary's `main`. The `generate` feature
//! enables project generation, plugin builds, and Cargo dependency discovery.
//! Applications can disable default features and enable `generate` only through
//! their configuration binary's required feature. Configuration types and the
//! [`run`] interface remain available to editors with generation disabled.
//!
//! ```no_run
//! whisker_cng::run(|app| {
//!     app.name("My App").bundle_id("com.example.myapp");
//! });
//! ```

mod runner;
pub use runner::{GenerationReport, GenerationTarget, PlatformSync, run};
#[cfg(feature = "generate")]
mod generator;
#[cfg(feature = "generate")]
pub use generator::sync_for_target;
#[cfg(feature = "generate")]
mod project;
#[cfg(feature = "generate")]
pub use project::generate;

#[cfg(feature = "generate")]
pub mod android;
#[cfg(feature = "generate")]
mod background;
#[cfg(feature = "generate")]
pub mod compose;
#[cfg(feature = "generate")]
pub mod dependency_graph;
#[cfg(feature = "generate")]
pub mod discovery;
#[cfg(feature = "generate")]
mod fingerprint;
#[cfg(feature = "generate")]
pub mod ios;
#[cfg(feature = "generate")]
pub mod ios_modules;
#[cfg(feature = "generate")]
pub mod macos;
#[cfg(feature = "generate")]
pub mod modules;
#[cfg(feature = "generate")]
pub mod plugins;
#[cfg(feature = "generate")]
mod render;
#[cfg(feature = "generate")]
pub mod web;

#[cfg(feature = "generate")]
pub use android::{AndroidInputs, sync as sync_android};
#[cfg(feature = "generate")]
pub use compose::{EnabledTargets, Engine, SubprocessPlugin};
#[cfg(feature = "generate")]
pub use dependency_graph::ProjectDependencyGraph;
#[cfg(feature = "generate")]
pub use discovery::{DiscoveredPlugin, discover_plugins};
#[cfg(feature = "generate")]
pub use ios::{IosInputs, sync as sync_ios};
#[cfg(feature = "generate")]
pub use macos::{MacosInputs, sync as sync_macos};
#[cfg(feature = "generate")]
pub use modules::{
    ModulePlatform, NativeManifestKind, ResolvedModule, ResolvedNativeManifest,
    ResolvedPlatformImplementation, ResolvedRustHostSource, ResolvedRustModuleContribution,
    build_modules_report, discover as discover_modules, refresh_gradle_module_cache,
};
#[cfg(feature = "generate")]
pub use web::{WebInputs, sync as sync_web};
pub use whisker_config::*;

/// One Cargo module crate and target definition wired into a generated Rust Host.
#[derive(Clone, Debug, serde::Serialize)]
#[cfg(feature = "generate")]
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
#[cfg(feature = "generate")]
pub enum RustHostDependency {
    /// Nested package available in a local path or git checkout.
    Path(std::path::PathBuf),
    /// Separately published Host package at the common module's version.
    Registry { version: String },
}

#[cfg(feature = "generate")]
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

#[cfg(feature = "generate")]
fn rust_element_module_config(modules: &[RustElementModuleInput]) -> String {
    modules
        .iter()
        .map(|module| {
            format!(
                "\n            .with_module(\n                {}::__whisker_element_module_definition(),\n                {}::__whisker_module_definition(),\n            )",
                rust_crate_name(&module.package),
                rust_crate_name(&module.host_package),
            )
        })
        .collect()
}

#[cfg(feature = "generate")]
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

#[cfg(all(test, feature = "generate"))]
mod rust_host_dependency_tests {
    use super::*;

    #[test]
    fn registry_host_dependency_is_pinned_to_the_common_module_version() {
        let dependencies = rust_element_module_dependencies(&[RustElementModuleInput {
            package: "whisker-example".into(),
            crate_path: "/cargo/registry/whisker-example-1.2.3".into(),
            host_package: "whisker-example-web".into(),
            host_dependency: RustHostDependency::Registry {
                version: "1.2.3".into(),
            },
        }]);

        assert!(dependencies.contains(
            "whisker-example-web = { package = \"whisker-example-web\", version = \"=1.2.3\" }"
        ));
        assert!(
            !dependencies
                .contains("whisker-example-web = { package = \"whisker-example-web\", path =")
        );
    }
}
