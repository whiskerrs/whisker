//! Whisker CLI implementation.
//!
//! ## Subcommands
//!
//! - `doctor` — environment / toolchain health check (Rust targets,
//!   Android NDK/SDK/JDK, Xcode).
//! - `run` — `whisker run`: build → install → launch → file-watch +
//!   hot-patch loop. Thin wrapper around
//!   [`whisker_dev_server::DevServer`]; the cli's job is to resolve
//!   the user crate's `whisker.rs` (via [`manifest`] + [`probe`])
//!   and project the resulting `Config` into the dev-server's
//!   flat [`whisker_dev_server::Config`].
//! - `new` / `new-module` — scaffolding.
//!
//! No `build` subcommand: production builds happen through the same
//! `xcodebuild` / `gradle assembleRelease` invocations CI uses. Past
//! revisions shipped a `whisker build` convenience wrapper, but it
//! existed mostly to manage the `~/.cache/whisker/lynx/` user cache,
//! which is itself gone now (iOS uses SPM remote binary targets,
//! Android pulls aars from Maven).
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

pub mod build_dispatch;
pub mod doctor;
pub mod fmt;
pub mod linker_shim;
pub mod manifest;
pub mod new_app;
pub mod new_module;
pub mod platforms;
pub mod probe;
pub mod run;
pub mod rustc_shim;
pub mod tui;

#[derive(Parser, Debug)]
#[command(
    name = "whisker",
    about = "Whisker — cross-platform mobile UI framework",
    version
)]
struct Cli {
    /// Show every step's full underlying output (raw cargo /
    /// xcodebuild / simctl streams + the internal debug logs the
    /// curated UI hides by default). Plumbed into `whisker-build::ui`
    /// via the `WHISKER_VERBOSE` env var so subprocesses
    /// (`whisker-dev-server`, the shim binaries, etc.) inherit it.
    #[arg(long, short = 'v', global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Inspect the local toolchain — Rust targets, Android NDK/SDK/JDK,
    /// Xcode.
    Doctor(doctor::Args),
    /// Build, install, and dev-loop a Whisker app — file watch + rebuild
    /// + subsecond hot patches over WebSocket.
    Run(run::Args),
    /// Scaffold a new Whisker module crate — Cargo.toml (with the
    /// `[package.metadata.whisker]` marker), Package.swift,
    /// build.gradle.kts, and skeleton Rust / Swift / Kotlin sources.
    /// See `docs/module-author-guide.md`.
    NewModule(new_module::NewModuleArgs),
    /// Scaffold a new Whisker app — single-crate workspace with
    /// `Cargo.toml`, a `#[whisker::main]` `src/lib.rs`, the
    /// `whisker.rs` `Config` probe, `.gitignore`, and `README.md`.
    /// The result compiles standalone; run `whisker run android` or
    /// `whisker run ios` from inside the new directory.
    New(new_app::NewAppArgs),

    /// Format Rust source — a rustfmt drop-in that ALSO formats
    /// Whisker's `render!` / `css!` macro bodies (which rustfmt leaves
    /// untouched). Respects `rustfmt.toml` only; no whisker-specific
    /// config. Use `--stdin` for the rust-analyzer integration
    /// (`rustfmt.overrideCommand = ["whisker", "fmt", "--stdin"]`).
    Fmt(fmt::FmtArgs),

    /// (internal) Cross-compile the user crate into
    /// `WhiskerDriver.framework`. Invoked by the generated Xcode
    /// project's Run Script Phase, not by users.
    #[command(name = "build-ios", hide = true)]
    BuildIos(build_dispatch::IosArgs),

    /// (internal) Cross-compile the user crate into `lib*.so`. Invoked
    /// by the Whisker Gradle plugin's `cargoBuild*` task, not by users.
    #[command(name = "build-android", hide = true)]
    BuildAndroid(build_dispatch::AndroidArgs),

    /// (internal) Emit a JSON manifest of the app's Whisker modules.
    /// Invoked by the Gradle Settings plugin at init, not by users.
    #[command(name = "modules", hide = true)]
    Modules(build_dispatch::ModulesArgs),
}

