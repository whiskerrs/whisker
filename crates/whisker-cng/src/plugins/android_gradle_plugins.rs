//! `whisker-gradle-plugins` — append Gradle `plugins { id(...) }`
//! entries to the generated `app/build.gradle.kts`.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<GradlePlugins>(|c| c
//!     .add("com.google.gms.google-services")
//!     .add_raw("id(\"com.android.dynamic-feature\") version \"8.5.0\""));
//! ```
//!
//! The plugin appends each entry to `ctx.android.gradle.apply_plugins`.
//! The renderer wraps simple `id`-only strings in `id("...")`; raw
//! `id(...)` lines pass through verbatim so users can attach
//! `version "X"` / `apply false` / etc.

use serde::{Deserialize, Serialize};
use whisker_plugin::{GenerateContext, Operation, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
pub struct GradlePluginsConfig {
    /// Plugin identifiers to apply. Each entry is either:
    ///   - a bare gradle plugin id (e.g. `"com.google.gms.google-services"`),
    ///     which the renderer wraps in `id("…")`
    ///   - a raw `id(...)` DSL line (starts with `id(`), which the
    ///     renderer emits verbatim. Use this for `version "…"` /
    ///     `apply false` qualifiers.
    #[serde(default)]
    pub entries: Vec<String>,
}

impl GradlePluginsConfig {
    /// Bare plugin id. Renderer wraps it in `id("…")`.
    pub fn add(&mut self, id: impl Into<String>) -> &mut Self {
        self.entries.push(id.into());
        self
    }
    /// Raw `id(...)` line. Renderer emits verbatim.
    pub fn add_raw(&mut self, line: impl Into<String>) -> &mut Self {
        self.entries.push(line.into());
        self
    }
}

impl PluginConfig for GradlePluginsConfig {
    const NAME: &'static str = "whisker-gradle-plugins";
}

pub struct GradlePlugins;

impl Plugin for GradlePlugins {
    type Config = GradlePluginsConfig;
    fn apply(&self, ctx: &mut GenerateContext, cfg: &GradlePluginsConfig) -> anyhow::Result<()> {
        let Some(android) = ctx.android.as_mut() else {
            return Ok(());
        };
        if cfg.entries.is_empty() {
            return Ok(());
        }
        let count = cfg.entries.len();
        android.gradle.apply_plugins.extend(cfg.entries.clone());
        ctx.journal.record(
            GradlePluginsConfig::NAME,
            Target::Android,
            "gradle.apply_plugins",
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
        GradlePlugins
            .apply(&mut ctx, &GradlePluginsConfig::default())
            .unwrap();
        assert!(ctx.android.unwrap().gradle.apply_plugins.is_empty());
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn populated_config_appends_each_entry_preserving_order() {
        let mut cfg = GradlePluginsConfig::default();
        cfg.add("com.google.gms.google-services")
            .add_raw("id(\"com.android.dynamic-feature\") version \"8.5.0\"");
        let mut ctx = ctx_with_android();
        GradlePlugins.apply(&mut ctx, &cfg).unwrap();
        let plugins = ctx.android.unwrap().gradle.apply_plugins;
        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[0], "com.google.gms.google-services");
        assert_eq!(
            plugins[1],
            "id(\"com.android.dynamic-feature\") version \"8.5.0\"",
        );
    }

    #[test]
    fn one_array_push_event_per_invocation() {
        let mut cfg = GradlePluginsConfig::default();
        cfg.add("a").add("b").add("c");
        let mut ctx = ctx_with_android();
        GradlePlugins.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(ctx.journal.records.len(), 1);
        let r = &ctx.journal.records[0];
        assert_eq!(r.plugin, "whisker-gradle-plugins");
        assert!(matches!(r.operation, Operation::ArrayPush { count: 3 }));
    }
}
