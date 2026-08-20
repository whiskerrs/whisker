//! `whisker build macos` — release build of the CNG-generated macOS project.

use anyhow::{Context, Result, anyhow};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use whisker_build::Profile;
use whisker_dev_server::Target;

use crate::{manifest, platforms};

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!("`whisker build macos` must run on macOS");

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
    })?;
    whisker_build::ui::info(format!("✓ {}", bundle.display()));
    whisker_build::ui::info("bundle is unsigned; distribution signing/notarization is a follow-up");
    Ok(())
}
