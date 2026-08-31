//! `whisker build web` — release build of the CNG-generated Web project.

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
    let manifest = manifest::resolve(args.manifest_path.as_deref())?;
    let workspace_root = crate::run::find_workspace_root(&manifest.crate_dir).ok_or_else(|| {
        anyhow!(
            "no [workspace] Cargo.toml at or above {}",
            manifest.crate_dir.display()
        )
    })?;
    let sync = platforms::sync_for_target(
        Target::Web,
        &manifest.config,
        &manifest.crate_dir,
        &workspace_root,
        &manifest.package,
    )
    .context("sync gen/web")?;
    let dist = sync.gen_dir.join("dist");
    let artifacts = whisker_build::web::build(&whisker_build::web::WebBuild {
        project_dir: sync.gen_dir,
        target_dir: workspace_root.join("target/.whisker/web"),
        dist_dir: dist.clone(),
        package: format!("{}-whisker-web", manifest.package),
        profile: Profile::Release,
        features: Vec::new(),
        capture: None,
        development: false,
    })?;
    whisker_build::ui::info(format!("✓ {}", artifacts.index_html.display()));
    Ok(())
}
