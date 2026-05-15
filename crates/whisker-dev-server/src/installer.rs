//! Tier 2 install + relaunch.
//!
//! After a successful cold-rebuild, the freshly-built artifact has to
//! land on the target and start (re-bootstrapping the dev-runtime so
//! it dials the dev-server back). For Android we shell out to `adb`;
//! for iOS Simulator to `xcrun simctl`; for `Host` we no-op (the user
//! runs the host binary themselves).
//!
//! Application identity (bundle id, applicationId, launcher activity,
//! scheme, …) is **not** baked in here. The cli passes those as
//! `Config::android` / `Config::ios` after reading the user's
//! `whisker.rs::configure(&mut AppConfig)`, so this module has zero
//! knowledge of which example or external user crate is in play.

use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::process::Command;

use crate::{AndroidParams, IosParams, Target};

pub struct Installer {
    target: Target,
    android: Option<AndroidParams>,
    ios: Option<IosParams>,
    workspace_root: PathBuf,
    package: String,
}

impl Installer {
    pub fn new(
        target: Target,
        android: Option<AndroidParams>,
        ios: Option<IosParams>,
        workspace_root: PathBuf,
        package: String,
    ) -> Self {
        Self {
            target,
            android,
            ios,
            workspace_root,
            package,
        }
    }

    pub async fn install_and_launch(&self) -> Result<()> {
        match self.target {
            Target::Host => self.host_skip(),
            Target::Android => {
                let p = self.android.as_ref().context(
                    "target=Android but no AndroidParams — cli must populate Config.android",
                )?;
                android_install_and_launch(p).await
            }
            Target::IosSimulator => {
                let p = self.ios.as_ref().context(
                    "target=IosSimulator but no IosParams — cli must populate Config.ios",
                )?;
                ios_install_and_launch(p, &self.workspace_root, &self.package).await
            }
        }
    }

    fn host_skip(&self) -> Result<()> {
        eprintln!(
            "[whisker-dev-server] host target: install/launch is the user's job (run the binary yourself)"
        );
        Ok(())
    }
}

async fn android_install_and_launch(p: &AndroidParams) -> Result<()> {
    let apk = p
        .project_dir
        .join("app/build/outputs/apk/debug/app-debug.apk");
    if !apk.is_file() {
        anyhow::bail!("APK missing at {}", apk.display());
    }

    // adb reverse — bridge device `127.0.0.1:9876` → host port so the
    // on-device dev-runtime can reach our WebSocket without knowing
    // the emulator-gateway IP (10.0.2.2). Best-effort: it might
    // already be set from a previous run, or the device might be a
    // non-emulator that doesn't need it.
    let _ = Command::new("adb")
        .args(["reverse", "tcp:9876", "tcp:9876"])
        .status()
        .await;

    let install = Command::new("adb")
        .args(["install", "-r"])
        .arg(&apk)
        .status()
        .await
        .context("spawn adb install")?;
    if !install.success() {
        anyhow::bail!("adb install -r {} failed ({install})", apk.display());
    }

    // adb shell am force-stop  (so the relaunch actually re-bootstraps)
    let _ = Command::new("adb")
        .args(["shell", "am", "force-stop", &p.application_id])
        .status()
        .await;

    let component = format!("{}/{}", p.application_id, p.launcher_activity);
    let launch = Command::new("adb")
        .args(["shell", "am", "start", "-n", &component])
        .status()
        .await
        .context("spawn adb am start")?;
    if !launch.success() {
        anyhow::bail!("adb am start {component} failed ({launch})");
    }
    Ok(())
}

