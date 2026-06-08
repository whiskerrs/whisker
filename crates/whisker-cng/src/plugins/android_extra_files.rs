//! `whisker-android-extra-files` — drop arbitrary text files into
//! the generated `gen/android/` tree.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<AndroidExtraFilesConfig>(|c| c
//!     .add("app/google-services.json", json_str)
//!     .add("app/proguard-rules-extra.pro", proguard_text));
//! ```
//!
//! Mirror of [`crate::plugins::ios_extra_files`] for Android.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use whisker_plugin::{FileEntry, GenerateContext, Operation, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
pub struct AndroidExtraFilesConfig {
    /// Path → file. Path is relative to `gen/android/`.
    #[serde(default)]
    pub files: BTreeMap<PathBuf, FileEntry>,
}

impl AndroidExtraFilesConfig {
    pub fn add(&mut self, path: impl Into<PathBuf>, contents: impl Into<String>) -> &mut Self {
        self.files.insert(
            path.into(),
            FileEntry {
                contents: contents.into(),
                mode: None,
            },
        );
        self
    }
    pub fn add_with_mode(
        &mut self,
        path: impl Into<PathBuf>,
        contents: impl Into<String>,
        mode: u32,
    ) -> &mut Self {
        self.files.insert(
            path.into(),
            FileEntry {
                contents: contents.into(),
                mode: Some(mode),
            },
        );
        self
    }
}

impl PluginConfig for AndroidExtraFilesConfig {
    const NAME: &'static str = "whisker-android-extra-files";
}

pub struct AndroidExtraFilesPlugin;

impl Plugin for AndroidExtraFilesPlugin {
    type Config = AndroidExtraFilesConfig;
    fn apply(
        &self,
        ctx: &mut GenerateContext,
        cfg: &AndroidExtraFilesConfig,
    ) -> anyhow::Result<()> {
        let Some(android) = ctx.android.as_mut() else {
            return Ok(());
        };
        if cfg.files.is_empty() {
            return Ok(());
        }
        let count = cfg.files.len();
        for (path, entry) in &cfg.files {
            android.extra_files.insert(path.clone(), entry.clone());
        }
        ctx.journal.record(
            AndroidExtraFilesConfig::NAME,
            Target::Android,
            "extra_files",
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
        AndroidExtraFilesPlugin
            .apply(&mut ctx, &AndroidExtraFilesConfig::default())
            .unwrap();
        assert!(ctx.android.unwrap().extra_files.is_empty());
    }

    #[test]
    fn populated_config_writes_each_file_to_ir() {
        let mut cfg = AndroidExtraFilesConfig::default();
        cfg.add("app/google-services.json", "{\"foo\": 1}")
            .add_with_mode("scripts/build.sh", "#!/bin/sh\n", 0o755);
        let mut ctx = ctx_with_android();
        AndroidExtraFilesPlugin.apply(&mut ctx, &cfg).unwrap();
        let files = ctx.android.unwrap().extra_files;
        assert_eq!(files.len(), 2);
        assert_eq!(
            files[&PathBuf::from("app/google-services.json")].contents,
            "{\"foo\": 1}",
        );
        assert_eq!(files[&PathBuf::from("scripts/build.sh")].mode, Some(0o755),);
    }
}
