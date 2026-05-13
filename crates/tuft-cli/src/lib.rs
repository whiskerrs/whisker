//! Tuft CLI implementation.
//!
//! Subcommands (current + planned):
//! - `doctor`   — environment / toolchain health check
//! - `new`      — scaffold a new Tuft app (planned)
//! - `run`      — build → install → launch on emulator/sim (planned)
//! - `dev`      — file-watch dev loop (planned)
//! - `build`    — production build (planned)
//! - `prebuild` — CNG codegen (planned)
//! - `clean`    — wipe build artifacts (planned)
//! - `plugin`   — manage plugins (planned)

use anyhow::Result;
use clap::{Parser, Subcommand};

pub mod doctor;

#[derive(Parser)]
#[command(
    name = "tuft",
    about = "Tuft — cross-platform mobile UI framework",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect the local toolchain — Rust targets, Android NDK/SDK/JDK,
    /// Xcode + CocoaPods, and the Lynx artifacts under `target/`.
    Doctor(doctor::Args),
}

pub fn run(args: impl IntoIterator<Item = String>) -> Result<()> {
    // Use clap's own exit path so `--help` / `--version` print to stdout
    // with exit code 0; bubbling the result through anyhow would prefix
    // it with "Error: " and exit non-zero.
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => e.exit(),
    };
    match cli.command {
        Command::Doctor(a) => doctor::run(a),
    }
}
