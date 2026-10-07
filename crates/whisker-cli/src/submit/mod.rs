//! `whisker submit` — upload an already-built release artifact to
//! its store.
//!
//! Responsibility boundary: submit never builds. It takes the
//! artifact the last `whisker build ipa` / `appbundle` left on disk
//! (or `--path`) and talks to the store's API directly — no
//! Transporter, altool, or fastlane. Credentials come from the same
//! `credentials/` store builds use; like build, submit only consumes
//! them.

mod android;
mod ios;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct SubmitArgs {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Upload the `.ipa` to App Store Connect (TestFlight / App
    /// Store) through the Build Upload API, authenticated with the
    /// stored App Store Connect API key.
    Ios(ios::Args),
    /// Upload the `.aab` to Google Play and release it on a track,
    /// authenticated with the stored service-account key.
    Android(android::Args),
}

pub fn run(args: SubmitArgs) -> Result<()> {
    match args.cmd {
        Cmd::Ios(a) => ios::run(a),
        Cmd::Android(a) => android::run(a),
    }
}

/// `--path` if given, else the last build's artifact.
fn artifact(
    explicit: Option<PathBuf>,
    last_build: Option<PathBuf>,
    build_command: &str,
) -> Result<PathBuf> {
    match explicit {
        Some(path) if path.is_file() => Ok(path),
        Some(path) => bail!("{} is not a file", path.display()),
        None => match last_build.filter(|p| p.is_file()) {
            Some(path) => Ok(path),
            None => bail!("nothing to submit — run `{build_command}` first, or pass --path"),
        },
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}
