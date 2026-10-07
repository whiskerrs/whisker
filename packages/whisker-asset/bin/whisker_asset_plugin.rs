//! `whisker-asset-plugin` subprocess plugin binary.
//!
//! The Whisker engine discovers this binary via the
//! `[package.metadata.whisker.plugins.whisker-asset]` table in this
//! crate's `Cargo.toml`, builds it via `cargo build --bin
//! whisker-asset-plugin`, and spawns it with versioned project JSON
//! requests on stdin and responses on stdout. The plugin logic lives in
//! [`whisker_asset::WhiskerAsset`]; this file is just the wrapper.

fn main() -> anyhow::Result<()> {
    whisker_plugin::project::protocol::run_as_subprocess(whisker_asset::WhiskerAsset)
}
