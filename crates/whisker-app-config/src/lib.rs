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
//!
//!     // Whisker CNG plugin declarations live alongside the platform
//!     // blocks. The Config struct's `PluginConfig::NAME` keys the
//!     // entry inside `plugins`, so this call replaces any prior
//!     // configuration for the same plugin.
//!     app.plugin::<Firebase>(|c| c
//!         .google_service_path("ios/GoogleService-Info.plist"));
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
use std::collections::BTreeMap;
use whisker_plugin::{Plugin, PluginConfig};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub build_number: Option<u32>,
    pub ios: IosConfig,
    pub android: AndroidConfig,
    /// Per-plugin Config serialized as JSON, keyed by the Config
    /// struct's `PluginConfig::NAME`. `whisker-cng` reads this map
    /// when composing the plugin pipeline — every entry corresponds
    /// to one `app.plugin::<T>(|cfg| ...)` call in `whisker.rs`.
    ///
    /// `BTreeMap` over `HashMap` for deterministic iteration order:
    /// `whisker-cng`'s fingerprint hashes the serialized AppConfig,
    /// and HashMap's random ordering would break the skip path.
    #[serde(default)]
    pub plugins: BTreeMap<String, serde_json::Value>,
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

    /// Declare a Whisker CNG plugin and configure its options.
    ///
    /// `P` is the plugin type (the `Plugin` trait impl shipped by
    /// the plugin author, e.g. `WhiskerAudio` from `whisker-audio`).
    /// The closure receives a `&mut P::Config` — the typed config
    /// struct paired with `P` — starting from `Config::default()`
    /// so a no-config call site reads as `app.plugin::<P>(|_| {})`
    /// and a configured one reads as `app.plugin::<P>(|c| c.field(...))`.
    ///
    /// The resulting Config is serialized as JSON and stored under
    /// `plugins[P::Config::NAME]`. Calling `plugin::<P>` twice for
    /// the same `P` replaces the prior entry — last call wins.
    ///
    /// ## Why the generic is `P: Plugin`, not `P: PluginConfig`
    ///
    /// `P` is the user-facing name of the plugin (the noun a user
    /// would say out loud: "I'm using WhiskerAudio"); `P::Config`
    /// is an implementation detail you only touch through the
    /// closure parameter. Spelling the plugin's identity as the
    /// turbofish keeps `app.plugin::<WhiskerAudio>(|c| …)` reading
    /// like "enable the WhiskerAudio plugin" instead of "register
    /// this config struct".
    ///
    /// # Panics
    ///
    /// If `serde_json::to_value(&cfg)` fails. In practice
    /// `PluginConfig::serialize` is total for any sane Config
    /// struct (the bound is enforced statically), so this only
    /// fires if the Config holds something inexpressible in JSON
    /// (e.g. a non-string map key or an enum without
    /// `#[serde(tag = ...)]`). Plugin authors should fix their
    /// Config; we don't return `Result` because `whisker.rs` is
    /// meant to be ergonomic builder code, not error-handling code.
    pub fn plugin<P>(&mut self, f: impl FnOnce(&mut P::Config)) -> &mut Self
    where
        P: Plugin,
    {
        let mut cfg = <P::Config as Default>::default();
        f(&mut cfg);
        let name = <P::Config as PluginConfig>::NAME;
        let json = serde_json::to_value(&cfg).unwrap_or_else(|e| {
            panic!("AppConfig::plugin: failed to serialize Config for plugin `{name}`: {e}",)
        });
        self.plugins.insert(name.to_string(), json);
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

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_plugin::GenerateContext;

    #[derive(Default, Serialize, Deserialize)]
    struct PermissionsConfig {
        camera_reason: Option<String>,
        permissions: Vec<String>,
    }

    impl PluginConfig for PermissionsConfig {
        const NAME: &'static str = "whisker-permissions";
    }

    impl PermissionsConfig {
        fn camera_reason(&mut self, r: impl Into<String>) -> &mut Self {
            self.camera_reason = Some(r.into());
            self
        }
        fn add(&mut self, p: impl Into<String>) -> &mut Self {
            self.permissions.push(p.into());
            self
        }
    }

    struct Permissions;
    impl Plugin for Permissions {
        type Config = PermissionsConfig;
        fn apply(&self, _: &mut GenerateContext, _: &PermissionsConfig) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[derive(Default, Serialize, Deserialize)]
    struct FirebaseConfig {
        google_service_path: Option<String>,
    }

    impl PluginConfig for FirebaseConfig {
        const NAME: &'static str = "whisker-firebase";
    }

    impl FirebaseConfig {
        fn google_service_path(&mut self, p: impl Into<String>) -> &mut Self {
            self.google_service_path = Some(p.into());
            self
        }
    }

    struct Firebase;
    impl Plugin for Firebase {
        type Config = FirebaseConfig;
        fn apply(&self, _: &mut GenerateContext, _: &FirebaseConfig) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn plugin_call_stores_serialized_config_keyed_by_name() {
        let mut app = AppConfig::default();
        app.plugin::<Firebase>(|c| {
            c.google_service_path("ios/GoogleService-Info.plist");
        });

        assert_eq!(app.plugins.len(), 1);
        let v = app
            .plugins
            .get("whisker-firebase")
            .expect("entry keyed by PluginConfig::NAME");
        assert_eq!(
            v.get("google_service_path").and_then(|x| x.as_str()),
            Some("ios/GoogleService-Info.plist"),
        );
    }

    #[test]
    fn plugin_default_config_round_trips() {
        let mut app = AppConfig::default();
        // closure leaves the config at default — entry should still
        // exist (the plugin was declared, just unconfigured).
        app.plugin::<Firebase>(|_| {});
        let v = app.plugins.get("whisker-firebase").unwrap();
        assert!(v.is_object());
        assert!(v.get("google_service_path").unwrap().is_null());
    }

    #[test]
    fn plugin_call_replaces_prior_entry_for_same_type() {
        let mut app = AppConfig::default();
        app.plugin::<Firebase>(|c| {
            c.google_service_path("old.plist");
        });
        app.plugin::<Firebase>(|c| {
            c.google_service_path("new.plist");
        });

        assert_eq!(app.plugins.len(), 1);
        assert_eq!(
            app.plugins["whisker-firebase"]["google_service_path"],
            "new.plist",
        );
    }

    #[test]
    fn multiple_distinct_plugins_coexist() {
        let mut app = AppConfig::default();
        app.plugin::<Firebase>(|c| {
            c.google_service_path("ios/GoogleService-Info.plist");
        });
        app.plugin::<Permissions>(|c| {
            c.camera_reason("Take photos for the app")
                .add("android.permission.CAMERA");
        });

        assert_eq!(app.plugins.len(), 2);
        let keys: Vec<_> = app.plugins.keys().cloned().collect();
        // BTreeMap → deterministic alphabetical order
        assert_eq!(keys, vec!["whisker-firebase", "whisker-permissions"]);
        assert_eq!(
            app.plugins["whisker-permissions"]["permissions"][0],
            "android.permission.CAMERA",
        );
    }

    #[test]
    fn appconfig_round_trips_through_json_with_plugins_field() {
        let mut app = AppConfig::default();
        app.name("Demo");
        app.plugin::<Firebase>(|c| {
            c.google_service_path("ios/GoogleService-Info.plist");
        });

        let json = serde_json::to_string(&app).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name.as_deref(), Some("Demo"));
        assert_eq!(back.plugins.len(), 1);
        assert!(back.plugins.contains_key("whisker-firebase"));
    }

    #[test]
    fn appconfig_deserializes_without_plugins_field() {
        // Pre-PR-2 wire format: no `plugins` key at all. The
        // `#[serde(default)]` on `plugins` should give us an empty
        // map rather than failing — this is what keeps an
        // already-deployed dev-server compatible with an older
        // probe binary.
        //
        // (`ios` / `android` are pre-PR-2 required fields, so the
        // probe always emits them. Including them keeps this test
        // focused on the plugins-field omission.)
        let json = r#"{"name":"OldApp","ios":{},"android":{}}"#;
        let back: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(back.name.as_deref(), Some("OldApp"));
        assert!(back.plugins.is_empty());
    }
}
