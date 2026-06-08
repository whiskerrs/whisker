//! `whisker-gradle-dependencies` — append raw Gradle dependency
//! lines to `app/build.gradle.kts`'s `dependencies { }` block.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<GradleDependenciesConfig>(|c| c
//!     .add("implementation(\"com.google.firebase:firebase-analytics:21.5.0\")")
//!     .add("kapt(\"androidx.room:room-compiler:2.6.0\")"));
//! ```
//!
//! Each entry is a raw DSL line. The renderer emits it verbatim
//! inside `dependencies { }`. Letting users pass the full line
//! keeps `implementation` / `api` / `kapt` / `runtimeOnly` / etc.
//! distinctions expressible without modelling Gradle's full
//! configuration grammar.

use serde::{Deserialize, Serialize};
use whisker_plugin::{GenerateContext, Operation, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
pub struct GradleDependenciesConfig {
    /// Raw `<configuration>("coordinate")` lines, e.g.
    /// `"implementation(\"com.google.firebase:firebase-analytics:21.5.0\")"`.
    /// Renderer emits each verbatim inside `dependencies { }`.
    #[serde(default)]
    pub entries: Vec<String>,
}

impl GradleDependenciesConfig {
    pub fn add(&mut self, line: impl Into<String>) -> &mut Self {
        self.entries.push(line.into());
        self
    }
}

impl PluginConfig for GradleDependenciesConfig {
    const NAME: &'static str = "whisker-gradle-dependencies";
}

pub struct GradleDependenciesPlugin;

impl Plugin for GradleDependenciesPlugin {
    type Config = GradleDependenciesConfig;
    fn apply(
        &self,
        ctx: &mut GenerateContext,
        cfg: &GradleDependenciesConfig,
    ) -> anyhow::Result<()> {
        let Some(android) = ctx.android.as_mut() else {
            return Ok(());
        };
        if cfg.entries.is_empty() {
            return Ok(());
        }
        let count = cfg.entries.len();
        android.gradle.dependencies.extend(cfg.entries.clone());
        ctx.journal.record(
            GradleDependenciesConfig::NAME,
            Target::Android,
            "gradle.dependencies",
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
        GradleDependenciesPlugin
            .apply(&mut ctx, &GradleDependenciesConfig::default())
            .unwrap();
        assert!(ctx.android.unwrap().gradle.dependencies.is_empty());
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn populated_config_appends_each_entry_preserving_order() {
        let mut cfg = GradleDependenciesConfig::default();
        cfg.add("implementation(\"com.google.firebase:firebase-analytics:21.5.0\")")
            .add("kapt(\"androidx.room:room-compiler:2.6.0\")");
        let mut ctx = ctx_with_android();
        GradleDependenciesPlugin.apply(&mut ctx, &cfg).unwrap();
        let deps = ctx.android.unwrap().gradle.dependencies;
        assert_eq!(deps.len(), 2);
        assert!(deps[0].starts_with("implementation("));
        assert!(deps[1].starts_with("kapt("));
    }

    #[test]
    fn one_array_push_event_per_invocation() {
        let mut cfg = GradleDependenciesConfig::default();
        cfg.add("implementation(\"a:b:1\")")
            .add("implementation(\"c:d:1\")");
        let mut ctx = ctx_with_android();
        GradleDependenciesPlugin.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(ctx.journal.records.len(), 1);
        let r = &ctx.journal.records[0];
        assert_eq!(r.plugin, "whisker-gradle-dependencies");
        assert!(matches!(r.operation, Operation::ArrayPush { count: 2 }));
    }
}
