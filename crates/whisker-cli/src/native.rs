//! Glue between `whisker-cng` and the CLI.
//!
//! Responsibilities split:
//!
//! - `whisker-cng` owns the *pure* renderer: AppConfig + paths → files
//!   on disk. No shelling out, no environment assumptions. Pure logic
//!   so it stays unit-testable against tempdirs.
//! - This module decides *where* the gen dirs live (always
//!   `<crate_dir>/gen/{android,ios}`), resolves the Whisker native
//!   runtime paths (today: `<workspace>/native/{android,ios}`), and
//!   handles the side-effect bits that follow a sync — running
//!   `xcodegen generate` after iOS regeneration so the
//!   `<scheme>.xcodeproj` is fresh before `xcodebuild` runs.
//!
//! Public entry point: [`sync_for_target`]. The cli's `run` and
//! `build` subcommands call this before kicking off the rest of the
//! build pipeline.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use whisker_app_config::AppConfig;
use whisker_dev_server::Target;

/// Run the platform-appropriate sync for `target`. Returns the gen
/// directory the caller should hand to gradle / xcodebuild — useful
/// even for the fast-path (`regenerated == false`) case.
pub fn sync_for_target(
    target: Target,
    app_config: &AppConfig,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<NativeSync> {
    match target {
        Target::Android => sync_android(app_config, crate_dir, workspace_root, package),
        Target::IosSimulator => sync_ios(app_config, crate_dir, workspace_root),
        Target::Host => Ok(NativeSync {
            gen_dir: crate_dir.to_path_buf(),
            regenerated: false,
        }),
    }
}

/// Outcome of one sync_native pass.
#[derive(Debug, Clone)]
pub struct NativeSync {
    /// Where the generated project tree lives — `gen/android/` or
    /// `gen/ios/` under `crate_dir`. For `Target::Host` this is just
    /// `crate_dir` (no native project to generate).
    pub gen_dir: PathBuf,
    /// `true` if the renderer rewrote files this pass, `false` if the
    /// fingerprint matched and the existing tree was reused.
    pub regenerated: bool,
}

fn sync_android(
    app_config: &AppConfig,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<NativeSync> {
    let whisker_runtime = resolve_whisker_native(workspace_root, "android/whisker-runtime")
        .context("resolve Whisker's native/android/whisker-runtime")?;
    let inputs = whisker_cng::android::inputs_from(
        app_config,
        package.replace('-', "_"),
        whisker_runtime,
    )?;
    let gen_dir = crate_dir.join("gen/android");
    let regenerated = whisker_cng::sync_android(&gen_dir, &inputs)
        .context("render gen/android")?;
    Ok(NativeSync {
        gen_dir,
        regenerated,
    })
}

fn sync_ios(
    app_config: &AppConfig,
    crate_dir: &Path,
    workspace_root: &Path,
) -> Result<NativeSync> {
    let whisker_runtime = resolve_whisker_native(workspace_root, "ios")
        .context("resolve Whisker's native/ios")?;
    let inputs = whisker_cng::ios::inputs_from(app_config, whisker_runtime)?;
    let gen_dir = crate_dir.join("gen/ios");
    let regenerated = whisker_cng::sync_ios(&gen_dir, &inputs)
        .context("render gen/ios")?;
    if regenerated {
        run_xcodegen(&gen_dir).context("xcodegen generate (iOS project sync)")?;
    }
    Ok(NativeSync {
        gen_dir,
        regenerated,
    })
}

/// Locate the Whisker-provided native subtree (Android Gradle module
/// or iOS SPM package). Today the only source is the in-workspace
/// `native/` dir; for external users this'll move to a downloaded
/// cache once `whisker-cli` learns to fetch published Lynx artifacts.
/// The clear-cut error message points at that future feature so
/// surprised users have a thread to pull.
fn resolve_whisker_native(workspace_root: &Path, relative: &str) -> Result<PathBuf> {
    let p = workspace_root.join("native").join(relative);
    if !p.exists() {
        return Err(anyhow!(
            "Whisker native runtime not found at {}.\n\
             Today `whisker-cli` only resolves the in-workspace `native/` tree; \
             external installs need the Lynx artifacts download feature (not \
             yet implemented).",
            p.display(),
        ));
    }
    Ok(p)
}

/// Run `xcodegen generate` in `gen_ios_dir`. `xcodegen` needs to be
/// on PATH — `whisker doctor` flags it as a required tool.
fn run_xcodegen(gen_ios_dir: &Path) -> Result<()> {
    eprintln!(
        "[whisker-cng] xcodegen generate ({})",
        gen_ios_dir.display(),
    );
    let status = Command::new("xcodegen")
        .arg("generate")
        .current_dir(gen_ios_dir)
        .status()
        .context("spawn xcodegen; is it installed and on PATH?")?;
    if !status.success() {
        return Err(anyhow!("xcodegen generate failed ({status})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_target_returns_crate_dir_without_regenerating() {
        let cfg = AppConfig::default();
        let crate_dir = PathBuf::from("/tmp/crate");
        let ws = PathBuf::from("/tmp/ws");
        let sync = sync_for_target(Target::Host, &cfg, &crate_dir, &ws, "pkg").unwrap();
        assert_eq!(sync.gen_dir, crate_dir);
        assert!(!sync.regenerated);
    }

    #[test]
    fn resolve_whisker_native_errors_when_missing() {
        let err = resolve_whisker_native(
            &PathBuf::from("/definitely-not-a-real-path"),
            "android/whisker-runtime",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("native runtime not found"),
            "got: {err:#}",
        );
    }
}
