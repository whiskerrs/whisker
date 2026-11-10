//! Render the AppKit Host Xcode project under `gen/macos/`.
//!
//! The project mirrors `gen/ios`: a Swift application target links the Rust
//! app as `libWhiskerDriver.dylib`, built by a run-script phase, and depends on
//! a generated SwiftPM aggregator for the Host SDK and native modules.

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use whisker_config::Config;
use whisker_plugin::FileEntry;
use whisker_plugin::project::{MacosProjectIr, ProjectIr};

use crate::modules::ResolvedModule;
use crate::render::render;

pub mod application;
pub(crate) mod icon;

const APP_DELEGATE_SWIFT: &str = include_str!("templates/macos/Sources/AppDelegate.swift");

/// Rust app library name the Xcode target links and embeds.
pub const DRIVER_LIBRARY: &str = "libWhiskerDriver.dylib";

/// Fully resolved inputs for one generated macOS project.
#[derive(Clone, Debug, serde::Serialize)]
pub struct MacosInputs {
    /// Human-readable application name, Xcode project name, and scheme.
    pub app_name: String,
    /// Static Host background configured in `whisker.rs` (`#RRGGBB`).
    pub background: String,
    /// Reverse-DNS bundle identifier.
    pub bundle_id: String,
    /// User-visible semantic version.
    pub version: String,
    /// Monotonic bundle build number.
    pub build_number: u32,
    /// Cargo package name of the user's application crate.
    pub user_package: String,
    /// Absolute path to the user's application crate.
    pub user_crate_path: PathBuf,
    /// Cargo workspace root the run-script phase builds from.
    pub workspace_root: PathBuf,
    /// Modules whose macOS SwiftPM packages join the aggregator.
    pub modules: Vec<ResolvedModule>,
    /// Local Swift package root for the Host SDK, named `whisker`. `None`
    /// resolves the released package unless `workspace_root` is a Whisker
    /// source checkout.
    pub host_sdk: Option<PathBuf>,
    /// `MACOSX_DEPLOYMENT_TARGET` of the application target.
    pub minimum_system_version: String,
    /// Source PNG configured through the shared AppIcon plugin.
    pub app_icon_png: Option<Vec<u8>>,
    /// Bumped whenever the generated project shape changes.
    pub template_version: u32,
    /// Application Cargo inputs included in the generation fingerprint.
    pub cargo_selection: crate::CargoSelection,
}

/// Final declarations and filesystem context for one macOS Xcode project.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MacosProjectInputs {
    pub project: MacosProjectIr,
    pub project_name: String,
    pub app_crate_dir: Option<PathBuf>,
    pub cargo_selection: crate::CargoSelection,
    pub template_version: u32,
}

/// Generates or reuses the complete `gen/macos` project.
pub fn sync(out_dir: &Path, inputs: &MacosInputs) -> Result<bool> {
    let result = crate::ProjectEngine::with_macos_application(inputs.clone())
        .compose(&Config::default(), &ProjectIr::Macos(Default::default()))?;
    let ProjectIr::Macos(project) = result.project else {
        unreachable!()
    };
    sync_project(
        out_dir,
        &MacosProjectInputs {
            project,
            project_name: inputs.app_name.clone(),
            app_crate_dir: Some(inputs.user_crate_path.clone()),
            cargo_selection: inputs.cargo_selection.clone(),
            template_version: inputs.template_version,
        },
    )
}

/// Render all native metadata and stage declared source/resource files.
pub fn render_project(
    inputs: &MacosProjectInputs,
) -> Result<BTreeMap<whisker_plugin::project::ProjectPath, FileEntry>> {
    crate::ios::render_apple(
        &ProjectIr::Macos(inputs.project.clone()),
        &inputs.project_name,
        inputs.app_crate_dir.as_deref(),
    )
}

/// Preflight every output and copied input before replacing the managed tree.
pub fn sync_project(out: &Path, inputs: &MacosProjectInputs) -> Result<bool> {
    crate::ios::sync_files(out, inputs, render_project(inputs)?, &inputs.project_name)
}

/// Resolves macOS fields from the declarative application config.
pub fn inputs_from(
    app_config: &Config,
    user_package: String,
    user_crate_path: PathBuf,
    workspace_root: PathBuf,
) -> Result<MacosInputs> {
    let crate::plugins::application::MacosApplication {
        app_name,
        bundle_id,
        version,
        build_number,
        minimum_system_version,
    } = crate::plugins::application::macos(app_config)?;
    let background = crate::background::AppBackground::resolve(app_config)?;
    let app_icon_png = icon::source(app_config, &user_crate_path)?;
    Ok(MacosInputs {
        app_name,
        background: background.hex().to_string(),
        bundle_id,
        version,
        build_number,
        user_package,
        user_crate_path,
        workspace_root,
        modules: Vec::new(),
        host_sdk: None,
        minimum_system_version,
        app_icon_png,
        template_version: 14,
        cargo_selection: crate::CargoSelection::default(),
    })
}

fn validate(inputs: &MacosInputs) -> Result<()> {
    if inputs.app_name.is_empty() || inputs.app_name.contains(['/', ':']) {
        bail!("macOS app name must be non-empty and contain neither '/' nor ':'");
    }
    if inputs.bundle_id.trim().is_empty() {
        bail!("macOS bundle id must not be empty");
    }
    if inputs.user_package.trim().is_empty() {
        bail!("macOS Cargo package name must not be empty");
    }
    let major = inputs
        .minimum_system_version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .context("invalid macOS deployment target")?;
    if major < crate::ios_modules::MACOS_PLATFORM_MAJOR {
        bail!(
            "macOS deployment target {} is below the AppKit Host minimum {}.0",
            inputs.minimum_system_version,
            crate::ios_modules::MACOS_PLATFORM_MAJOR
        );
    }
    crate::background::AppBackground::parse(&inputs.background)
        .context("validate macOS application background")?;
    Ok(())
}

fn template_vars(inputs: &MacosInputs) -> std::collections::HashMap<&'static str, String> {
    let mut vars = std::collections::HashMap::new();
    vars.insert("app_title_swift", swift_string_body(&inputs.app_name));
    let background = crate::background::AppBackground::parse(&inputs.background)
        .expect("MacosInputs background is validated when it is resolved");
    let [red, green, blue] = background.rgb();
    vars.insert("background_red", color_component(red));
    vars.insert("background_green", color_component(green));
    vars.insert("background_blue", color_component(blue));
    vars.insert(
        "whisker_workspace_root",
        shell_string_body(&inputs.workspace_root.display().to_string()),
    );
    vars.insert(
        "whisker_user_package",
        shell_string_body(&inputs.user_package),
    );
    vars
}

fn color_component(value: u8) -> String {
    format!("{:.6}", f32::from(value) / 255.0)
}

fn swift_string_body(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\\' => "\\\\".into(),
            '"' => "\\\"".into(),
            c if c.is_control() => format!("\\u{{{:x}}}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

fn shell_string_body(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

#[cfg(test)]
mod tests;
