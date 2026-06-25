//! `whisker-router-plugin` subprocess plugin binary.
//!
//! The Whisker engine discovers this binary via the
//! `[package.metadata.whisker.plugins.whisker-router]` table in this
//! crate's `Cargo.toml`, builds it (`cargo build --bin
//! whisker-router-plugin`), and spawns it with a `PluginRequest` JSON on
//! stdin / `PluginResponse` JSON on stdout. The plugin logic lives in
//! [`whisker_router::RouterPlugin`]; this file is just the wrapper.

fn main() -> anyhow::Result<()> {
    whisker_plugin::run_as_subprocess(whisker_router::RouterPlugin)
}
