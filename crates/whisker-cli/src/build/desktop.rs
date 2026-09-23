//! Cargo distribution builds for Windows and Linux.
use anyhow::{Context, Result};
use std::path::PathBuf;
use whisker_cng::GenerationTarget;
#[derive(clap::Args, Debug)]
pub struct Args {
    #[command(flatten)]
    cargo: crate::manifest::FeatureArgs,
    #[arg(long)]
    manifest_path: Option<PathBuf>,
    /// Rust target triple. Cross compilation requires its toolchain and sysroot.
    #[arg(long)]
    cargo_target: Option<String>,
}
pub fn run(args: Args, no_tui: bool, platform: GenerationTarget) -> Result<()> {
    let mut selection = args.cargo.selection();
    selection.target = args.cargo_target;
    let manifest = crate::manifest::resolve_with_selection(
        args.manifest_path.as_deref(),
        platform,
        &selection,
    )?;
    let sync = manifest
        .projects
        .get(&platform)
        .context("missing generated desktop project")?;
    let ui = super::BuildUi::start(
        no_tui,
        platform.as_str(),
        manifest.config.name.as_deref().unwrap_or(&manifest.package),
    );
    let target = manifest
        .workspace_root
        .join("target/.whisker")
        .join(platform.as_str())
        .join(&manifest.package);
    let artifact = whisker_build::desktop::build_app(&whisker_build::desktop::DesktopBuild {
        project_dir: &sync.gen_dir,
        target_dir: &target,
        profile: whisker_build::Profile::Release,
    })?;
    ui.complete(&artifact);
    Ok(())
}
