//! `whisker-status-bar-plugin` subprocess plugin binary.
//!
//! The Whisker engine discovers this via the
//! `[package.metadata.whisker.plugins.whisker-status-bar]` table in
//! this crate's `Cargo.toml`, builds it, and spawns it with a
//! `PluginRequest` JSON on stdin / `PluginResponse` JSON on stdout. The
//! plugin logic lives in [`whisker_status_bar::WhiskerStatusBar`]; this
//! file is just the wrapper.

fn main() -> anyhow::Result<()> {
    whisker_plugin::run_as_subprocess(whisker_status_bar::WhiskerStatusBar)
}