pub fn run(args: impl IntoIterator<Item = String>) -> Result<()> {
    // Use clap's own exit path so `--help` / `--version` print to stdout
    // with exit code 0; bubbling the result through anyhow would prefix
    // it with "Error: " and exit non-zero.
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => e.exit(),
    };
    // `--verbose` and `WHISKER_VERBOSE=1` are the same switch. Setting
    // the env var means any subprocess we spawn (dev-server, shim
    // binaries) sees the same mode without further plumbing.
    if cli.verbose {
        std::env::set_var("WHISKER_VERBOSE", "1");
    }
    match cli.command {
        Command::Doctor(a) => doctor::run(a),
        Command::Run(a) => run::run(a),
        Command::NewModule(a) => new_module::run(a),
        Command::New(a) => new_app::run(a),
        Command::Fmt(a) => fmt::run(a),
        Command::BuildIos(a) => build_dispatch::run_ios(a),
        Command::BuildAndroid(a) => build_dispatch::run_android(a),
        Command::Modules(a) => build_dispatch::run_modules(a),
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
            }
            other => panic!("expected Doctor, got {other:?}"),
        }
    }

    #[test]
    fn parses_run_with_only_target() {
        // `target` is now required (no default), so the bare
        // `whisker run` form is gone — supply a positional target
        // and assert the rest of the args adopt their defaults.
        let cli = parse(["whisker", "run", "android"]).unwrap();
        match cli.command {
            Command::Run(a) => {
                assert!(a.manifest_path.is_none());
                assert_eq!(a.target, run::CliTarget::Android);
                assert_eq!(a.bind.port(), 9876);
                // Hot-patch is the dev default — opt out with --no-hot-patch.
                assert!(!a.no_hot_patch);
                assert!(a.workspace_root.is_none());
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn parses_run_without_target_fails() {
        // `whisker run` with no positional target is now an error
        // (Host was the previous default and has been removed).
        let res = parse(["whisker", "run"]);
        assert!(res.is_err(), "expected clap error, got {res:?}");
    }

    #[test]
    fn parses_run_with_explicit_target_and_flags() {
        // `target` moved from `--target <value>` to a positional
        // argument (`whisker run android`) — clap accepts it in any
        // position relative to the named flags, so the test mixes
        // them deliberately.
        let cli = parse([
            "whisker",
            "run",
            "--manifest-path",
            "/tmp/my-app/Cargo.toml",
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
    fn parses_fmt_stdin() {
        let cli = parse(["whisker", "fmt", "--stdin"]).unwrap();
        match cli.command {
            Command::Fmt(a) => {
                assert!(a.stdin);
                assert!(!a.check);
                assert!(a.files.is_empty());
            }
            other => panic!("expected Fmt, got {other:?}"),
        }
    }

    #[test]
    fn parses_fmt_files_and_check() {
        let cli = parse(["whisker", "fmt", "--check", "a.rs", "b.rs"]).unwrap();
        match cli.command {
            Command::Fmt(a) => {
                assert!(a.check);
                assert!(!a.stdin);
                assert_eq!(a.files.len(), 2);
            }
            other => panic!("expected Fmt, got {other:?}"),
        }
    }

    #[test]
    fn parses_doctor_skip_flags() {
        let cli = parse(["whisker", "doctor", "--no-ios", "--no-android"]).unwrap();
        match cli.command {
            Command::Doctor(a) => {
                assert!(a.no_ios);
                assert!(a.no_android);
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

    // The generated native projects (gen/ios pbxproj Run Script, the
    // Gradle plugin) call these hidden subcommands by exact name +
    // flags. If a rename/flag-change slips through, the templates break
    // silently at build time — so pin the CLI contract here.
    #[test]
    fn internal_build_subcommands_parse() {
        match parse([
            "whisker",
            "build-ios",
            "--workspace=/ws",
            "--package=app",
            "--configuration=Debug",
            "--platform=iphonesimulator",
            "--archs=arm64",
            "--built-products-dir=/out",
        ])
        .unwrap()
        .command
        {
            Command::BuildIos(_) => {}
            other => panic!("expected BuildIos, got {other:?}"),
        }

        match parse([
            "whisker",
            "build-android",
            "--workspace=/ws",
            "--package=app",
            "--profile=debug",
            "--abi=arm64-v8a",
            "--jni-libs-dir=/jni",
        ])
        .unwrap()
        .command
        {
            Command::BuildAndroid(_) => {}
            other => panic!("expected BuildAndroid, got {other:?}"),
        }

        match parse(["whisker", "modules", "--workspace=/ws", "--package=app"])
            .unwrap()
            .command
        {
            Command::Modules(_) => {}
            other => panic!("expected Modules, got {other:?}"),
        }
    }

    #[test]
    fn version_flag_short_circuits_to_displayversion() {
        let e = parse(["whisker", "--version"]).unwrap_err();
        assert_eq!(e.kind(), clap::error::ErrorKind::DisplayVersion);
    }
}
