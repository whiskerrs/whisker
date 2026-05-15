//! App configuration types used by `whisker.rs`.
//!
//! Users build an `AppConfig` via the builder API:
//! ```ignore
//! pub fn configure(app: &mut AppConfig) {
//!     app.name("MyApp")
//!        .bundle_id("dev.example.myapp")
//!        .version("1.0.0");
//!
//!     app.android(|a| a
//!         .application_id("dev.example.myapp")
//!         .launcher_activity(".MainActivity")
//!         .min_sdk(24));
//!
//!     app.ios(|i| i
//!         .bundle_id("dev.example.MyApp")
//!         .scheme("MyApp")
//!         .deployment_target("14.0"));
//! }
//! ```
//!
//! `whisker run` compiles a tiny probe binary that includes the user's
//! `whisker.rs` and serializes the resulting `AppConfig` to JSON over
//! stdout. The host shell (`whisker-cli`) parses that JSON, projects
//! the fields it needs (paths, application id, bundle id, scheme, …),
//! and passes them as flat parameters to `whisker-dev-server`. The
//! dev-server itself does not depend on this crate.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub build_number: Option<u32>,
    pub ios: IosConfig,
    pub android: AndroidConfig,
}

impl AppConfig {
    pub fn name(&mut self, name: impl Into<String>) -> &mut Self {
        self.name = Some(name.into());
        self
    }

    pub fn bundle_id(&mut self, id: impl Into<String>) -> &mut Self {
        self.bundle_id = Some(id.into());
        self
    }

    pub fn version(&mut self, v: impl Into<String>) -> &mut Self {
        self.version = Some(v.into());
        self
    }

    pub fn build_number(&mut self, n: u32) -> &mut Self {
        self.build_number = Some(n);
        self
    }

    pub fn ios(&mut self, f: impl FnOnce(&mut IosConfig)) -> &mut Self {
        f(&mut self.ios);
        self
    }

    pub fn android(&mut self, f: impl FnOnce(&mut AndroidConfig)) -> &mut Self {
        f(&mut self.android);
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct IosConfig {
    /// CFBundleIdentifier of the iOS app. Used by `xcrun simctl
    /// install / terminate / launch` and as the right-hand side of
    /// the `am start -n` style component string. Falls back to the
    /// top-level [`AppConfig::bundle_id`] if unset (since iOS and
    /// Android often share a bundle id but not always).
    pub bundle_id: Option<String>,
    /// Xcode scheme + the `<scheme>.app` filename xcodebuild
    /// produces. With XcodeGen-generated projects these always
    /// match the project name.
    pub scheme: Option<String>,
    pub deployment_target: Option<String>,
}

impl IosConfig {
    pub fn bundle_id(&mut self, id: impl Into<String>) -> &mut Self {
        self.bundle_id = Some(id.into());
        self
    }

    pub fn scheme(&mut self, s: impl Into<String>) -> &mut Self {
        self.scheme = Some(s.into());
        self
    }

    pub fn deployment_target(&mut self, t: impl Into<String>) -> &mut Self {
        self.deployment_target = Some(t.into());
        self
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AndroidConfig {
    pub package: Option<String>,
    pub min_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
    /// Android `applicationId` (= JVM package the launcher invokes).
    /// Used as the left side of `am start -n <id>/<launcher>`.
    /// Distinct from `package` (the Kotlin/Java package the manifest
    /// declares for `R.java` lookups), which is purely a build-time
    /// convention.
    pub application_id: Option<String>,
    /// Launcher activity class name, with a leading dot. `am start
    /// -n` expands `.MainActivity` against `application_id`.
    /// Defaults to `.MainActivity` if unset.
    pub launcher_activity: Option<String>,
}

impl AndroidConfig {
    pub fn package(&mut self, p: impl Into<String>) -> &mut Self {
        self.package = Some(p.into());
        self
    }

    pub fn min_sdk(&mut self, v: u32) -> &mut Self {
        self.min_sdk = Some(v);
        self
    }

    pub fn target_sdk(&mut self, v: u32) -> &mut Self {
        self.target_sdk = Some(v);
        self
    }

    pub fn application_id(&mut self, id: impl Into<String>) -> &mut Self {
        self.application_id = Some(id.into());
        self
    }

    pub fn launcher_activity(&mut self, a: impl Into<String>) -> &mut Self {
        self.launcher_activity = Some(a.into());
        self
    }
}
