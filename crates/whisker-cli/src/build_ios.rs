//! iOS-side production build helpers for `whisker build`.
//!
//! Phase 2 split: the xcframework wrap (`cargo xtask ios
//! build-xcframework`) is invoked as a subprocess here — Phase 3 will
//! port that logic into this module so the user-app build path no
//! longer needs xtask. `xcodebuild` against the generated Xcode
//! project (under `gen/ios/`) already lives here.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build the WhiskerDriver.xcframework that wraps the user crate's
/// dylib for all iOS slices (arm64-ios + arm64-ios-sim + x86_64-ios).
///
/// **Temporary**: delegates to `cargo xtask ios build-xcframework -p
/// <package>`. The xcframework logic will move into this module in
/// Phase 3 so the user-app build path drops its xtask dependency
/// entirely. Until then this is the same code path `whisker run`
/// triggers via `whisker-dev-server::Builder`.
pub fn build_xcframework(workspace_root: &Path, package: &str) -> Result<()> {
    eprintln!("[whisker build] cargo xtask ios build-xcframework -p {package}");
    let status = Command::new("cargo")
        .args(["xtask", "ios", "build-xcframework", "-p", package])
        .current_dir(workspace_root)
        .status()
        .context("spawn cargo xtask ios build-xcframework")?;
    if !status.success() {
        return Err(anyhow!("xtask build-xcframework failed ({status})"));
    }
    Ok(())
}

/// Configuration for an `xcodebuild` invocation. We only model the
/// switches `whisker build` cares about — the user can always drop
/// down to xcodebuild directly with the generated `gen/ios/` project.
pub struct XcodebuildArgs<'a> {
    pub gen_ios: &'a Path,
    pub scheme: &'a str,
    /// Either `iphonesimulator` (Simulator) or `iphoneos` (device).
    pub sdk: &'a str,
    /// `Release` for `whisker build`, `Debug` is unused today but
    /// kept here so a future Phase 3 can reuse this for `whisker run`.
    pub configuration: &'a str,
    /// `<scheme>.xcodeproj` is the canonical XcodeGen output, but tests
    /// might point at a different name. Default in callers: same as
    /// `scheme`.
    pub xcodeproj_name: &'a str,
    /// Out dir for `-derivedDataPath`. Picked by callers so the gen
    /// tree stays drift-free for the next regeneration.
    pub derived_data: &'a Path,
}

/// Run `xcodebuild -configuration <configuration>` and return the
/// produced `.app` directory.
pub fn run_xcodebuild_app(args: &XcodebuildArgs<'_>) -> Result<PathBuf> {
    let project = args.gen_ios.join(format!("{}.xcodeproj", args.xcodeproj_name));
    if !project.is_dir() {
        return Err(anyhow!(
            "Xcode project missing at {} — did `xcodegen generate` run?",
            project.display(),
        ));
    }

    eprintln!(
        "[whisker build] xcodebuild -configuration {} -sdk {}",
        args.configuration, args.sdk,
    );
    let destination = match args.sdk {
        "iphonesimulator" => "generic/platform=iOS Simulator".to_string(),
        "iphoneos" => "generic/platform=iOS".to_string(),
        other => return Err(anyhow!("unknown SDK: {other}")),
    };

    let status = Command::new("xcodebuild")
        .arg("-project")
        .arg(&project)
        .args(["-scheme", args.scheme])
        .args(["-configuration", args.configuration])
        .args(["-destination", &destination])
        .arg("-derivedDataPath")
        .arg(args.derived_data)
        .args(["-quiet", "build"])
        .status()
        .context("spawn xcodebuild")?;
    if !status.success() {
        return Err(anyhow!("xcodebuild failed ({status})"));
    }

    let product_subdir = match args.sdk {
        "iphonesimulator" => format!("{}-iphonesimulator", args.configuration),
        "iphoneos" => format!("{}-iphoneos", args.configuration),
        _ => unreachable!("checked above"),
    };
    let app = args
        .derived_data
        .join("Build/Products")
        .join(product_subdir)
        .join(format!("{}.app", args.scheme));
    if !app.is_dir() {
        return Err(anyhow!(
            "xcodebuild succeeded but {} is missing",
            app.display(),
        ));
    }
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_xcodeproj_errors() {
        let tmp = std::env::temp_dir().join("whisker-cli-build_ios-test");
        let _ = std::fs::create_dir_all(&tmp);
        let dd = tmp.join("derived");
        let args = XcodebuildArgs {
            gen_ios: &tmp,
            scheme: "X",
            sdk: "iphonesimulator",
            configuration: "Release",
            xcodeproj_name: "X",
            derived_data: &dd,
        };
        let err = run_xcodebuild_app(&args).unwrap_err();
        assert!(
            err.to_string().contains("Xcode project missing"),
            "got: {err:#}",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unknown_sdk_errors() {
        let tmp = std::env::temp_dir().join("whisker-cli-build_ios-sdk-test");
        let _ = std::fs::create_dir_all(&tmp);
        let proj = tmp.join("X.xcodeproj");
        let _ = std::fs::create_dir_all(&proj);
        let dd = tmp.join("derived");
        let args = XcodebuildArgs {
            gen_ios: &tmp,
            scheme: "X",
            sdk: "bogus",
            configuration: "Release",
            xcodeproj_name: "X",
            derived_data: &dd,
        };
        let err = run_xcodebuild_app(&args).unwrap_err();
        assert!(err.to_string().contains("unknown SDK"), "got: {err:#}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
