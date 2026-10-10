//! `whisker submit ios` — upload the `.ipa` through App Store
//! Connect's Build Upload API: declare the build, reserve the file,
//! PUT its bytes where Apple says, commit, then wait for Apple's
//! processing verdict.
//!
//! The bundle id and version numbers sent to Apple are read out of
//! the ipa itself, so they can't drift from what is actually being
//! uploaded.

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use whisker_build::ui;
use whisker_dev_server::Target;
use whisker_submit::{asc, ipa};

use crate::credential;
use crate::manifest;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Processing normally takes a few minutes; past this the upload is
/// still valid, we just stop watching it.
const PROCESSING_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// The `.ipa` to upload. Defaults to the one the last
    /// `whisker build ipa` produced.
    #[arg(long, value_name = "IPA")]
    path: Option<PathBuf>,

    /// Return as soon as the upload is committed instead of waiting
    /// for App Store Connect to finish processing the build.
    #[arg(long)]
    no_wait: bool,

    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    let m = manifest::resolve_for_target(args.manifest_path.as_deref(), Target::IosSimulator)?;
    let ipa_path = super::artifact(
        args.path,
        whisker_build::ios::exported_ipa(&m.workspace_root, &m.package),
        "whisker build ipa",
    )?;
    let info = ipa::inspect(&ipa_path)?;
    if info.device_limited {
        bail!(
            "{} is signed for registered devices only (ad-hoc or development) and can't be \
             uploaded — rebuild with `whisker build ipa --method app-store-connect`",
            ipa_path.display()
        );
    }
    let store = crate::store::load(&m.crate_dir)?;
    let notes = super::release_notes(
        store
            .iter()
            .flat_map(|s| &s.appstore.beta_build.localizations)
            .map(|l| (l.locale.as_str(), l.whats_new.as_deref())),
        "TestFlight",
        whisker_submit::appstore::BETA_WHATS_NEW_LIMIT,
    )?;
    let key = credential::require_asc_key(&m.crate_dir, &info.bundle_id)?;
    let auth = asc::KeyAuth {
        p8_pem: &key.p8_pem,
        key_id: &key.key_id,
        issuer_id: &key.issuer_id,
    };

    let size = ipa_path.metadata()?.len();
    ui::section("Submit");
    ui::info(format!(
        "submitting {} {} ({}) — {} ({})",
        info.bundle_id,
        info.short_version,
        info.bundle_version,
        ipa_path.display(),
        super::megabytes(size),
    ));

    let app_id = asc::find_app_id(&auth, &info.bundle_id)?.ok_or_else(|| {
        anyhow!(
            "no app with bundle id {} in App Store Connect. The API cannot create apps —\n\
             add it at https://appstoreconnect.apple.com/apps (＋ → New App), then re-run.",
            info.bundle_id
        )
    })?;
    let upload_id =
        asc::create_build_upload(&auth, &app_id, &info.short_version, &info.bundle_version)?;
    let file_name = ipa_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("{} has no UTF-8 file name", ipa_path.display()))?;
    let (file_id, operations) = asc::reserve_build_upload_file(&auth, &upload_id, file_name, size)?;

    let step = ui::step(ui::OperationKind::Upload, file_name);
    if let Err(e) = asc::upload_parts(&ipa_path, size, &operations) {
        step.fail("");
        return Err(e);
    }
    asc::commit_build_upload_file(&auth, &file_id)?;
    step.done(super::megabytes(size));

    if args.no_wait {
        ui::info("uploaded — App Store Connect is processing the build");
        if !notes.is_empty() {
            ui::warn(
                "store.rs release notes were not sent — TestFlight only accepts them once \
                 the build is processed; drop `--no-wait`",
            );
        }
        return Ok(());
    }
    wait_for_processing(&auth, &upload_id)?;
    ui::info(format!(
        "{} ({}) is processed — available in TestFlight and for App Store review",
        info.short_version, info.bundle_version
    ));
    if !notes.is_empty() {
        send_whats_new(&auth, &app_id, &info, &notes)?;
    }
    Ok(())
}

fn send_whats_new(
    auth: &asc::KeyAuth,
    app_id: &str,
    info: &ipa::IpaInfo,
    notes: &[(String, String)],
) -> Result<()> {
    let Some(build_id) =
        asc::find_build_id(auth, app_id, &info.short_version, &info.bundle_version)?
    else {
        // The upload itself succeeded, so this must not fail the submit.
        ui::warn(
            "store.rs release notes were not sent — App Store Connect does not list the \
             build yet; add them in TestFlight",
        );
        return Ok(());
    };
    for (locale, text) in notes {
        asc::set_beta_whats_new(auth, &build_id, locale, text)?;
    }
    ui::info(format!(
        "TestFlight \"What to Test\" set for {}",
        notes
            .iter()
            .map(|(locale, _)| locale.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(())
}

fn wait_for_processing(auth: &asc::KeyAuth, upload_id: &str) -> Result<()> {
    let step = ui::step(ui::OperationKind::Upload, "App Store Connect processing");
    let deadline = Instant::now() + PROCESSING_TIMEOUT;
    loop {
        let status = match asc::build_upload_status(auth, upload_id) {
            Ok(status) => status,
            Err(e) => {
                step.fail("");
                return Err(e);
            }
        };
        match status.state.as_str() {
            "COMPLETE" => {
                step.done("");
                for warning in &status.warnings {
                    ui::warn(warning);
                }
                return Ok(());
            }
            "FAILED" => {
                step.fail("");
                bail!(
                    "App Store Connect rejected the build:\n  {}",
                    if status.errors.is_empty() {
                        "(no detail given)".to_string()
                    } else {
                        status.errors.join("\n  ")
                    }
                );
            }
            _ if Instant::now() >= deadline => {
                step.fail("");
                bail!(
                    "still processing after {} minutes — the upload itself succeeded; check \
                     the build's status in App Store Connect",
                    PROCESSING_TIMEOUT.as_secs() / 60
                );
            }
            _ => std::thread::sleep(POLL_INTERVAL),
        }
    }
}
