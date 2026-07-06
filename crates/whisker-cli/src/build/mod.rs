//! `whisker build` — produce distributable, signed artifacts.
//!
//! Responsibility boundary: build resolves the config, syncs `gen/`,
//! and drives gradle/xcodebuild. Everything credential-shaped
//! (creating keystores, acquiring keys, decrypting) is delegated to
//! the `credential` module's pre-step — build only *consumes*
//! staged signing inputs and never writes to `credentials/`.
//!
//! Sequencing rule: the credential pre-step (which may prompt for
//! the decryption key) runs BEFORE any compilation, so the one
//! interactive moment happens up front and the long build can be
//! left unattended.

mod android;
mod ios;

use anyhow::Result;
use clap::{Args, Subcommand};
use whisker_build::android::ReleaseArtifact;

#[derive(Args, Debug)]
pub struct BuildArgs {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Android App Bundle (.aab), release-signed from the
    /// credentials/ store — the format Play Console accepts.
    Appbundle(android::Args),
    /// Release-signed APK for direct distribution (Firebase App
    /// Distribution, internal testing, sideloading).
    Apk(android::Args),
    /// iOS .ipa — `xcodebuild archive` + export, signed via the App
    /// Store Connect API key with Apple's cloud-managed distribution
    /// certificate. `--method ad-hoc` for device-limited distribution.
    Ipa(ios::Args),
}

pub fn run(args: BuildArgs) -> Result<()> {
    match args.cmd {
        Cmd::Appbundle(a) => android::run(ReleaseArtifact::AppBundle, a),
        Cmd::Apk(a) => android::run(ReleaseArtifact::Apk, a),
        Cmd::Ipa(a) => ios::run(a),
    }
}
