//! `whisker-ios-pbxproj-ops` — request structural mutations
//! against the generated Xcode `project.pbxproj`.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<IosPbxprojOpsCfg>(|c| c
//!     .add_resource("GoogleService-Info.plist")
//!     .link_system_framework("AVFoundation.framework")
//!     .set_build_setting("OTHER_LDFLAGS", "-ObjC"));
//! ```
//!
//! Each op is appended to `ctx.ios.pbxproj_ops`; the renderer
//! produces the matching pbxproj entries (PBXFileReference,
//! PBXBuildFile, PBXGroup membership, build-phase files list,
//! buildSettings dict) at sync time. The plugin doesn't touch the
//! pbxproj text directly — that work happens in
//! `crate::ios::template_vars`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use whisker_plugin::{GenerateContext, Operation, PbxprojOp, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
pub struct IosPbxprojOpsCfg {
    #[serde(default)]
    pub ops: Vec<PbxprojOp>,
}

impl IosPbxprojOpsCfg {
    /// Register a file as a Resource (bundled into the `.app`).
    /// Use for `GoogleService-Info.plist` etc. The file itself
    /// should already exist on disk — drop it via
    /// `whisker-ios-extra-files` first.
    pub fn add_resource(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.ops.push(PbxprojOp::AddResource { path: path.into() });
        self
    }
    /// Add a `.swift` / `.m` / `.mm` to the app target's Sources
    /// compile phase.
    pub fn add_source(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.ops.push(PbxprojOp::AddSource { path: path.into() });
        self
    }
    /// Append `KEY = VALUE;` to both Debug and Release target
    /// build settings.
    pub fn set_build_setting(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> &mut Self {
        self.ops.push(PbxprojOp::SetBuildSetting {
            key: key.into(),
            value: value.into(),
        });
        self
    }
    /// Add a system framework (e.g. `"AVFoundation.framework"`)
    /// to the app target's Frameworks link phase.
    pub fn link_system_framework(&mut self, name: impl Into<String>) -> &mut Self {
        self.ops
            .push(PbxprojOp::LinkSystemFramework { name: name.into() });
        self
    }
}

impl PluginConfig for IosPbxprojOpsCfg {
    const NAME: &'static str = "whisker-ios-pbxproj-ops";
}

pub struct IosPbxprojOpsPlugin;

impl Plugin for IosPbxprojOpsPlugin {
    type Config = IosPbxprojOpsCfg;
    fn apply(&self, ctx: &mut GenerateContext, cfg: &IosPbxprojOpsCfg) -> anyhow::Result<()> {
        let Some(ios) = ctx.ios.as_mut() else {
            return Ok(());
        };
        if cfg.ops.is_empty() {
            return Ok(());
        }
        let count = cfg.ops.len();
        ios.pbxproj_ops.extend(cfg.ops.clone());
        ctx.journal.record(
            IosPbxprojOpsCfg::NAME,
            Target::Ios,
            "pbxproj_ops",
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
        IosPbxprojOpsPlugin
            .apply(&mut ctx, &IosPbxprojOpsCfg::default())
            .unwrap();
        assert!(ctx.ios.unwrap().pbxproj_ops.is_empty());
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn populated_config_appends_each_op_preserving_order() {
        let mut cfg = IosPbxprojOpsCfg::default();
        cfg.add_resource("GoogleService-Info.plist")
            .link_system_framework("AVFoundation.framework")
            .set_build_setting("OTHER_LDFLAGS", "-ObjC");
        let mut ctx = ctx_with_ios();
        IosPbxprojOpsPlugin.apply(&mut ctx, &cfg).unwrap();
        let ops = ctx.ios.unwrap().pbxproj_ops;
        assert_eq!(ops.len(), 3);
        match &ops[0] {
            PbxprojOp::AddResource { path } => {
                assert_eq!(path, &PathBuf::from("GoogleService-Info.plist"));
            }
            other => panic!("unexpected op: {other:?}"),
        }
        match &ops[1] {
            PbxprojOp::LinkSystemFramework { name } => {
                assert_eq!(name, "AVFoundation.framework");
            }
            other => panic!("unexpected op: {other:?}"),
        }
        match &ops[2] {
            PbxprojOp::SetBuildSetting { key, value } => {
                assert_eq!(key, "OTHER_LDFLAGS");
                assert_eq!(value, "-ObjC");
            }
            other => panic!("unexpected op: {other:?}"),
        }
    }

    #[test]
    fn one_array_push_event_per_invocation() {
        let mut cfg = IosPbxprojOpsCfg::default();
        cfg.add_resource("a").add_source("b.swift");
        let mut ctx = ctx_with_ios();
        IosPbxprojOpsPlugin.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(ctx.journal.records.len(), 1);
        let r = &ctx.journal.records[0];
        assert_eq!(r.plugin, "whisker-ios-pbxproj-ops");
        assert!(matches!(r.operation, Operation::ArrayPush { count: 2 }));
    }
}
