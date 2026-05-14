//! `tuft run` — start the dev server.
//!
//! Thin wrapper: parses the CLI flags into a [`tuft_dev_server::Config`]
//! and calls [`tuft_dev_server::DevServer::run`]. All the heavy lifting
//! (file watch / cargo build / WebSocket push / subsecond patches)
//! lives in `tuft-dev-server` so other host shells (an editor plugin,
//! a notebook front-end, …) can reuse the same loop.

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;
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
        None => std::env::current_dir().context("CWD")?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_target_maps_to_dev_server_target() {
        assert_eq!(Target::from(CliTarget::Host), Target::Host);
        assert_eq!(Target::from(CliTarget::Android), Target::Android);
        assert_eq!(Target::from(CliTarget::Ios), Target::IosSimulator);
    }
}
