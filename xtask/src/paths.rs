//! Cross-cutting path helpers reused by every subcommand.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Absolute path to the Cargo workspace root.
///
/// `CARGO_MANIFEST_DIR` is resolved at *compile time* against the
/// `xtask/` crate, so the workspace root is its parent.
pub fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .map(|p| p.to_path_buf())
        .context("xtask manifest dir has no parent — broken workspace layout?")
}

/// `$HOME` resolved at runtime. Returns an error rather than panicking
/// if it isn't set (which would be unusual on a dev machine but
/// possible in stripped-down containers).
pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set in the environment")
}
