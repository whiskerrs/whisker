//! Tier 2 install + relaunch.
//!
//! Tier 2 cold rebuild needs the freshly-built artifact to actually
//! land on the target and (re)start. For Android we shell out to
//! `adb`; for iOS Simulator to `xcrun simctl`; for `Host` we just
//! re-exec the binary the cargo build dropped under `target/`.
//!
//! Application identity (package name, launcher activity, bundle id,
//! …) is hard-coded for the `hello-world` example today. Generalising
//! it is a follow-up — either through a whisker.rs config or by reading
//! the example's AndroidManifest / Info.plist.

use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::process::Command;

use crate::builder::android_apk_path;
use crate::Target;

pub struct Installer {
    workspace_root: PathBuf,
    package: String,
    target: Target,
}

impl Installer {
    pub fn new(workspace_root: PathBuf, package: String, target: Target) -> Self {
        Self {
            workspace_root,
            package,
            target,
        }
    }

    pub async fn install_and_launch(&self) -> Result<()> {
        match self.target {
            Target::Host => self.host_skip(),
            Target::Android => self.android_install_and_launch().await,
            Target::IosSimulator => self.ios_skip(),
        }
    }

    fn host_skip(&self) -> Result<()> {
        eprintln!(
            "[whisker-dev-server] host target: install/launch is the user's job (run the binary yourself)"
        );
        Ok(())
    }

    fn ios_skip(&self) -> Result<()> {
        eprintln!(
            "[whisker-dev-server] iOS install/launch is not wired up in I4e — \
             rerun `xcodebuild`+`xcrun simctl install/launch` manually for now"
        );
        Ok(())
    }

    async fn android_install_and_launch(&self) -> Result<()> {
        let apk = android_apk_path(&self.workspace_root, &self.package);
        if !apk.is_file() {
            anyhow::bail!("APK missing at {}", apk.display());
        }
        let id = AndroidIdentity::for_package(&self.package);

        // adb reverse — bridge device `127.0.0.1:9876` → host port so the
        // on-device dev-runtime can reach our WebSocket without knowing
        // the emulator-gateway IP (10.0.2.2). Best-effort: it might
        // already be set from a previous run, or the device might be a
        // non-emulator that doesn't need it.
        let _ = Command::new("adb")
            .args(["reverse", "tcp:9876", "tcp:9876"])
            .status()
            .await;

        // adb install -r <apk>
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
            .args(["shell", "am", "force-stop", &id.application_id])
            .status()
            .await;

        // adb shell am start -n <pkg>/<.MainActivity>
        let component = format!("{}/{}", id.application_id, id.launcher);
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
}

/// Application identity tuple for Android. Today it's hard-coded
/// for the `hello-world` example; generalising means reading the
/// example's `AndroidManifest.xml` (or, eventually, a whisker.rs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AndroidIdentity {
    /// Android applicationId (= JVM package). Used by adb am as the
    /// left side of the component name.
    pub application_id: String,
    /// Launcher activity, expressed as the relative class shown in
    /// `am start -n <id>/<launcher>`. Always starts with a dot for
    /// short form ("am start" expands `.MainActivity` against the
    /// applicationId).
    pub launcher: String,
}

impl AndroidIdentity {
    pub(crate) fn for_package(package: &str) -> Self {
        // hello-world → rs.whisker.examples.helloworld /
        //                .MainActivity (matches the example's
        // AndroidManifest.xml). When more examples land, this lookup
        // grows or moves into per-example metadata.
        match package {
            "hello-world" => Self {
                application_id: "rs.whisker.examples.helloworld".into(),
                launcher: ".MainActivity".into(),
            },
            other => Self {
                application_id: format!("rs.whisker.examples.{}", other.replace('-', "")),
                launcher: ".MainActivity".into(),
            },
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_identity_for_hello_world_matches_manifest() {
        let id = AndroidIdentity::for_package("hello-world");
        assert_eq!(id.application_id, "rs.whisker.examples.helloworld");
        assert_eq!(id.launcher, ".MainActivity");
    }

    #[test]
    fn android_identity_for_unknown_package_strips_hyphens() {
        let id = AndroidIdentity::for_package("my-shiny-app");
        assert_eq!(id.application_id, "rs.whisker.examples.myshinyapp");
        assert_eq!(id.launcher, ".MainActivity");
    }
}
