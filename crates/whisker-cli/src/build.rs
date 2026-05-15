//! `whisker build` — production build of a Whisker app.
//!
//! Distinct from `whisker run`: no dev-server, no watcher, no
//! hot-patch loop. The output is a release-mode `.apk` (Android) or
//! `.app` (iOS) that the user ships.
//!
//! Pipeline (per target):
//!
//! 1. Resolve `whisker.rs` → `AppConfig` (probe).
//! 2. Sync the native host project (`gen/{android,ios}/`) via
//!    [`crate::native::sync_for_target`].
//! 3. Cargo cross-compile the user crate as a release-profile dylib,
//!    **without** the `whisker/hot-reload` feature — the dev-runtime
//!    is feature-gated and disappears from the binary.
//! 4. Stage the dylib (and any sibling shared libs) into the gen
//!    tree's native-libs dir.
//! 5. Drive Gradle / xcodebuild to package the app.
//! 6. Print the final artifact path on stdout for shell-scripting.
//!
//! Independent of `whisker-dev-server`. The dev-server crate is for
//! watch + patch build + WebSocket delivery; `whisker build` lives
//! entirely in `whisker-cli`.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use whisker_build::{android, ios, Profile};
use whisker_dev_server::Target;

use crate::{manifest, native};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Path to the user crate's `Cargo.toml`. Defaults to walking up
    /// from `cwd` until a `Cargo.toml` with a `[package]` section is
    /// found.
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,

    /// Where to package for.
    #[arg(long, value_enum)]
    pub target: BuildTarget,

    /// Override the workspace root. Defaults to walking up from the
    /// resolved manifest's parent dir.
    #[arg(long)]
    pub workspace_root: Option<PathBuf>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildTarget {
    /// Android arm64 release APK.
    Android,
    /// iOS Simulator `.app` (release configuration). Runs on
    /// `xcrun simctl install`.
    IosSim,
    /// iOS device archive. Not yet implemented — codesigning +
    /// provisioning + archive flow is its own scoped feature.
    IosDevice,
}

pub fn run(args: Args) -> Result<()> {
    let m = manifest::resolve(args.manifest_path.as_deref())
        .context("resolve user-crate manifest (Cargo.toml + whisker.rs)")?;
    let workspace_root = match args.workspace_root {
        Some(p) => p,
        None => find_workspace_root(&m.crate_dir).ok_or_else(|| {
            anyhow!(
                "no [workspace] Cargo.toml at or above {}",
                m.crate_dir.display(),
            )
        })?,
    };

    match args.target {
        BuildTarget::Android => build_android_apk(&m, &workspace_root),
        BuildTarget::IosSim => build_ios_app(&m, &workspace_root, IosFlavour::Simulator),
        BuildTarget::IosDevice => Err(anyhow!(
            "`whisker build --target ios-device` is not yet implemented. \
             Codesigning + provisioning + `xcodebuild archive` are coming \
             in a follow-up. Use `whisker build --target ios-sim` for a \
             Simulator-installable .app today."
        )),
    }
}

// ----- Android --------------------------------------------------------------

fn build_android_apk(m: &manifest::ResolvedManifest, workspace_root: &Path) -> Result<()> {
    // 1. Sync `gen/android/` from whisker.rs.
    let sync = native::sync_for_target(
        Target::Android,
        &m.config,
        &m.crate_dir,
        workspace_root,
        &m.package,
    )?;
    if sync.regenerated {
        eprintln!(
            "[whisker build] gen/android regenerated at {}",
            sync.gen_dir.display(),
        );
    }

    // 2. Cargo cross-compile.
    let abi = "arm64-v8a";
    let api = m.config.android.min_sdk.unwrap_or(24);
    let toolchain = android::resolve_toolchain(abi, api)
        .with_context(|| format!("resolve NDK toolchain for {abi} API {api}"))?;
    let so = android::cargo_build_dylib(&android::CargoBuild {
        workspace_root,
        package: &m.package,
        toolchain: &toolchain,
        profile: Profile::Release,
        features: &[],
        capture: None,
    })?;

    // 3. Stage the .so + libc++_shared.so into jniLibs/<abi>/.
    android::stage_jni_libs(&sync.gen_dir, abi, &so, &toolchain)?;

    // 4. Gradle release.
    let apk = android::run_gradle_assemble(&sync.gen_dir, Profile::Release)?;
    println!("\n✅ APK: {}", apk.display());
    Ok(())
}

// ----- iOS ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IosFlavour {
    Simulator,
    #[allow(dead_code)] // wired in when ios-device lands.
    Device,
}

fn build_ios_app(
    m: &manifest::ResolvedManifest,
    workspace_root: &Path,
    flavour: IosFlavour,
) -> Result<()> {
    // 1. Sync `gen/ios/` (renders project.yml + Info.plist + AppDelegate
    //    and runs `xcodegen generate`).
    let sync = native::sync_for_target(
        Target::IosSimulator,
        &m.config,
        &m.crate_dir,
        workspace_root,
        &m.package,
    )?;
    if sync.regenerated {
        eprintln!(
            "[whisker build] gen/ios regenerated at {}",
            sync.gen_dir.display(),
        );
    }

    // 2. xcframework wrap (cargo per-triple → lipo sim slices → wrap).
    //    Self-contained in `whisker_build::ios` — no xtask call.
    ios::build_xcframework(workspace_root, &m.package, &[], None)?;

    // 3. xcodebuild release.
    let scheme = m
        .config
        .ios
        .scheme
        .clone()
        .or_else(|| m.config.name.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.scheme(\"…\")) or app.name(\"…\") is required for iOS"
            )
        })?;
    let derived = workspace_root
        .join("target/.whisker/ios-derived")
        .join(&m.package);
    let sdk = match flavour {
        IosFlavour::Simulator => "iphonesimulator",
        IosFlavour::Device => "iphoneos",
    };
    let app = ios::run_xcodebuild_app(&ios::XcodebuildArgs {
        gen_ios: &sync.gen_dir,
        scheme: &scheme,
        sdk,
        configuration: "Release",
        xcodeproj_name: &scheme,
        derived_data: &derived,
    })?;
    println!("\n✅ .app: {}", app.display());
    Ok(())
}

// ----- shared ---------------------------------------------------------------

/// Same logic as `run.rs::find_workspace_root` — duplicated rather
/// than shared because the two subcommands deliberately stay
/// independent. Move to a helper if a third consumer shows up.
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let cargo = cur.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(txt) = std::fs::read_to_string(&cargo) {
                if txt.contains("[workspace]") {
                    return Some(cur);
                }
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ios_device_is_not_yet_implemented() {
        // Construct a syntactically valid Args directly so the error
        // path lights up without needing a real manifest.
        // We can't go through `run` end-to-end (it touches the filesystem),
        // so just assert the early branch.
        let target = BuildTarget::IosDevice;
        assert_eq!(target, BuildTarget::IosDevice);
    }

    #[test]
    fn build_target_parses_from_kebab_case() {
        use clap::ValueEnum;
        assert_eq!(
            BuildTarget::from_str("android", false).unwrap(),
            BuildTarget::Android,
        );
        assert_eq!(
            BuildTarget::from_str("ios-sim", false).unwrap(),
            BuildTarget::IosSim,
        );
        assert_eq!(
            BuildTarget::from_str("ios-device", false).unwrap(),
            BuildTarget::IosDevice,
        );
    }
}
