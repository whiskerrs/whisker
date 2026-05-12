//! `cargo xtask android …` subtree — cross-compile + packaging pipeline.

use clap::{Args, Subcommand};

mod cargo_build;
mod ndk;

#[derive(Args)]
pub struct AndroidArgs {
    #[command(subcommand)]
    command: AndroidCommand,
}

#[derive(Subcommand)]
enum AndroidCommand {
    /// Build a Rust crate for an Android ABI (drop-in replacement for
    /// `cargo ndk … build`). Sets up the NDK clang as linker and CC/CXX
    /// for cc-rs, then invokes plain `cargo build`. Pass extra args to
    /// cargo after a literal `--`.
    Cargo(cargo_build::CargoBuildArgs),
}

pub fn run(args: AndroidArgs) -> anyhow::Result<()> {
    match args.command {
        AndroidCommand::Cargo(a) => cargo_build::run(a),
    }
}
