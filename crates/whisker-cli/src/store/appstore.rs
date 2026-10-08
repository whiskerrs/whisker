//! `whisker store push appstore`.

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use whisker_build::ui;
use whisker_dev_server::Target;
use whisker_submit::{appstore, asc};

use crate::{credential, manifest};

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Show what would be written and stop, without contacting App
    /// Store Connect.
    #[arg(long)]
    dry_run: bool,

    /// When no version is being prepared, create one from
    /// whisker.rs's `version` instead of failing.
    #[arg(long)]
    create_version: bool,

    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    let m = manifest::resolve_for_target(args.manifest_path.as_deref(), Target::IosSimulator)?;
    // Same resolution `whisker build ipa` uses.
    let bundle_id = m
        .config
        .ios
        .bundle_id
        .clone()
        .or_else(|| m.config.bundle_id.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.bundle_id(\"…\")) or app.bundle_id(\"…\") \
                 is required to push to App Store Connect"
            )
        })?;
    let create_version = match (args.create_version, &m.config.version) {
        (true, Some(version)) => Some(version.as_str()),
        (true, None) => bail!("--create-version needs app.version(\"…\") in whisker.rs"),
        (false, _) => None,
    };
    let config = super::require(&m.crate_dir)?.appstore;
    appstore::validate(&config)?;
    let planned = appstore::describe(&config);
    if planned.is_empty() {
        bail!("store.rs declares nothing `store push appstore` sends");
    }

    ui::section("Store");
    ui::info(format!("pushing to App Store Connect as {bundle_id}"));
    if args.dry_run {
        for line in planned {
            ui::info(format!("would write {line}"));
        }
        return Ok(());
    }

    let key = credential::require_asc_key(&m.crate_dir, &bundle_id)?;
    let auth = asc::KeyAuth {
        p8_pem: &key.p8_pem,
        key_id: &key.key_id,
        issuer_id: &key.issuer_id,
    };
    let options = appstore::PushOptions { create_version };
    for line in appstore::push(&auth, &bundle_id, &config, &options)? {
        ui::info(line);
    }
    Ok(())
}
