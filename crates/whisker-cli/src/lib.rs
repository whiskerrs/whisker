//! Whisker CLI implementation.
//!
//! ## Subcommands
//!
//! - `doctor` — environment / toolchain health check
//!   (Rust targets, Android NDK/SDK/JDK, Xcode + CocoaPods, the
//!   Lynx artifacts under `target/`).
//! - `run` — `whisker run`: build → install → launch → file-watch +
//!   hot-patch loop. Thin wrapper around
//!   [`whisker_dev_server::DevServer`]; the cli's job is to resolve
//!   the user crate's `whisker.rs` (via [`manifest`] + [`probe`])
//!   and project the resulting `AppConfig` into the dev-server's
//!   flat [`whisker_dev_server::Config`].
//!
//! ## Internal binaries
//!
//! In addition to the user-facing `whisker` binary, the package also
//! produces two shim binaries used during the initial fat build to
//! capture the rustc + linker invocations that Tier 1 hot-patch will
//! replay later:
//!
//! - `whisker-rustc-shim` (`-Cstrip=…` / `-Csave-temps=y` style
//!   wrapper around rustc) — captures argv to
//!   `$WHISKER_RUSTC_CACHE_DIR/<crate>-<timestamp>.json`.
//! - `whisker-linker-shim` (forwarded by rustc's `-C linker=…`) —
//!   captures argv to `$WHISKER_LINKER_CACHE_DIR/<output>-…json`.

use anyhow::Result;
use clap::{Parser, Subcommand};

pub mod build;
pub mod build_android;
pub mod build_ios;
pub mod doctor;
pub mod linker_shim;
pub mod manifest;
pub mod native;
pub mod probe;
pub mod run;
pub mod rustc_shim;

#[derive(Parser, Debug)]
#[command(
    name = "whisker",
    about = "Whisker — cross-platform mobile UI framework",
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
    /// Build, install, and dev-loop a Whisker app — file watch + rebuild
    /// + subsecond hot patches over WebSocket.
    Run(run::Args),
    /// Production build of a Whisker app — release-mode cargo build +
    /// gradle / xcodebuild without the dev-server. Output is the
    /// shippable `.apk` / `.app`.
    Build(build::Args),
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
        Command::Run(a) => run::run(a),
        Command::Build(a) => build::run(a),
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
        let cli = parse(["whisker", "doctor"]).unwrap();
        match cli.command {
            Command::Doctor(a) => {
                assert!(!a.no_ios);
                assert!(!a.no_android);
                assert!(!a.no_lynx);
            }
            other => panic!("expected Doctor, got {other:?}"),
        }
    }

    #[test]
    fn parses_run_with_defaults() {
        let cli = parse(["whisker", "run"]).unwrap();
        match cli.command {
            Command::Run(a) => {
                assert!(a.manifest_path.is_none());
                assert_eq!(a.target, run::CliTarget::Host);
                assert_eq!(a.bind.port(), 9876);
                // Hot-patch is the dev default — opt out with --no-hot-patch.
                assert!(!a.no_hot_patch);
                assert!(a.workspace_root.is_none());
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn parses_run_with_explicit_target_and_flags() {
        let cli = parse([
            "whisker",
            "run",
            "--manifest-path",
            "/tmp/my-app/Cargo.toml",
            "--target",
            "android",
            "--bind",
            "0.0.0.0:1234",
            "--no-hot-patch",
        ])
        .unwrap();
        match cli.command {
            Command::Run(a) => {
                assert_eq!(
                    a.manifest_path.as_deref(),
                    Some(std::path::Path::new("/tmp/my-app/Cargo.toml")),
                );
                assert_eq!(a.target, run::CliTarget::Android);
                assert_eq!(a.bind.to_string(), "0.0.0.0:1234");
                assert!(a.no_hot_patch);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn parses_doctor_skip_flags() {
        let cli = parse(["whisker", "doctor", "--no-ios", "--no-lynx"]).unwrap();
        match cli.command {
            Command::Doctor(a) => {
                assert!(a.no_ios);
                assert!(!a.no_android);
                assert!(a.no_lynx);
            }
            other => panic!("expected Doctor, got {other:?}"),
        }
    }

    #[test]
    fn missing_subcommand_is_an_error() {
        // Clap renders help when no subcommand is given (we haven't
        // marked any as default), so the error kind here is the
        // help-on-missing-arg variant rather than `MissingSubcommand`.
        let e = parse(["whisker"]).unwrap_err();
        assert_eq!(
            e.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand,
        );
    }

    #[test]
    fn unknown_subcommand_is_an_error() {
        let e = parse(["whisker", "frobnicate"]).unwrap_err();
        assert_eq!(e.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn help_flag_short_circuits_to_displayhelp() {
        let e = parse(["whisker", "--help"]).unwrap_err();
        assert_eq!(e.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn version_flag_short_circuits_to_displayversion() {
        let e = parse(["whisker", "--version"]).unwrap_err();
        assert_eq!(e.kind(), clap::error::ErrorKind::DisplayVersion);
    }
}
