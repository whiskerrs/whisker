//! `whisker store push playstore`.

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use whisker_build::ui;
use whisker_dev_server::Target;
use whisker_submit::{play, playstore};

use crate::{credential, manifest};

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Show what would be written and stop, without contacting
    /// Google Play.
    #[arg(long)]
    dry_run: bool,

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
             is required to push to Google Play"
        )
    })?;
    let config = super::require(&m.crate_dir)?.playstore;
    playstore::validate(&config)?;
    let planned = playstore::describe(&config);
    if planned.is_empty() {
        bail!("store.rs declares no playstore details or listings — nothing to push");
    }

    ui::section("Store");
    ui::info(format!("pushing to Google Play as {application_id}"));
    if args.dry_run {
        for line in planned {
            ui::info(format!("would write {line}"));
        }
        return Ok(());
    }

    let account = credential::require_playstore_service_account(&m.crate_dir, &application_id)?;
    let client = play::Client::connect(&account, &application_id)?;
    for line in playstore::push(&client, &config)? {
        ui::info(line);
    }
    ui::info("committed — Google Play reviews listing changes before they go live");
    Ok(())
}
