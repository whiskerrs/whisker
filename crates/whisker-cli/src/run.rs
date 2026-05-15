//! `whisker run` — start the dev server.
//!
//! Thin wrapper: resolves the user crate's `whisker.rs` config (via
//! [`super::manifest::resolve`] + [`super::probe::run`]), translates
//! the resulting [`whisker_app_config::AppConfig`] into a flat
//! [`whisker_dev_server::Config`], and hands off to
//! `DevServer::run`. All the heavy lifting (file watch / cargo build
//! / WebSocket push / subsecond patches) lives in
//! `whisker-dev-server` so other host shells (an editor plugin, a
//! notebook front-end, …) can reuse the same loop without a
//! whisker-app-config dependency.

use anyhow::{anyhow, Context, Result};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use whisker_dev_server::{AndroidParams, Config, DevServer, HotPatchMode, IosParams, Target};

use crate::manifest;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Path to the user crate's `Cargo.toml`. Defaults to walking up
    /// from `cwd` until a `Cargo.toml` with a `[package]` section is
    /// found (cargo-style).
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,

    /// Where to deploy the rebuilt artifact.
    #[arg(long, value_enum, default_value_t = CliTarget::Host)]
    pub target: CliTarget,

    /// WebSocket bind address. The Whisker app on the device dials this
    /// (via `WHISKER_DEV_ADDR`) to receive patches.
    #[arg(long, default_value = "127.0.0.1:9876")]
    pub bind: SocketAddr,

    /// Opt out of Tier 1 subsecond hot-patching and fall back to Tier 2
    /// cold rebuilds. `whisker run` defaults to Tier 1; this flag is
    /// for situations where the hot-patch path is misbehaving and you
    /// just want the slower-but-bulletproof path.
    #[arg(long)]
    pub no_hot_patch: bool,

    /// Override the workspace root (= directory containing the
    /// `Cargo.toml` with `[workspace]`). Defaults to walking up from
    /// the resolved manifest's parent dir.
    #[arg(long)]
    pub workspace_root: Option<PathBuf>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliTarget {
    Host,
    Android,
    Ios,
}

impl From<CliTarget> for Target {
    fn from(t: CliTarget) -> Self {
        match t {
            CliTarget::Host => Target::Host,
            CliTarget::Android => Target::Android,
            CliTarget::Ios => Target::IosSimulator,
        }
    }
}

