//! `whisker-info-plist-extra` — append arbitrary `(key, string)`
//! pairs to the iOS `Info.plist`.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<InfoPlistExtra>(|c| c
//!     .add("NSCameraUsageDescription", "Take photos for posts.")
//!     .add("LSApplicationQueriesSchemes", "comgooglemaps"));
//! ```
//!
//! The plugin writes each `(key, value)` straight into
//! `ctx.ios.info_plist` as a `PlistValue::String`. Two different
//! plugins (built-in or 3rd-party) writing the same key surfaces as
//! a conflict via the [`Operation::Set`] / mutation-journal path —
//! this plugin is no different from a 3rd-party one in that
//! respect.
//!
//! ## Why string-only
//!
//! 90%+ of Info.plist additions are either privacy strings or
//! capability flags expressed as strings. Dict / array values
//! exist but are rarer and not yet shipped — a future built-in
//! (`whisker-info-plist-structured`) can extend the schema when
//! the first real consumer asks.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use whisker_plugin::{GenerateContext, Operation, PlistValue, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
pub struct InfoPlistExtraConfig {
    /// `(key, value)` pairs added to `Info.plist`. `BTreeMap` for
    /// deterministic iteration order — the engine's fingerprint
    /// hashes the IR, and `HashMap` random ordering would invalidate
    /// the skip path.
    #[serde(default)]
    pub entries: BTreeMap<String, String>,
}

impl InfoPlistExtraConfig {
    /// Add (or replace) one `(key, value)` pair. Returns `&mut Self`
    /// for fluent chaining inside the `app.plugin::<…>(|c| …)`
    /// closure.
    pub fn add(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.entries.insert(key.into(), value.into());
        self
    }
}

impl PluginConfig for InfoPlistExtraConfig {
    const NAME: &'static str = "whisker-info-plist-extra";
}

pub struct InfoPlistExtra;

impl Plugin for InfoPlistExtra {
    type Config = InfoPlistExtraConfig;

    fn apply(&self, ctx: &mut GenerateContext, cfg: &InfoPlistExtraConfig) -> anyhow::Result<()> {
        let Some(ios) = ctx.ios.as_mut() else {
            // Target not enabled — nothing to do.
            return Ok(());
        };
        for (key, value) in &cfg.entries {
            ios.info_plist
                .insert(key.clone(), PlistValue::String(value.clone()));
            ctx.journal.record(
                InfoPlistExtraConfig::NAME,
                Target::Ios,
                &format!("info_plist.{key}"),
                Operation::Set,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_plugin::IosProjectIr;

    fn ctx_with_ios() -> GenerateContext {
        GenerateContext {
            ios: Some(IosProjectIr::default()),
            ..Default::default()
        }
    }

    #[test]
    fn default_config_contributes_nothing() {
        let mut ctx = ctx_with_ios();
        InfoPlistExtra
            .apply(&mut ctx, &InfoPlistExtraConfig::default())
            .unwrap();
        assert!(ctx.ios.unwrap().info_plist.is_empty());
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn populated_config_writes_each_entry_to_info_plist() {
        let mut cfg = InfoPlistExtraConfig::default();
        cfg.add("NSCameraUsageDescription", "Take photos.")
            .add("LSApplicationQueriesSchemes", "comgooglemaps");
        let mut ctx = ctx_with_ios();
        InfoPlistExtra.apply(&mut ctx, &cfg).unwrap();
        let plist = ctx.ios.unwrap().info_plist;
        assert_eq!(
            plist["NSCameraUsageDescription"],
            PlistValue::String("Take photos.".into()),
        );
        assert_eq!(
            plist["LSApplicationQueriesSchemes"],
            PlistValue::String("comgooglemaps".into()),
        );
    }

    #[test]
    fn each_entry_records_a_journal_event() {
        let mut cfg = InfoPlistExtraConfig::default();
        cfg.add("Key1", "v1").add("Key2", "v2");
        let mut ctx = ctx_with_ios();
        InfoPlistExtra.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(ctx.journal.records.len(), 2);
        for r in &ctx.journal.records {
            assert_eq!(r.plugin, "whisker-info-plist-extra");
            assert_eq!(r.target, Target::Ios);
            assert!(matches!(r.operation, Operation::Set));
        }
    }

    #[test]
    fn no_ios_target_means_no_op() {
        let mut cfg = InfoPlistExtraConfig::default();
        cfg.add("k", "v");
        let mut ctx = GenerateContext::default(); // ios = None
        InfoPlistExtra.apply(&mut ctx, &cfg).unwrap();
        assert!(ctx.journal.records.is_empty());
    }
}
