//! `whisker build macos` — release build of the CNG-generated macOS project.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use anyhow::{Context, anyhow};
#[cfg(target_os = "macos")]
use whisker_build::Profile;
#[cfg(target_os = "macos")]
use whisker_dev_server::Target;

#[cfg(target_os = "macos")]
use crate::{manifest, platforms};

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

#[cfg(not(target_os = "macos"))]
pub fn run(_args: Args, _no_tui: bool) -> Result<()> {
    anyhow::bail!("`whisker build macos` must run on macOS")
}

#[cfg(target_os = "macos")]
pub fn run(args: Args, no_tui: bool) -> Result<()> {
    let manifest = manifest::resolve(args.manifest_path.as_deref())?;
    let workspace_root = crate::run::find_workspace_root(&manifest.crate_dir).ok_or_else(|| {
        anyhow!(
            "no [workspace] Cargo.toml at or above {}",
            manifest.crate_dir.display()
        )
    })?;
    let app_name = manifest
        .config
        .name
        .as_deref()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required for macOS"))?;
    let build_ui = super::BuildUi::start(no_tui, "Desktop (macOS)", app_name);
    let sync = platforms::sync_for_target(
        Target::Macos,
        &manifest.config,
        &manifest.crate_dir,
        &workspace_root,
        &manifest.package,
    )
    .context("sync gen/macos")?;
    let binary_name = format!("{}-whisker-macos", manifest.package);
    let target_dir = workspace_root.join("target/.whisker/macos");
    whisker_build::ui::section("Build");
    whisker_build::ui::info(format!("building {app_name} — release macOS .app"));
    let bundle = whisker_build::macos::build_app(&whisker_build::macos::MacosBuild {
        project_dir: &sync.gen_dir,
        target_dir: &target_dir,
        app_name,
        binary_name: &binary_name,
        profile: Profile::Release,
        features: &[],
        capture: None,
    })?;
    build_ui.complete(&bundle);
    whisker_build::ui::info("bundle is unsigned; distribution signing/notarization is a follow-up");
    Ok(())
}
