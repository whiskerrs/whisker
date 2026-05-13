//! Build automation for Lyra (`cargo xtask` pattern).
//!
//! `xtask` is a workspace member that we register as the
//! `cargo xtask` alias in `.cargo/config.toml`. It exists to host build
//! orchestration that doesn't fit cargo natively — Android NDK glue,
//! iOS xcframework packaging, Lynx AAR build, etc. — without pulling
//! in external tools (Make, just, cargo-ndk, …) or shelling into
//! brittle bash.
//!
//! Each subcommand corresponds to one composable build step. Higher-
//! level orchestration (e.g. "build the hello-world example end to
//! end") is itself a subcommand that calls the lower-level ones.

use clap::{Parser, Subcommand};

mod android;
mod ios;

#[derive(Parser)]
#[command(name = "xtask", about = "Lyra build automation", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Android build pipeline (cargo cross-compile, AAR packaging, etc.).
    Android(android::AndroidArgs),
    /// iOS build pipeline (xcframework + Lynx framework assembly).
    Ios(ios::IosArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Android(args) => android::run(args),
        Command::Ios(args) => ios::run(args),
    }
}
