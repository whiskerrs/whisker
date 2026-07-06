//! `whisker build ipa` — the iOS release pipeline: resolve config →
//! credential pre-step → cng sync → `xcodebuild archive` →
//! `-exportArchive`, all authenticated through the App Store Connect
//! API key (no Xcode sign-in, identical locally and in CI).

use anyhow::{Result, anyhow};
use clap::{Args as ClapArgs, ValueEnum};
use std::path::PathBuf;
use whisker_build::ios::{ExportMethod, IosReleaseInputs, ReleaseSigning};
use whisker_build::ui;
use whisker_dev_server::Target;

use crate::{credential, manifest, platforms};

/// Mirrors Apple's ExportOptions `method` spellings (`ad-hoc`, not
/// fastlane's `adhoc`) so CLI input, plist content, and xcodebuild
/// error output all use one vocabulary.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// App Store / TestFlight upload (default).
    #[value(name = "app-store-connect")]
    AppStoreConnect,
    /// Registered-device distribution (Firebase App Distribution, …).
    #[value(name = "ad-hoc")]
    AdHoc,
}

impl From<Method> for ExportMethod {
    fn from(m: Method) -> Self {
        match m {
            Method::AppStoreConnect => ExportMethod::AppStoreConnect,
            Method::AdHoc => ExportMethod::AdHoc,
        }
    }
}

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Distribution method for the export step.
    #[arg(long, value_enum, default_value = "app-store-connect")]
    method: Method,

    /// Explicit path to the app's Cargo.toml. Defaults to walking up
    /// from the current directory.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    let m = manifest::resolve(args.manifest_path.as_deref())?;
    // Same resolution `whisker run ios` uses (run.rs::ios_params_from).
    let bundle_id = m
        .config
        .ios
        .bundle_id
        .clone()
        .or_else(|| m.config.bundle_id.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.bundle_id(\"…\")) or app.bundle_id(\"…\") \
                 is required for iOS builds"
            )
        })?;
    let scheme = m
        .config
        .ios
        .scheme
        .clone()
        .or_else(|| m.config.name.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.scheme(\"…\")) or app.name(\"…\") \
                 is required for iOS builds"
            )
        })?;
    let workspace_root = crate::run::find_workspace_root(&m.crate_dir).ok_or_else(|| {
        anyhow!(
            "no [workspace] Cargo.toml at or above {}",
            m.crate_dir.display()
        )
    })?;

    // Resolved-identity banner first — see build/android.rs for why.
    let version = m.config.version.clone().unwrap_or_else(|| "0.1.0".into());
    let build_number = m.config.build_number.unwrap_or(1);
    let method: ExportMethod = args.method.into();
    ui::section("Build");
    ui::info(format!(
        "building {bundle_id} {version} ({build_number}) — release ipa [{}]",
        method.plist_value(),
    ));

    // Credential pre-step before any compilation (prompt up front,
    // fail fast on key problems). `_staging` holds the decrypted .p8
    // until xcodebuild finishes.
    let (_staging, signing) = credential::require_ios_signing(&m.crate_dir, &bundle_id)?;

    // `Target::IosSimulator` here only selects which gen tree to
    // render — gen/ios/ is one project for simulator and device;
    // release vs dev is xcodebuild's -destination, not cng's concern.
    let sync = platforms::sync_for_target(
        Target::IosSimulator,
        &m.config,
        &m.crate_dir,
        &workspace_root,
        &m.package,
    )?;
    // Stage Whisker modules' iOS Swift sources before xcodebuild so
    // the pbxproj's WhiskerModules SwiftPM ref resolves — same step
    // the dev loop runs (see whisker-dev-server::builder). Writes a
    // no-op package even with zero modules so `import WhiskerModules`
    // always resolves.
    let modules = whisker_build::modules::discover(&workspace_root.join("Cargo.toml"), &m.package)?;
    whisker_build::ios::stage_module_swift_sources(
        &sync.gen_dir,
        &workspace_root.join("platforms/ios"),
        &workspace_root.join("platforms/ios/macros"),
        &modules,
    )?;

    let ipa = whisker_build::ios::archive_and_export(&IosReleaseInputs {
        gen_dir: &sync.gen_dir,
        scheme: &scheme,
        workspace_root: &workspace_root,
        package: &m.package,
        method,
        signing: ReleaseSigning {
            team_id: &signing.team_id,
            key_path: &signing.key_path,
            key_id: &signing.key_id,
            issuer_id: &signing.issuer_id,
        },
    })?;
    ui::info(format!("✓ {}", ipa.display()));
    match method {
        ExportMethod::AppStoreConnect => {
            ui::info("upload via Transporter.app, or keep it for `whisker submit` (planned)");
        }
        ExportMethod::AdHoc => {
            ui::info(
                "installable on registered devices only — register tester UDIDs on the \
                 developer portal, then re-run to refresh the profile",
            );
        }
    }
    Ok(())
}
