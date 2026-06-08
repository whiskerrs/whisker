//! `whisker-android-permissions` — append `<uses-permission>` rows
//! to the generated `AndroidManifest.xml`.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<AndroidPermissions>(|c| c
//!     .add("android.permission.CAMERA")
//!     .add("android.permission.RECORD_AUDIO"));
//! ```
//!
//! The plugin appends each permission to
//! `ctx.android.manifest.permissions`. Duplicates within a single
//! `add(...)` chain stay duplicated — the renderer dedups at write
//! time so the manifest doesn't carry redundant entries even when
//! several plugins contribute the same permission.

use serde::{Deserialize, Serialize};
use whisker_plugin::{GenerateContext, Operation, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
pub struct AndroidPermissionsConfig {
    /// Permission strings exactly as they appear in
    /// `android:name="…"`. e.g. `"android.permission.CAMERA"`.
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl AndroidPermissionsConfig {
    pub fn add(&mut self, name: impl Into<String>) -> &mut Self {
        self.permissions.push(name.into());
        self
    }
}

impl PluginConfig for AndroidPermissionsConfig {
    const NAME: &'static str = "whisker-android-permissions";
}

pub struct AndroidPermissions;

impl Plugin for AndroidPermissions {
    type Config = AndroidPermissionsConfig;

    fn apply(
        &self,
        ctx: &mut GenerateContext,
        cfg: &AndroidPermissionsConfig,
    ) -> anyhow::Result<()> {
        let Some(android) = ctx.android.as_mut() else {
            return Ok(());
        };
        if cfg.permissions.is_empty() {
            return Ok(());
        }
        let count = cfg.permissions.len();
        android.manifest.permissions.extend(cfg.permissions.clone());
        ctx.journal.record(
            AndroidPermissionsConfig::NAME,
            Target::Android,
            "manifest.permissions",
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
        AndroidPermissions
            .apply(&mut ctx, &AndroidPermissionsConfig::default())
            .unwrap();
        assert!(ctx.android.unwrap().manifest.permissions.is_empty());
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn populated_config_appends_each_permission() {
        let mut cfg = AndroidPermissionsConfig::default();
        cfg.add("android.permission.CAMERA")
            .add("android.permission.RECORD_AUDIO");
        let mut ctx = ctx_with_android();
        AndroidPermissions.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(
            ctx.android.unwrap().manifest.permissions,
            vec![
                "android.permission.CAMERA".to_string(),
                "android.permission.RECORD_AUDIO".to_string(),
            ],
        );
    }

    #[test]
    fn one_array_push_event_per_invocation() {
        let mut cfg = AndroidPermissionsConfig::default();
        cfg.add("a").add("b").add("c");
        let mut ctx = ctx_with_android();
        AndroidPermissions.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(ctx.journal.records.len(), 1);
        let r = &ctx.journal.records[0];
        assert_eq!(r.plugin, "whisker-android-permissions");
        assert_eq!(r.target, Target::Android);
        assert!(matches!(r.operation, Operation::ArrayPush { count: 3 }));
    }

    #[test]
    fn no_android_target_means_no_op() {
        let mut cfg = AndroidPermissionsConfig::default();
        cfg.add("android.permission.CAMERA");
        let mut ctx = GenerateContext::default();
        AndroidPermissions.apply(&mut ctx, &cfg).unwrap();
        assert!(ctx.journal.records.is_empty());
    }
}
