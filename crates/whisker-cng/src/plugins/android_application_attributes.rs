//! `whisker-android-application-attributes` — set attributes on the
//! generated `AndroidManifest.xml`'s `<application>` *tag itself*
//! (not `<meta-data>` child elements).
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<AndroidApplicationAttributes>(|c| c
//!     .set("android:enableOnBackInvokedCallback", "true")
//!     .set("android:largeHeap", "true"));
//! ```
//!
//! The most common reason apps reach for this is opting into the
//! Android 13+ predictive-back API
//! (`android:enableOnBackInvokedCallback="true"`), without which the
//! OS won't deliver the `handleOnBackStarted` / `handleOnBackProgressed`
//! preview callbacks. Distinct from
//! [`android_meta_data`](super::android_meta_data): that adds
//! `<meta-data>` *children*; this sets *attributes* on the
//! `<application>` element.

use serde::{Deserialize, Serialize};
use whisker_plugin::{
    ApplicationAttribute, GenerateContext, Operation, Plugin, PluginConfig, Target,
};

#[derive(Default, Serialize, Deserialize)]
pub struct AndroidApplicationAttributesConfig {
    /// `(name, value)` pairs rendered as `android:name="value"` on the
    /// `<application>` tag. Dedup'd by name at render time (last writer
    /// wins). Stored ordered for a deterministic manifest.
    #[serde(default)]
    pub attributes: Vec<ApplicationAttribute>,
}

impl AndroidApplicationAttributesConfig {
    /// Set an `<application>` attribute. `name` should include the
    /// `android:` namespace prefix (e.g.
    /// `"android:enableOnBackInvokedCallback"`).
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.attributes.push(ApplicationAttribute {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

impl PluginConfig for AndroidApplicationAttributesConfig {
    const NAME: &'static str = "whisker-android-application-attributes";
}

pub struct AndroidApplicationAttributes;

impl Plugin for AndroidApplicationAttributes {
    type Config = AndroidApplicationAttributesConfig;

    fn apply(
        &self,
        ctx: &mut GenerateContext,
        cfg: &AndroidApplicationAttributesConfig,
    ) -> anyhow::Result<()> {
        let Some(android) = ctx.android.as_mut() else {
            return Ok(());
        };
        if cfg.attributes.is_empty() {
            return Ok(());
        }
        let count = cfg.attributes.len();
        android
            .manifest
            .application_attributes
            .extend(cfg.attributes.clone());
        ctx.journal.record(
            AndroidApplicationAttributesConfig::NAME,
            Target::Android,
            "manifest.application_attributes",
            Operation::ArrayPush { count },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_plugin::AndroidProjectIr;

    fn ctx_with_android() -> GenerateContext {
        GenerateContext {
            android: Some(AndroidProjectIr::default()),
            ..Default::default()
        }
    }

    #[test]
    fn default_config_contributes_nothing() {
        let mut ctx = ctx_with_android();
        AndroidApplicationAttributes
            .apply(&mut ctx, &AndroidApplicationAttributesConfig::default())
            .unwrap();
        assert!(
            ctx.android
                .unwrap()
                .manifest
                .application_attributes
                .is_empty()
        );
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn populated_config_appends_each_attribute() {
        let mut cfg = AndroidApplicationAttributesConfig::default();
        cfg.set("android:enableOnBackInvokedCallback", "true")
            .set("android:largeHeap", "true");
        let mut ctx = ctx_with_android();
        AndroidApplicationAttributes.apply(&mut ctx, &cfg).unwrap();
        let attrs = ctx.android.unwrap().manifest.application_attributes;
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].name, "android:enableOnBackInvokedCallback");
        assert_eq!(attrs[0].value, "true");
        assert_eq!(attrs[1].name, "android:largeHeap");
    }

    #[test]
    fn one_array_push_event_per_invocation() {
        let mut cfg = AndroidApplicationAttributesConfig::default();
        cfg.set("a", "1").set("b", "2");
        let mut ctx = ctx_with_android();
        AndroidApplicationAttributes.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(ctx.journal.records.len(), 1);
        let r = &ctx.journal.records[0];
        assert_eq!(r.plugin, "whisker-android-application-attributes");
        assert!(matches!(r.operation, Operation::ArrayPush { count: 2 }));
    }
}
