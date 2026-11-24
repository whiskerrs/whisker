//! `whisker clean` — remove build artifacts no recent whisker build uses.

use anyhow::{Context, Result, anyhow};
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Args, Debug)]
pub struct CleanArgs {
    /// Path to the user crate's `Cargo.toml`. Defaults to walking up from
    /// `cwd` until a `Cargo.toml` with a `[package]` section is found.
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,
    /// Also delete this workspace's whisker state (`target/.whisker`: hot
    /// reload captures, patches, Xcode DerivedData) and stop keeping the
    /// build artifacts only this workspace uses in a shared target directory.
    #[arg(long)]
    pub all: bool,
}

pub fn run(args: CleanArgs) -> Result<()> {
    let cargo_toml = crate::manifest::application_manifest(args.manifest_path.as_deref())?;
    let workspace_root = workspace_root(&cargo_toml)?;
    let mut freed = 0;
    if args.all {
        whisker_build::gc::forget_workspace(&workspace_root)?;
        let state = workspace_root.join("target/.whisker");
        if state.is_dir() {
            freed += dir_size(&state);
            std::fs::remove_dir_all(&state)
                .with_context(|| format!("remove {}", state.display()))?;
        }
    }
    let stats = whisker_build::gc::sweep_target_dir(&workspace_root)?;
    freed += stats.bytes;
    eprintln!(
        "removed {} stale build artifact(s) from {}, {} freed",
        stats.entries,
        whisker_build::cargo_target_dir(&workspace_root).display(),
        whisker_build::gc::human_bytes(freed),
    );
    Ok(())
}

fn workspace_root(cargo_toml: &Path) -> Result<PathBuf> {
    let output =
        std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(["locate-project", "--workspace", "--message-format", "plain"])
            .arg("--manifest-path")
            .arg(cargo_toml)
            .output()
            .context("run cargo locate-project")?;
    if !output.status.success() {
        return Err(anyhow!(
            "cargo locate-project failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let manifest = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("workspace manifest {} has no parent", manifest.display()))
}

fn dir_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => dir_size(&entry.path()),
            _ => entry.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}
