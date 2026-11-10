//! Build the CNG-generated macOS Xcode project.
//!
//! Both `whisker build macos` and `whisker run macos` call [`build_app`].
//! The Rust app is compiled by the project's own run-script phase
//! (`whisker build-ios --platform=macosx`), so cargo features and the hot
//! reload capture envelope ride along on the `xcodebuild` environment.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Profile, ui};

/// Inputs needed to compile one macOS `.app` bundle.
pub struct MacosBuild<'a> {
    /// Generated `gen/macos` project directory.
    pub project_dir: &'a Path,
    /// Xcode project and scheme name.
    pub project_name: &'a str,
    /// `xcodebuild -derivedDataPath`.
    pub derived_data: &'a Path,
    /// Xcode configuration.
    pub profile: Profile,
    /// Cargo features forwarded to the Rust build phase.
    pub features: &'a [String],
    /// Optional hot-patch capture envelope for development builds.
    pub capture: Option<&'a crate::CaptureShims>,
}

/// Rust target triple of the macOS slice this host runs natively.
pub fn host_triple() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64-apple-darwin"
    } else {
        "aarch64-apple-darwin"
    }
}

/// Compiles the generated project and returns the built `.app` path.
pub fn build_app(inputs: &MacosBuild<'_>) -> Result<PathBuf> {
    let project = inputs
        .project_dir
        .join(format!("{}.xcodeproj", inputs.project_name));
    if !project.is_dir() {
        return Err(anyhow!(
            "Xcode project missing at {}; regenerate gen/macos",
            project.display()
        ));
    }
    let configuration = configuration(inputs.profile);
    let step = ui::step(
        ui::OperationKind::Xcodebuild,
        inputs.project_name.to_string(),
    );
    let mut command = Command::new("xcodebuild");
    command
        .arg("-project")
        .arg(&project)
        .args(["-scheme", inputs.project_name])
        .args(["-configuration", configuration])
        .args(["-destination", "platform=macOS"])
        .arg("-derivedDataPath")
        .arg(inputs.derived_data)
        // Whisker's codegen plugin and compiler macros have separate trust prompts that headless builds cannot answer.
        .arg("-skipPackagePluginValidation")
        .arg("-skipMacroValidation");
    if inputs.profile == Profile::Debug {
        command.arg(format!("ARCHS={}", xcode_arch()));
    }
    command.args(["-quiet", "build"]);
    if let Ok(cli) = std::env::current_exe() {
        command.env("WHISKER_CLI", cli);
    }
    if !inputs.features.is_empty() {
        command.env("WHISKER_FEATURES", inputs.features.join(" "));
    }
    if let Some(capture) = inputs.capture {
        for (key, value) in crate::capture_env_vars_for_triple(capture, Some(host_triple())) {
            command.env(key, value);
        }
    }
    let status = step.pipe(&mut command).context("spawn xcodebuild")?;
    if !status.success() {
        step.fail(status.to_string());
        return Err(anyhow!("xcodebuild failed ({status})"));
    }
    step.done("");
    let app = inputs
        .derived_data
        .join("Build/Products")
        .join(configuration)
        .join(format!("{}.app", inputs.project_name));
    if !app.is_dir() {
        return Err(anyhow!(
            "xcodebuild succeeded but {} is missing",
            app.display()
        ));
    }
    Ok(app)
}

/// Executable inside a bundle produced by [`build_app`].
pub fn app_executable(app: &Path, project_name: &str) -> PathBuf {
    app.join("Contents/MacOS").join(project_name)
}

/// Single-arch Rust dylib the run-script phase compiled; hot patching reads
/// the running app's symbols from this file.
pub fn driver_dylib(workspace_root: &Path, package: &str) -> PathBuf {
    workspace_root
        .join("target")
        .join(host_triple())
        .join("release")
        .join(format!("lib{}.dylib", package.replace('-', "_")))
}

fn configuration(profile: Profile) -> &'static str {
    match profile {
        Profile::Debug => "Debug",
        Profile::Release => "Release",
    }
}

fn xcode_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "arm64"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_dylib_uses_the_host_triple_and_crate_name() {
        assert_eq!(
            driver_dylib(Path::new("/ws"), "whisker-news"),
            Path::new("/ws/target")
                .join(host_triple())
                .join("release/libwhisker_news.dylib")
        );
    }
}
