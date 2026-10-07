//! `whisker submit android` — upload the `.aab` to Google Play.
//!
//! One Play "edit" (a transaction) carries the whole submit: upload
//! the bundle, put its versionCode on the track, commit. Nothing is
//! visible in Play Console until the commit succeeds.

use anyhow::{Result, anyhow};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use whisker_build::ui;
use whisker_dev_server::Target;

use crate::credential::{self, google};
use crate::manifest;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// The `.aab` to upload. Defaults to the one the last
    /// `whisker build appbundle` produced.
    #[arg(long, value_name = "AAB")]
    path: Option<PathBuf>,

    /// Play track to release on: `internal`, `alpha` (closed
    /// testing), `beta` (open testing), `production`, or a custom
    /// closed-testing track name.
    #[arg(long, default_value = "internal")]
    track: String,

    /// Create the release as a draft to roll out from Play Console.
    /// Required until the app has been published once.
    #[arg(long)]
    draft: bool,

    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    let m = manifest::resolve_for_target(args.manifest_path.as_deref(), Target::Android)?;
    let application_id = manifest::android_application_id(&m.config).ok_or_else(|| {
        anyhow!(
            "whisker.rs: app.android(|a| a.application_id(\"…\")) (or app.bundle_id) \
             is required for Android submits"
        )
    })?;
    let aab = super::artifact(
        args.path,
        Some(whisker_build::android::release_bundle_path(
            &m.project(Target::Android)?.gen_dir,
        )),
        "whisker build appbundle",
    )?;
    let account = credential::require_playstore_service_account(&m.crate_dir, &application_id)?;

    ui::section("Submit");
    ui::info(format!(
        "submitting {} ({}) as {application_id} — Play track `{}`",
        aab.display(),
        super::megabytes(aab.metadata()?.len()),
        args.track,
    ));

    let client = google::Client::connect(&account, &application_id)?;
    let edit = client.insert_edit()?;
    let step = ui::step(ui::OperationKind::Upload, "app bundle");
    let version_code = match client.upload_bundle(&edit, &aab) {
        Ok(code) => {
            step.done(format!("versionCode {code}"));
            code
        }
        Err(e) => {
            step.fail("");
            return Err(e);
        }
    };
    let status = if args.draft { "draft" } else { "completed" };
    client.set_track_release(&edit, &args.track, version_code, status)?;
    client.commit_edit(&edit)?;

    if args.draft {
        ui::info(format!(
            "versionCode {version_code} is a draft on `{}` — roll it out from Play Console",
            args.track
        ));
    } else {
        ui::info(format!(
            "versionCode {version_code} released on `{}`",
            args.track
        ));
    }
    Ok(())
}