pub fn run(args: Args) -> Result<()> {
    let m = manifest::resolve(args.manifest_path.as_deref())
        .context("resolve user-crate manifest (Cargo.toml + whisker.rs)")?;

    let workspace_root = match args.workspace_root {
        Some(p) => p,
        None => find_workspace_root(&m.crate_dir).ok_or_else(|| {
            anyhow!(
                "no [workspace] Cargo.toml at or above {}",
                m.crate_dir.display()
            )
        })?,
    };

    let target: Target = args.target.into();

    // Sync the native host project (gen/{android,ios}/) before doing
    // anything else. Fast-path on fingerprint match — typical run is
    // a single file read. Errors here (missing whisker.rs fields,
    // missing native runtime) are fatal: there's no point starting
    // the dev loop if we can't build the app it would deploy.
    let sync = crate::native::sync_for_target(
        target,
        &m.config,
        &m.crate_dir,
        &workspace_root,
        &m.package,
    )
    .context("sync native project (gen/{android,ios}/)")?;
    if sync.regenerated {
        eprintln!(
            "[whisker run] native project regenerated at {}",
            sync.gen_dir.display(),
        );
    }

    let android = match target {
        Target::Android => Some(android_params_from(&m, &sync.gen_dir)?),
        _ => None,
    };
    let ios = match target {
        Target::IosSimulator => Some(ios_params_from(&m, &sync.gen_dir)?),
        _ => None,
    };

    let watch_paths = vec![m.crate_dir.join("src"), m.crate_dir.join("whisker.rs")];

    let config = Config {
        workspace_root,
        crate_dir: m.crate_dir,
        package: m.package,
        target,
        watch_paths,
        bind_addr: args.bind,
        // Tier 1 is the dev-loop default — `--no-hot-patch` is the
        // emergency-exit when subsecond is misbehaving and you just
        // need a working cold-rebuild loop.
        hot_patch_mode: if args.no_hot_patch {
            HotPatchMode::Tier2ColdRebuild
        } else {
            HotPatchMode::Tier1Subsecond
        },
        android,
        ios,
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    rt.block_on(DevServer::new(config)?.run())
}

/// Build [`AndroidParams`] from the resolved manifest. Returns an
/// error if the user's `whisker.rs` left required fields (like the
/// `applicationId`) unset.
///
/// `project_dir` is the *generated* Gradle project under
/// `gen/android/` — `whisker-cng` writes the tree, this function just
/// stitches in the `applicationId` + launcher activity for installer
/// use.
fn android_params_from(
    m: &manifest::ResolvedManifest,
    project_dir: &Path,
) -> Result<AndroidParams> {
    let a = &m.config.android;
    let application_id = a
        .application_id
        .clone()
        .or_else(|| m.config.bundle_id.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.android(|a| a.application_id(\"…\")) is required for --target android"
            )
        })?;
    let launcher_activity = a
        .launcher_activity
        .clone()
        .unwrap_or_else(|| ".MainActivity".into());
    Ok(AndroidParams {
        project_dir: project_dir.to_path_buf(),
        application_id,
        launcher_activity,
        // Single-ABI dev loops only — multi-ABI is a release concern.
        abi: "arm64-v8a".into(),
    })
}

/// Build [`IosParams`] from the resolved manifest. `project_dir` is
/// the generated `gen/ios/` tree (after `whisker-cng` + xcodegen
/// have run).
fn ios_params_from(m: &manifest::ResolvedManifest, project_dir: &Path) -> Result<IosParams> {
    let i = &m.config.ios;
    let bundle_id = i
        .bundle_id
        .clone()
        .or_else(|| m.config.bundle_id.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.bundle_id(\"…\")) or app.bundle_id(\"…\") is required for --target ios"
            )
        })?;
    let scheme = i
        .scheme
        .clone()
        .or_else(|| m.config.name.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.scheme(\"…\")) or app.name(\"…\") is required for --target ios"
            )
        })?;
    Ok(IosParams {
        project_dir: project_dir.to_path_buf(),
        scheme,
        bundle_id,
        device_override: std::env::var("WHISKER_IOS_SIMULATOR").ok(),
    })
}

/// Walk up from `start` looking for a `Cargo.toml` containing a
/// `[workspace]` section. Returns the directory holding the matching
/// Cargo.toml, or `None` if we walk off the top of the filesystem.
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let cargo = cur.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(txt) = std::fs::read_to_string(&cargo) {
                if txt.contains("[workspace]") {
                    return Some(cur);
                }
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn cli_target_maps_to_dev_server_target() {
        assert_eq!(Target::from(CliTarget::Host), Target::Host);
        assert_eq!(Target::from(CliTarget::Android), Target::Android);
        assert_eq!(Target::from(CliTarget::Ios), Target::IosSimulator);
    }

    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-cli-run-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn find_workspace_root_returns_dir_when_cargo_toml_at_start() {
        let tmp = unique_tempdir();
        std::fs::write(tmp.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        assert_eq!(find_workspace_root(&tmp).as_deref(), Some(tmp.as_path()));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn find_workspace_root_walks_up_from_a_member_dir() {
        let tmp = unique_tempdir();
        std::fs::write(tmp.join("Cargo.toml"), "[workspace]\nmembers = [\"app\"]\n").unwrap();
        let nested = tmp.join("app");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        assert_eq!(find_workspace_root(&nested).as_deref(), Some(tmp.as_path()),);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
