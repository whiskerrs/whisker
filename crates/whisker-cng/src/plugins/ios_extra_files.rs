//! `whisker-ios-extra-files` — drop arbitrary text files into the
//! generated `gen/ios/` tree.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<IosExtraFiles>(|c| c
//!     .add("Sources/Helper.swift", SWIFT_SRC)
//!     .add("Resources/Config.json", json_str)
//!     .add_with_mode("Scripts/run.sh", SCRIPT, 0o755));
//! ```
//!
//! The plugin writes each `(relative_path, contents)` pair into
//! `ctx.ios.extra_files`. The renderer copies them into
//! `gen/ios/<relative_path>` after the template-driven files have
//! been written. Paths are validated to be relative and free of
//! `..` traversal — a plugin can't escape the gen tree.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use whisker_plugin::{FileEntry, GenerateContext, Operation, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
pub struct IosExtraFilesConfig {
    /// Path → file. Path is relative to `gen/ios/`. `BTreeMap` for
    /// deterministic iteration order — the fingerprint hashes the
    /// IR and `HashMap` random ordering would break the skip path.
    #[serde(default)]
    pub files: BTreeMap<PathBuf, FileEntry>,
}

impl IosExtraFilesConfig {
    /// Add (or replace) one file with default `0o644` permissions.
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
    /// Add (or replace) one file with explicit POSIX mode bits
    /// (e.g. `0o755` for an executable script).
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

impl PluginConfig for IosExtraFilesConfig {
    const NAME: &'static str = "whisker-ios-extra-files";
}

pub struct IosExtraFiles;

impl Plugin for IosExtraFiles {
    type Config = IosExtraFilesConfig;
    fn apply(&self, ctx: &mut GenerateContext, cfg: &IosExtraFilesConfig) -> anyhow::Result<()> {
        let Some(ios) = ctx.ios.as_mut() else {
            return Ok(());
        };
        if cfg.files.is_empty() {
            return Ok(());
        }
        let count = cfg.files.len();
        for (path, entry) in &cfg.files {
            ios.extra_files.insert(path.clone(), entry.clone());
        }
        ctx.journal.record(
            IosExtraFilesConfig::NAME,
            Target::Ios,
            "extra_files",
            Operation::ArrayPush { count },
        );
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
        IosExtraFiles
            .apply(&mut ctx, &IosExtraFilesConfig::default())
            .unwrap();
        assert!(ctx.ios.unwrap().extra_files.is_empty());
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn populated_config_writes_each_file_to_ir() {
        let mut cfg = IosExtraFilesConfig::default();
        cfg.add("Sources/Helper.swift", "// helper").add_with_mode(
            "Scripts/run.sh",
            "#!/bin/sh\necho ok\n",
            0o755,
        );
        let mut ctx = ctx_with_ios();
        IosExtraFiles.apply(&mut ctx, &cfg).unwrap();
        let files = ctx.ios.unwrap().extra_files;
        assert_eq!(files.len(), 2);
        let helper = &files[&PathBuf::from("Sources/Helper.swift")];
        assert_eq!(helper.contents, "// helper");
        assert!(helper.mode.is_none());
        let script = &files[&PathBuf::from("Scripts/run.sh")];
        assert_eq!(script.mode, Some(0o755));
    }

    #[test]
    fn one_array_push_event_per_invocation() {
        let mut cfg = IosExtraFilesConfig::default();
        cfg.add("a.swift", "").add("b.swift", "").add("c.swift", "");
        let mut ctx = ctx_with_ios();
        IosExtraFiles.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(ctx.journal.records.len(), 1);
        let r = &ctx.journal.records[0];
        assert_eq!(r.plugin, "whisker-ios-extra-files");
        assert!(matches!(r.operation, Operation::ArrayPush { count: 3 }));
    }

    #[test]
    fn no_ios_target_means_no_op() {
        let mut cfg = IosExtraFilesConfig::default();
        cfg.add("a.swift", "");
        let mut ctx = GenerateContext::default();
        IosExtraFiles.apply(&mut ctx, &cfg).unwrap();
        assert!(ctx.journal.records.is_empty());
    }
}