async fn ios_install_and_launch(
    p: &IosParams,
    workspace_root: &std::path::Path,
    package: &str,
) -> Result<()> {
    let xcode_project = p.project_dir.join(format!("{}.xcodeproj", p.scheme));
    if !xcode_project.is_dir() {
        anyhow::bail!(
            "Xcode project missing at {} — run xcodegen first",
            xcode_project.display()
        );
    }
    let derived = workspace_root
        .join("target/.whisker/ios-derived")
        .join(package);

    eprintln!("[whisker-dev-server] xcodebuild building app for Simulator");
    let xc_status = Command::new("xcodebuild")
        .arg("-project")
        .arg(&xcode_project)
        .args(["-scheme", &p.scheme])
        .args(["-configuration", "Debug"])
        .args(["-destination", "generic/platform=iOS Simulator"])
        .arg("-derivedDataPath")
        .arg(&derived)
        .args(["-quiet", "build"])
        .status()
        .await
        .context("spawn xcodebuild")?;
    if !xc_status.success() {
        anyhow::bail!("xcodebuild build failed ({xc_status})");
    }

    let app_path = derived
        .join("Build/Products/Debug-iphonesimulator")
        .join(format!("{}.app", p.scheme));
    if !app_path.is_dir() {
        anyhow::bail!(
            "expected {}.app missing under {} after build",
            p.scheme,
            derived.display()
        );
    }

    // Best-effort boot of either the caller's override or the first
    // available iPhone simctl knows about.
    let device = p
        .device_override
        .clone()
        .or_else(pick_available_iphone)
        .unwrap_or_else(|| "iPhone 17 Pro".into());
    let _ = Command::new("xcrun")
        .args(["simctl", "boot", &device])
        .status()
        .await;

    eprintln!("[whisker-dev-server] simctl install {}", app_path.display());
    let install = Command::new("xcrun")
        .args(["simctl", "install", "booted"])
        .arg(&app_path)
        .status()
        .await
        .context("spawn simctl install")?;
    if !install.success() {
        anyhow::bail!("simctl install {} failed ({install})", app_path.display());
    }

    // Force the previous run to die so the relaunch re-bootstraps the
    // runtime + reconnects the dev WebSocket.
    let _ = Command::new("xcrun")
        .args(["simctl", "terminate", "booted", &p.bundle_id])
        .status()
        .await;

    // `SIMCTL_CHILD_<NAME>` shows up as `<NAME>` inside the launched
    // app's env — that's how the dev-runtime finds us.
    let launch = Command::new("xcrun")
        .args(["simctl", "launch", "booted", &p.bundle_id])
        .env("SIMCTL_CHILD_WHISKER_DEV_ADDR", "127.0.0.1:9876")
        .status()
        .await
        .context("spawn simctl launch")?;
    if !launch.success() {
        anyhow::bail!("simctl launch {} failed ({launch})", p.bundle_id);
    }
    Ok(())
}

/// Best-effort pick of an iPhone simulator that's installed on this
/// machine. `pick_available_iphone()` returns `None` if simctl isn't
/// available or the output doesn't parse; the caller substitutes a
/// hard-coded default.
fn pick_available_iphone() -> Option<String> {
    let out = std::process::Command::new("xcrun")
        .args(["simctl", "list", "devices", "available"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        // Lines look like:  iPhone 17 Pro (UDID...) (Shutdown)
        let Some((name, _rest)) = trimmed.split_once(" (") else {
            continue;
        };
        if name.starts_with("iPhone ") {
            return Some(name.to_string());
        }
    }
    None
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn android_params() -> AndroidParams {
        AndroidParams {
            project_dir: PathBuf::from("/tmp/x"),
            application_id: "rs.whisker.examples.helloworld".into(),
            launcher_activity: ".MainActivity".into(),
            abi: "arm64-v8a".into(),
        }
    }

    #[test]
    fn installer_for_host_doesnt_need_android_or_ios() {
        let inst = Installer::new(Target::Host, None, None, PathBuf::new(), "x".into());
        // Just exercise the `host_skip` branch via the public API —
        // it doesn't await anything async so we can run it on the
        // current thread without a runtime.
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async { inst.install_and_launch().await.unwrap() });
    }

    #[test]
    fn installer_for_android_without_params_errors() {
        let inst = Installer::new(Target::Android, None, None, PathBuf::new(), "x".into());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt
            .block_on(async { inst.install_and_launch().await })
            .unwrap_err();
        assert!(err.to_string().contains("AndroidParams"), "got: {err:#}");
    }

    #[test]
    fn installer_for_ios_without_params_errors() {
        let inst = Installer::new(Target::IosSimulator, None, None, PathBuf::new(), "x".into());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt
            .block_on(async { inst.install_and_launch().await })
            .unwrap_err();
        assert!(err.to_string().contains("IosParams"), "got: {err:#}");
    }

    #[test]
    fn android_install_errors_when_apk_missing() {
        let p = android_params();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt
            .block_on(async { android_install_and_launch(&p).await })
            .unwrap_err();
        assert!(err.to_string().contains("APK missing"), "got: {err:#}");
    }
}
