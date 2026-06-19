//! `whisker-android-meta-data` — append `<meta-data>` rows inside
//! the generated `AndroidManifest.xml`'s `<application>` block.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<AndroidMetaData>(|c| c
//!     .add("com.google.firebase.messaging.default_notification_icon",
//!          "@drawable/ic_notification")
//!     .add("com.google.android.geo.API_KEY", "AIza..."));
//! ```
//!
//! Required by Firebase, Google Maps, App Links host declarations
//! and most other 1st-party Google SDKs that need to surface a
//! manifest-time value to the runtime.

use serde::{Deserialize, Serialize};
use whisker_plugin::{GenerateContext, MetaDataEntry, Operation, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
pub struct AndroidMetaDataConfig {
    /// `(name, value)` pairs the renderer emits as
    /// `<meta-data android:name="…" android:value="…"/>` inside
    /// `<application>`. Stored ordered so multiple plugins
    /// contributing entries produce a deterministic manifest.
    #[serde(default)]
    pub entries: Vec<MetaDataEntry>,
}

impl AndroidMetaDataConfig {
    pub fn add(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.entries.push(MetaDataEntry {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

impl PluginConfig for AndroidMetaDataConfig {
    const NAME: &'static str = "whisker-android-meta-data";
}

pub struct AndroidMetaData;

impl Plugin for AndroidMetaData {
    type Config = AndroidMetaDataConfig;

    fn apply(&self, ctx: &mut GenerateContext, cfg: &AndroidMetaDataConfig) -> anyhow::Result<()> {
        let Some(android) = ctx.android.as_mut() else {
            return Ok(());
        };
        if cfg.entries.is_empty() {
            return Ok(());
        }
        let count = cfg.entries.len();
        android
            .manifest
            .application_meta_data
            .extend(cfg.entries.clone());
        ctx.journal.record(
            AndroidMetaDataConfig::NAME,
            Target::Android,
            "manifest.application_meta_data",
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
        AndroidMetaData
            .apply(&mut ctx, &AndroidMetaDataConfig::default())
            .unwrap();
        assert!(
            ctx.android
                .unwrap()
                .manifest
                .application_meta_data
                .is_empty()
        );
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn populated_config_appends_each_entry() {
        let mut cfg = AndroidMetaDataConfig::default();
        cfg.add(
            "com.google.firebase.messaging.default_notification_icon",
            "@drawable/ic_notification",
        )
        .add("com.google.android.geo.API_KEY", "AIza...");
        let mut ctx = ctx_with_android();
        AndroidMetaData.apply(&mut ctx, &cfg).unwrap();
        let entries = ctx.android.unwrap().manifest.application_meta_data;
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].name,
            "com.google.firebase.messaging.default_notification_icon",
        );
        assert_eq!(entries[1].name, "com.google.android.geo.API_KEY");
        assert_eq!(entries[1].value, "AIza...");
    }

    #[test]
    fn one_array_push_event_per_invocation() {
        let mut cfg = AndroidMetaDataConfig::default();
        cfg.add("a", "1").add("b", "2");
        let mut ctx = ctx_with_android();
        AndroidMetaData.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(ctx.journal.records.len(), 1);
        let r = &ctx.journal.records[0];
        assert_eq!(r.plugin, "whisker-android-meta-data");
        assert!(matches!(r.operation, Operation::ArrayPush { count: 2 }));
    }
}
