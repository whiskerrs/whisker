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

#[derive(Parser, Debug)]
#[command(
    name = "tuft",
    about = "Tuft — cross-platform mobile UI framework",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse<I, S>(args: I) -> std::result::Result<Cli, clap::Error>
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString> + Clone,
    {
        Cli::try_parse_from(args)
    }

    #[test]
    fn parses_doctor_with_no_flags() {
        let cli = parse(["tuft", "doctor"]).unwrap();
        match cli.command {
            Command::Doctor(a) => {
                assert!(!a.no_ios);
                assert!(!a.no_android);
                assert!(!a.no_lynx);
            }
        }
    }

    #[test]
    fn parses_doctor_skip_flags() {
        let cli = parse(["tuft", "doctor", "--no-ios", "--no-lynx"]).unwrap();
        match cli.command {
            Command::Doctor(a) => {
                assert!(a.no_ios);
                assert!(!a.no_android);
                assert!(a.no_lynx);
            }
        }
    }

    #[test]
    fn missing_subcommand_is_an_error() {
        // Clap renders help when no subcommand is given (we haven't
        // marked any as default), so the error kind here is the
        // help-on-missing-arg variant rather than `MissingSubcommand`.
        let e = parse(["tuft"]).unwrap_err();
        assert_eq!(
            e.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand,
        );
    }

    #[test]
    fn unknown_subcommand_is_an_error() {
        let e = parse(["tuft", "frobnicate"]).unwrap_err();
        assert_eq!(e.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn help_flag_short_circuits_to_displayhelp() {
        let e = parse(["tuft", "--help"]).unwrap_err();
        assert_eq!(e.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn version_flag_short_circuits_to_displayversion() {
        let e = parse(["tuft", "--version"]).unwrap_err();
        assert_eq!(e.kind(), clap::error::ErrorKind::DisplayVersion);
    }
}
