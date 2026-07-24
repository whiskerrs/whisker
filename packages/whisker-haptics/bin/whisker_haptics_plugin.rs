//! `whisker-haptics-plugin` subprocess plugin binary.
//!
//! The Whisker engine discovers this binary via the
//! `[package.metadata.whisker.plugins.whisker-haptics]` table in
//! this crate's `Cargo.toml`, builds it via `cargo build --bin
//! whisker-haptics-plugin`, and spawns it with a `PluginRequest`
//! JSON on stdin / `PluginResponse` JSON on stdout. The plugin logic
//! lives in [`whisker_haptics::WhiskerHaptics`]; this file is just
//! the wrapper.

fn main() -> anyhow::Result<()> {
    whisker_plugin::run_as_subprocess(whisker_haptics::WhiskerHaptics)
}
