//! `whisker build appbundle` / `whisker build apk` — the Android
//! release pipeline: resolve config → credential pre-step → cng
//! sync → `gradle :app:bundleRelease` / `:app:assembleRelease` with
//! signing injected via env vars.

use anyhow::{Result, anyhow};
use clap::Args as ClapArgs;
use std::path::PathBuf;
use whisker_build::android::ReleaseArtifact;
use whisker_build::ui;
use whisker_dev_server::Target;

use crate::{credential, manifest, platforms};

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(artifact: ReleaseArtifact, args: Args) -> Result<()> {
    let m = manifest::resolve(args.manifest_path.as_deref())?;
    let application_id = manifest::android_application_id(&m.config).ok_or_else(|| {
        anyhow!(
            "whisker.rs: app.android(|a| a.application_id(\"…\")) (or app.bundle_id) \
             is required for Android builds"
        )
    })?;
    let workspace_root = crate::run::find_workspace_root(&m.crate_dir).ok_or_else(|| {
        anyhow!(
            "no [workspace] Cargo.toml at or above {}",
            m.crate_dir.display()
        )
    })?;

    // Announce the fully resolved identity FIRST. `configure()` is
    // arbitrary Rust and may branch on ambient env (WHISKER_ENV-
    // style flavors) — silently building the wrong flavor is the
    // failure mode this line exists to catch.
    let version = m.config.version.clone().unwrap_or_else(|| "0.1.0".into());
    let build_number = m.config.build_number.unwrap_or(1);
    let label = match artifact {
        ReleaseArtifact::AppBundle => "appbundle (.aab)",
        ReleaseArtifact::Apk => "apk",
    };
    ui::section("Build");
    ui::info(format!(
        "building {application_id} {version} ({build_number}) — release {label}",
    ));

    // Credential pre-step BEFORE any compilation: the decryption-key
    // prompt (if any) happens now, and key problems fail before, not
    // after, the long gradle+cargo build. `_staging` must stay alive
    // until gradle exits — the signing paths point into it.
    let (_staging, signing) = credential::require_android_signing(&m.crate_dir, &application_id)?;

    let sync = platforms::sync_for_target(
        Target::Android,
        &m.config,
        &m.crate_dir,
        &workspace_root,
        &m.package,
    )?;

    // The Gradle Settings plugin trusts a Cargo.lock-keyed module
    // report cache that goes stale in ways the lock hash can't see
    // (multi-app workspaces, metadata-only edits). Rewrite it fresh
    // before gradle reads it — see refresh_gradle_module_cache docs.
    whisker_build::modules::refresh_gradle_module_cache(&workspace_root, &m.package)?;

    // Contract with the generated app/build.gradle.kts — see the
    // WHISKER_ANDROID_* block in the template.
    let signing_env = vec![
        (
            "WHISKER_ANDROID_KEYSTORE".to_string(),
            signing.keystore_path.display().to_string(),
        ),
        (
            "WHISKER_ANDROID_KEYSTORE_PASSWORD".to_string(),
            signing.store_password.clone(),
        ),
        (
            "WHISKER_ANDROID_KEY_ALIAS".to_string(),
            signing.key_alias.clone(),
        ),
        (
            "WHISKER_ANDROID_KEY_PASSWORD".to_string(),
            signing.key_password.clone(),
        ),
    ];

    let artifact_path =
        whisker_build::android::run_gradle_release(&sync.gen_dir, artifact, &signing_env)?;
    ui::info(format!("✓ {}", artifact_path.display()));
    match artifact {
        ReleaseArtifact::AppBundle => {
            ui::info("upload to Play Console (first release: create the app there manually)");
        }
        ReleaseArtifact::Apk => {
            ui::info("ready for direct distribution (Firebase App Distribution, sideload, …)");
        }
    }
    Ok(())
}
