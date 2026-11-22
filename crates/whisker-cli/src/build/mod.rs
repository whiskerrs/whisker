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
mod desktop;
mod ios;
mod macos;
mod web;

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
    /// Native macOS `.app` bundle built from the CNG-generated Cargo project.
    Macos(macos::Args),
    /// Windows executable and resources (unpackaged, unsigned).
    Windows(desktop::Args),
    /// Linux installation tree (bin/ and share/).
    Linux(desktop::Args),
    /// Static browser bundle (`index.html`, JavaScript glue, and WebAssembly).
    Web(web::Args),
}

pub fn run(args: BuildArgs, no_tui: bool) -> Result<()> {
    match args.cmd {
        Cmd::Appbundle(a) => android::run(ReleaseArtifact::AppBundle, a, no_tui),
        Cmd::Apk(a) => android::run(ReleaseArtifact::Apk, a, no_tui),
        Cmd::Ipa(a) => ios::run(a, no_tui),
        Cmd::Windows(a) => desktop::run(a, no_tui, whisker_cng::GenerationTarget::Windows),
        Cmd::Linux(a) => desktop::run(a, no_tui, whisker_cng::GenerationTarget::Linux),
        Cmd::Macos(a) => macos::run(a, no_tui),
        Cmd::Web(a) => web::run(a, no_tui),
    }
}

pub(super) struct BuildUi {
    session: Option<crate::tui::TuiSession>,
    crate_dir: std::path::PathBuf,
    started_at: std::time::Instant,
}

impl BuildUi {
    /// Start the build UI and announce what is being built. `identity` is
    /// the bundle identifier or name the platform's tools build under.
    pub fn start(
        no_tui: bool,
        manifest: &crate::manifest::ResolvedManifest,
        platform: whisker_cng::GenerationTarget,
        identity: &str,
        flavor: &str,
    ) -> Self {
        let label = crate::platforms::label(platform);
        let session = if crate::tui::should_start(no_tui) {
            match crate::tui::TuiSession::start(crate::tui::WorkflowKind::Build, label, identity) {
                Ok(session) => Some(session),
                Err(error) => {
                    eprintln!("couldn't start TUI ({error:#}); falling back to plain output");
                    None
                }
            }
        } else {
            None
        };
        manifest.announce(platform, identity, label, flavor);
        whisker_build::ui::section("Build");
        Self {
            session,
            crate_dir: manifest.crate_dir.clone(),
            started_at: std::time::Instant::now(),
        }
    }

    /// Report the artifact; hints the caller prints afterwards land in the
    /// same `Output` section.
    pub fn complete(&self, artifact: &std::path::Path) {
        let shown = artifact
            .strip_prefix(&self.crate_dir)
            .unwrap_or(artifact)
            .display()
            .to_string();
        if let Some(session) = self.session.as_ref() {
            let handle = session.handle();
            handle.set_artifact(shown.clone());
            handle.set_phase(crate::tui::AppPhase::Completed);
        }
        use whisker_build::ui;
        ui::section("Output");
        ui::info(ui::labeled("artifact", shown));
        ui::info(ui::labeled(
            "built",
            ui::format_elapsed(self.started_at.elapsed()),
        ));
    }
}
