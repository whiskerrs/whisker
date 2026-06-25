//! Whisker build plugin for the router.
//!
//! The router's Android predictive-back gesture ([`AndroidPredictiveBack`])
//! relies on the OS delivering the `handleOnBackStarted` /
//! `handleOnBackProgressed` preview callbacks. Android only delivers those
//! when the app's manifest opts in with
//! `android:enableOnBackInvokedCallback="true"` on `<application>`;
//! without it the system runs the *legacy* back path (commit only, no
//! interactive preview).
//!
//! Rather than make every app that uses the router edit its manifest, the
//! router declares the requirement itself — the same idea as an Expo
//! config plugin's `withAndroidManifest`. This [`Plugin`] is discovered via
//! the `[package.metadata.whisker.plugins.whisker-router]` table in this
//! crate's `Cargo.toml`; the engine builds the `whisker-router-plugin`
//! binary and runs it on every `whisker build` / `whisker run`, so any app
//! depending on `whisker-router` automatically gets the attribute.
//!
//! [`AndroidPredictiveBack`]: crate::render::AndroidPredictiveBack

use serde::{Deserialize, Serialize};
use whisker_plugin::{
    ApplicationAttribute, GenerateContext, Operation, Plugin, PluginConfig, Target,
};

/// Config for [`RouterPlugin`]. The router's manifest requirement is
/// unconditional, so there is nothing to configure — the struct exists
/// only to satisfy the [`PluginConfig`] bound and to name the plugin.
#[derive(Default, Serialize, Deserialize)]
pub struct RouterPluginConfig {}

impl PluginConfig for RouterPluginConfig {
    const NAME: &'static str = "whisker-router";
}

/// The router build plugin: declares the Android manifest requirements
/// the router's gestures need.
pub struct RouterPlugin;

impl Plugin for RouterPlugin {
    type Config = RouterPluginConfig;

    fn apply(&self, ctx: &mut GenerateContext, _cfg: &RouterPluginConfig) -> anyhow::Result<()> {
        let Some(android) = ctx.android.as_mut() else {
            return Ok(());
        };
        // Opt into the Android 13+ predictive-back API so the OS delivers
        // the interactive preview callbacks `AndroidPredictiveBack` reads.
        android
            .manifest
            .application_attributes
            .push(ApplicationAttribute {
                name: "android:enableOnBackInvokedCallback".into(),
                value: "true".into(),
            });
        ctx.journal.record(
            RouterPluginConfig::NAME,
            Target::Android,
            "manifest.application_attributes",
            Operation::ArrayPush { count: 1 },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_plugin::AndroidProjectIr;

    #[test]
    fn adds_enable_on_back_invoked_callback() {
        let mut ctx = GenerateContext {
            android: Some(AndroidProjectIr::default()),
            ..Default::default()
        };
        RouterPlugin
            .apply(&mut ctx, &RouterPluginConfig::default())
            .unwrap();
        let attrs = ctx.android.unwrap().manifest.application_attributes;
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].name, "android:enableOnBackInvokedCallback");
        assert_eq!(attrs[0].value, "true");
    }

    #[test]
    fn no_android_target_is_a_noop() {
        let mut ctx = GenerateContext::default(); // no android IR
        RouterPlugin
            .apply(&mut ctx, &RouterPluginConfig::default())
            .unwrap();
        assert!(ctx.journal.records.is_empty());
    }
}
