//! `whisker store push appstore` and `whisker store pull appstore`.

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use whisker_build::ui;
use whisker_dev_server::Target;
use whisker_submit::{appstore, asc, render};

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

// Same resolution `whisker build ipa` uses.
fn bundle_id(m: &manifest::ResolvedManifest) -> Result<String> {
    m.config
        .ios
        .bundle_id
        .clone()
        .or_else(|| m.config.bundle_id.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.bundle_id(\"…\")) or app.bundle_id(\"…\") \
                 is required to reach App Store Connect"
            )
        })
}

#[derive(ClapArgs, Debug)]
pub struct PullArgs {
    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn pull(args: PullArgs) -> Result<()> {
    let m = manifest::resolve_for_target(args.manifest_path.as_deref(), Target::IosSimulator)?;
    let bundle_id = bundle_id(&m)?;
    let key = credential::require_asc_key(&m.crate_dir, &bundle_id)?;
    let auth = asc::KeyAuth {
        p8_pem: &key.p8_pem,
        key_id: &key.key_id,
        issuer_id: &key.issuer_id,
    };
    let (config, notes) = appstore::pull(&auth, &bundle_id)?;
    ui::info(format!("read {bundle_id} from App Store Connect"));
    for note in notes {
        ui::info(note);
    }
    print!("{}", render::appstore(&config));
    Ok(())
}

pub fn run(args: Args) -> Result<()> {
    let m = manifest::resolve_for_target(args.manifest_path.as_deref(), Target::IosSimulator)?;
    let bundle_id = bundle_id(&m)?;
    let create_version = match (args.create_version, &m.config.version) {
        (true, Some(version)) => Some(version.as_str()),
        (true, None) => bail!("--create-version needs app.version(\"…\") in whisker.rs"),
        (false, _) => None,
    };
    let config = super::require(&m.crate_dir)?.appstore;
    appstore::validate(&config, &m.crate_dir)?;
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
    for line in appstore::push(&auth, &bundle_id, &config, &m.crate_dir, &options)? {
        ui::info(line);
    }
    Ok(())
}
