//! `whisker credential playstore` — acquire and store the Google Cloud
//! service-account key `whisker submit android` uploads with.
//!
//! Play has no console-issued API key: the Developer API only
//! accepts Google Cloud credentials, so the wizard walks through
//! creating a service account in Cloud Console and inviting it in
//! Play Console, then validates the downloaded key before storing it.

use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use std::path::{Path, PathBuf};
use whisker_credentials::{PlaystoreServiceAccount, Store, android_playstore_rel};

use super::{google, prompt};
use crate::manifest;

const PLAY_CONSOLE_URL: &str = "https://play.google.com/console/developers";

#[derive(Args, Debug)]
pub struct PlaystoreArgs {
    /// Store the key under a specific applicationId instead of the
    /// account-wide `default` entry. Only needed when some apps ship
    /// under a DIFFERENT Play developer account.
    #[arg(long, value_name = "APPLICATION_ID")]
    id: Option<String>,

    /// Use an already-downloaded service-account JSON key, skipping
    /// the Cloud Console steps.
    #[arg(long, value_name = "PATH")]
    import: Option<PathBuf>,

    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: PlaystoreArgs) -> Result<()> {
    if !prompt::is_interactive() {
        bail!(
            "`whisker credential playstore` is an interactive wizard — run it locally, commit\n\
             the credentials/ directory, and give CI only $WHISKER_CREDENTIALS_KEY"
        );
    }
    let m = manifest::resolve_for_target(
        args.manifest_path.as_deref(),
        whisker_dev_server::Target::Android,
    )?;
    let store = super::open_or_bootstrap(&m.crate_dir)?;

    let rel = android_playstore_rel(args.id.as_deref());
    if store.has(&rel)
        && !prompt::confirm("A Play service-account key is already stored. Replace it?")?
    {
        bail!("aborted — existing key kept.");
    }
    let application_id = args
        .id
        .clone()
        .or_else(|| manifest::android_application_id(&m.config));
    acquire_and_store(
        &store,
        &rel,
        application_id.as_deref(),
        args.import.as_deref(),
    )
}

/// The wizard body. Also the inline pre-step `whisker submit android`
/// runs when no key is stored yet (interactive sessions only).
pub(crate) fn acquire_and_store(
    store: &Store,
    rel: &str,
    application_id: Option<&str>,
    import: Option<&Path>,
) -> Result<()> {
    let key_path = match import {
        Some(path) => path.to_path_buf(),
        None => guide_key_creation()?,
    };
    let key_json =
        std::fs::read(&key_path).with_context(|| format!("read {}", key_path.display()))?;
    let account: PlaystoreServiceAccount = serde_json::from_slice(&key_json).map_err(|e| {
        anyhow!(
            "{} is not a service-account JSON key ({e}) — in Cloud Console, open the \
             service account → Keys → Add key → JSON",
            key_path.display()
        )
    })?;

    println!();
    println!("④ Invite the service account in Play Console:");
    println!("     {PLAY_CONSOLE_URL}");
    println!("   - Users and permissions → Invite new users");
    println!("   - Email address: {}", account.client_email);
    println!("   - App permissions → add your app → tick the **Releases** permissions");
    println!("   - Invite user");
    prompt::line("Press Enter to open Play Console in your browser…")?;
    prompt::open_in_browser(PLAY_CONSOLE_URL);
    prompt::line("Press Enter once the invite is sent…")?;

    println!("   Validating against Google…");
    let package = application_id.unwrap_or_default();
    let client = google::Client::connect(&account, package)?;
    println!("   ✓ key works — {}", account.client_email);
    // Access is checked only when an applicationId is known, and only
    // as a warning: a fresh invite takes a while to apply, and the app
    // may not exist in Play Console yet.
    if application_id.is_some() {
        match client.insert_edit() {
            Ok(edit) => {
                client.delete_edit(&edit).ok();
                println!("   ✓ can release {package}");
            }
            Err(e) => println!("   ! couldn't open a release for {package} yet:\n     {e:#}"),
        }
    }

    store.put(rel, &key_json)?;
    println!("  ✓ credentials/{rel}.age");
    println!();
    println!("Commit the credentials/ directory. `whisker submit android` will upload with");
    println!("this service account.");
    println!(
        "Tip: you can now delete {} — whisker keeps its own encrypted copy.",
        key_path.display()
    );
    Ok(())
}

fn guide_key_creation() -> Result<PathBuf> {
    println!("Create a Google Cloud service account for uploads (free, one-time):");
    println!("① Pick or create a Cloud project to hold it:");
    println!("     https://console.cloud.google.com/projectcreate");
    println!("② Enable the Google Play Android Developer API in that project:");
    println!("     https://console.cloud.google.com/apis/library/androidpublisher.googleapis.com");
    println!("③ Create a service account (no roles needed), then Keys → Add key → JSON:");
    println!("     https://console.cloud.google.com/iam-admin/serviceaccounts");
    prompt::file_path("Path to the downloaded JSON key (drag & drop works):")
}
