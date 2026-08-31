//! Build and bundle a CNG-generated Cargo-based macOS project.
//!
//! Both `whisker build macos` and `whisker run desktop` call this module. The
//! generated `gen/macos` tree is therefore the single platform project rather
//! than a development-only launcher.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use crate::{Profile, ui};

/// Inputs needed to compile and assemble one macOS `.app` bundle.
pub struct MacosBuild<'a> {
    /// Generated `gen/macos` project directory.
    pub project_dir: &'a Path,
    /// Cargo target directory shared by run and build.
    pub target_dir: &'a Path,
    /// Human-readable `.app` bundle name.
    pub app_name: &'a str,
    /// Generated Cargo binary/package name.
    pub binary_name: &'a str,
    /// Cargo build profile.
    pub profile: Profile,
    /// Generated Host features enabled for this build.
    pub features: &'a [String],
    /// Optional hot-patch capture envelope for development builds.
    pub capture: Option<&'a crate::CaptureShims>,
}

/// Compiles the generated project and returns the assembled `.app` path.
pub fn build_app(inputs: &MacosBuild<'_>) -> Result<PathBuf> {
    let manifest = inputs.project_dir.join("Cargo.toml");
    if !manifest.is_file() {
        bail!(
            "generated macOS Cargo project missing at {}",
            manifest.display()
        );
    }
    let step = ui::step(
        "cargo",
        format!("{} ({:?})", inputs.binary_name, inputs.profile),
    );
    let mut command = std::process::Command::new("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(inputs.target_dir);
    if matches!(inputs.profile, Profile::Release) {
        command.arg("--release");
    }
    if !inputs.features.is_empty() {
        command.arg("--features").arg(inputs.features.join(","));
    }
    if let Some(capture) = inputs.capture {
        for (key, value) in crate::capture_env_vars_all_crates(capture) {
            command.env(key, value);
        }
    }
    let status = step
        .pipe(&mut command)
        .context("spawn cargo for macOS Host")?;
    if !status.success() {
        step.fail(status.to_string());
        bail!("cargo build for macOS Host failed ({status})");
    }
    step.done("");

    let profile_dir = match inputs.profile {
        Profile::Debug => "debug",
        Profile::Release => "release",
    };
    let executable = inputs.target_dir.join(profile_dir).join(inputs.binary_name);
    if !executable.is_file() {
        bail!(
            "macOS Host executable missing after cargo build: {}",
            executable.display()
        );
    }

    let bundle = inputs
        .target_dir
        .join("bundles")
        .join(profile_dir)
        .join(format!("{}.app", inputs.app_name));
    if bundle.exists() {
        std::fs::remove_dir_all(&bundle)
            .with_context(|| format!("remove stale bundle {}", bundle.display()))?;
    }
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    std::fs::create_dir_all(&macos).with_context(|| format!("create {}", macos.display()))?;
    std::fs::create_dir_all(&resources)
        .with_context(|| format!("create {}", resources.display()))?;
    std::fs::copy(&executable, macos.join(inputs.binary_name))
        .with_context(|| format!("copy executable into {}", bundle.display()))?;
    std::fs::copy(
        inputs.project_dir.join("Info.plist"),
        contents.join("Info.plist"),
    )
    .with_context(|| format!("copy Info.plist into {}", bundle.display()))?;
    copy_tree_if_present(&inputs.project_dir.join("Resources"), &resources)?;
    Ok(bundle)
}

fn copy_tree_if_present(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    for entry in
        std::fs::read_dir(source).with_context(|| format!("read resources {}", source.display()))?
    {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            std::fs::create_dir_all(&to)
                .with_context(|| format!("create resource directory {}", to.display()))?;
            copy_tree_if_present(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy resource {}", from.display()))?;
        }
    }
    Ok(())
}
