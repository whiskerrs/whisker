//! `tuft run` — start the dev server.
//!
//! Thin wrapper: parses the CLI flags into a [`tuft_dev_server::Config`]
//! and calls [`tuft_dev_server::DevServer::run`]. All the heavy lifting
//! (file watch / cargo build / WebSocket push / subsecond patches)
//! lives in `tuft-dev-server` so other host shells (an editor plugin,
//! a notebook front-end, …) can reuse the same loop.

use anyhow::{anyhow, Context, Result};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tuft_dev_server::{Config, DevServer, HotPatchMode, Target};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// User-crate package to build and run. The package's source dir
    /// is also what the file watcher watches.
    #[arg(short = 'p', long, default_value = "hello-world")]
    pub package: String,

    /// Where to deploy the rebuilt artifact.
    #[arg(long, value_enum, default_value_t = CliTarget::Host)]
    pub target: CliTarget,

    /// WebSocket bind address. The Tuft app on the device dials this
    /// (via `TUFT_DEV_ADDR`) to receive patches.
    #[arg(long, default_value = "127.0.0.1:9876")]
    pub bind: SocketAddr,

    /// Enable Tier 1 subsecond hot-patching (defaults to Tier 2 cold
    /// rebuild). Tier 1 is wired up in I4g; until then this flag
    /// behaves like Tier 2 with a warning.
    #[arg(long)]
    pub hot_patch: bool,

    /// Override the workspace root. Defaults to the current dir.
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
    let workspace_root = match args.workspace_root {
        Some(p) => p,
        None => {
            let cwd = std::env::current_dir().context("CWD")?;
            find_workspace_root(&cwd).ok_or_else(|| {
                anyhow!(
                    "could not find a Cargo workspace at or above {} \
                     (pass --workspace-root to override)",
                    cwd.display(),
                )
            })?
        }
    };

    let mut config = Config::defaults_for(workspace_root, args.package, args.target.into());
    config.bind_addr = args.bind;
    config.hot_patch_mode = if args.hot_patch {
        HotPatchMode::Tier1Subsecond
    } else {
        HotPatchMode::Tier2ColdRebuild
    };

    // Multi-thread runtime: the WebSocket server, file watcher, and
    // cargo build subprocess all want to make progress concurrently.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    rt.block_on(DevServer::new(config)?.run())
}

/// Walk up from `start` looking for a `Cargo.toml` containing a
/// `[workspace]` section. Returns the directory holding the matching
/// Cargo.toml, or `None` if we walk off the top of the filesystem
/// without finding one. Pure: no env / no CWD reads, so unit-testable.
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

    /// Per-test scratch dir so concurrent tests don't tread on each
    /// other. The test suite is too small to justify the `tempfile`
    /// crate as a dev-dep — same pattern as in doctor.rs.
    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("tuft-cli-run-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn find_workspace_root_returns_dir_when_cargo_toml_at_start() {
        let tmp = unique_tempdir();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = []\n",
        )
        .unwrap();
        assert_eq!(find_workspace_root(&tmp).as_deref(), Some(tmp.as_path()));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn find_workspace_root_walks_up_from_a_member_dir() {
        let tmp = unique_tempdir();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"examples/hello-world\"]\n",
        )
        .unwrap();
        let nested = tmp.join("examples/hello-world");
        std::fs::create_dir_all(&nested).unwrap();
        // Member's own Cargo.toml exists but doesn't have [workspace]
        // — walker must keep going up.
        std::fs::write(
            nested.join("Cargo.toml"),
            "[package]\nname = \"hello-world\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        assert_eq!(
            find_workspace_root(&nested).as_deref(),
            Some(tmp.as_path()),
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn find_workspace_root_returns_none_outside_any_workspace() {
        let tmp = unique_tempdir();
        // No Cargo.toml at all.
        assert_eq!(find_workspace_root(&tmp), None);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
