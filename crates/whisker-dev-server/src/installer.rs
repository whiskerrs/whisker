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
            Target::IosSimulator => self.ios_install_and_launch().await,
        }
    }

    fn host_skip(&self) -> Result<()> {
        eprintln!(
            "[whisker-dev-server] host target: install/launch is the user's job (run the binary yourself)"
        );
        Ok(())
    }

    /// iOS Simulator install/launch.
    ///
    /// Pipeline (mirrors Android's flow but routed through
    /// `xcodebuild` + `xcrun simctl`):
    ///   1. `xcodebuild build` against the example's Xcode project
    ///      using a stable `-derivedDataPath` so we know where
    ///      `HelloWorld.app` lands without parsing xcodebuild output.
    ///   2. Ensure a Simulator is booted (best-effort `simctl boot`).
    ///   3. `simctl install booted <app>`.
    ///   4. `simctl terminate booted <bundle_id>` (best-effort, so
    ///      relaunch actually re-bootstraps the Rust runtime).
    ///   5. `simctl launch booted <bundle_id>` with
    ///      `SIMCTL_CHILD_WHISKER_DEV_ADDR=127.0.0.1:9876` so the
    ///      dev-runtime dials back. Simulator shares the host's
    ///      loopback, so no `adb reverse`-style port forwarding is
    ///      needed.
    async fn ios_install_and_launch(&self) -> Result<()> {
        let id = IosIdentity::for_package(&self.package);
        let xcode_project = self
            .workspace_root
            .join("examples")
            .join(&self.package)
            .join("ios/HelloWorld.xcodeproj");
        if !xcode_project.is_dir() {
            anyhow::bail!(
                "Xcode project missing at {} — run xcodegen first",
                xcode_project.display()
            );
        }
        let derived = self
            .workspace_root
            .join("target/.whisker/ios-derived")
            .join(&self.package);

        eprintln!("[whisker-dev-server] xcodebuild building app for Simulator");
        let xc_status = Command::new("xcodebuild")
            .arg("-project")
            .arg(&xcode_project)
            .args(["-scheme", &id.scheme])
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

        // Stable derived-data layout: Build/Products/<config>-iphonesimulator/<scheme>.app
        let app_path = derived
            .join("Build/Products/Debug-iphonesimulator")
            .join(format!("{}.app", id.scheme));
        if !app_path.is_dir() {
            anyhow::bail!(
                "expected {}.app missing under {} after build",
                id.scheme,
                derived.display()
            );
        }

        // Best-effort boot. If a sim is already booted simctl exits
        // with an error string but we don't care — `booted` resolves
        // to whatever is running.
        let _ = Command::new("xcrun")
            .args(["simctl", "boot", &id.default_device])
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

        // Force the previous run to die so the relaunch reruns
        // app start (which re-dlopens the user dylib + reconnects
        // the WebSocket).
        let _ = Command::new("xcrun")
            .args(["simctl", "terminate", "booted", &id.bundle_id])
            .status()
            .await;

        // Pass the dev-server WS addr into the launched process. The
        // `SIMCTL_CHILD_<NAME>` convention turns into a regular
        // `<NAME>` env var inside the simulated app's process.
        let launch = Command::new("xcrun")
            .args(["simctl", "launch", "booted", &id.bundle_id])
            .env("SIMCTL_CHILD_WHISKER_DEV_ADDR", "127.0.0.1:9876")
            .status()
            .await
            .context("spawn simctl launch")?;
        if !launch.success() {
            anyhow::bail!("simctl launch {} failed ({launch})", id.bundle_id);
        }
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

/// iOS counterpart to [`AndroidIdentity`]. Same hard-coded mapping
/// for `hello-world` today; same generalisation story (read from
/// `Info.plist` / a future `whisker.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IosIdentity {
    /// CFBundleIdentifier from `Info.plist`. Used by simctl install /
    /// terminate / launch as the right-hand identifier.
    pub bundle_id: String,
    /// Xcode scheme + .app filename. With XcodeGen-generated projects
    /// these always match the project name.
    pub scheme: String,
    /// Simulator name we ask simctl to boot if nothing is currently
    /// booted. Picking a specific model rather than relying on
    /// "first available" keeps the dev loop reproducible across
    /// machines.
    pub default_device: String,
}

impl IosIdentity {
    pub(crate) fn for_package(package: &str) -> Self {
        // hello-world → rs.whisker.examples.helloWorld / HelloWorld
        // (matches examples/hello-world/ios/project.yml). When more
        // examples land, this grows or moves out to per-example
        // metadata, same as AndroidIdentity.
        let default_device = std::env::var("WHISKER_IOS_SIMULATOR")
            .unwrap_or_else(|_| pick_available_iphone().unwrap_or_else(|| "iPhone 17 Pro".into()));
        match package {
            "hello-world" => Self {
                bundle_id: "rs.whisker.examples.helloWorld".into(),
                scheme: "HelloWorld".into(),
                default_device,
            },
            other => Self {
                bundle_id: format!("rs.whisker.examples.{}", camel_case(other)),
                scheme: pascal_case(other),
                default_device,
            },
        }
    }
}

/// Best-effort pick of an iPhone simulator that's installed on this
/// machine. Falls back to a hard-coded default if `simctl` isn't
/// available or the output doesn't parse — caller can still override
/// via `WHISKER_IOS_SIMULATOR`.
///
/// Picks the *first* iPhone name that appears in `simctl list devices`,
/// independent of OS version (we want any iPhone — newest Xcode might
/// only ship iPhone 17 Pro, older Xcode might be on iPhone 15).
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
        // Lines look like:
        //   iPhone 17 Pro (UDID...) (Shutdown)
        // Slice off everything from " (" onwards to get the name.
        let Some((name, _rest)) = trimmed.split_once(" (") else {
            continue;
        };
        if name.starts_with("iPhone ") {
            return Some(name.to_string());
        }
    }
    None
}

/// "hello-world" → "helloWorld"
fn camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper_next = false;
    for ch in s.chars() {
        if ch == '-' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// "hello-world" → "HelloWorld"
fn pascal_case(s: &str) -> String {
    let camel = camel_case(s);
    let mut chars = camel.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
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

    #[test]
    fn ios_identity_for_hello_world_matches_project_yml() {
        let id = IosIdentity::for_package("hello-world");
        assert_eq!(id.bundle_id, "rs.whisker.examples.helloWorld");
        assert_eq!(id.scheme, "HelloWorld");
        // default_device is dynamic (depends on what simctl reports
        // on this machine), so assert only that it looks plausible.
        assert!(
            id.default_device.starts_with("iPhone"),
            "got default_device={:?}",
            id.default_device,
        );
    }

    #[test]
    fn ios_identity_for_unknown_package_camel_and_pascal_cases() {
        let id = IosIdentity::for_package("my-shiny-app");
        assert_eq!(id.bundle_id, "rs.whisker.examples.myShinyApp");
        assert_eq!(id.scheme, "MyShinyApp");
    }
}
