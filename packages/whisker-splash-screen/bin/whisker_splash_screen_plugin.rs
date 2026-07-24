//! `whisker-splash-screen-plugin` subprocess plugin binary.
//!
//! The Whisker engine discovers this via the
//! `[package.metadata.whisker.plugins.whisker-splash-screen]` table in
//! this crate's `Cargo.toml`, builds it, and spawns it with a
//! `PluginRequest` JSON on stdin / `PluginResponse` JSON on stdout. The
//! plugin logic lives in [`whisker_splash_screen::WhiskerSplashScreen`].

fn main() -> anyhow::Result<()> {
    whisker_plugin::run_as_subprocess(whisker_splash_screen::WhiskerSplashScreen)
}
