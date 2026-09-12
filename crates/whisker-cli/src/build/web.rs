//! `whisker build web` — release build of the CNG-generated Web project.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::path::PathBuf;
use whisker_build::Profile;
use whisker_dev_server::Target;

use crate::manifest;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: Args, no_tui: bool) -> Result<()> {
    let manifest = manifest::resolve_for_target(args.manifest_path.as_deref(), Target::Web)?;
    let workspace_root = manifest.workspace_root.clone();
    let build_ui = super::BuildUi::start(no_tui, "Web", &manifest.package);
    let sync = manifest.project(Target::Web)?;
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
    build_ui.complete(&artifacts.index_html);
    Ok(())
}
