//! Render the iOS host project under `gen/ios/` from an
//! [`AppConfig`].
//!
//! The output is a complete Xcode project — `.pbxproj` is rendered
//! directly from a template, no `xcodegen` step:
//!
//! ```text
//! gen/ios/
//! ├── <scheme>.xcodeproj/
//! │   ├── project.pbxproj
//! │   └── project.xcworkspace/
//! │       └── contents.xcworkspacedata
//! ├── Info.plist
//! └── Sources/AppDelegate.swift
//! ```
//!
//! Why direct pbxproj rendering: avoids the `xcodegen` runtime
//! dependency. xcodegen was useful to spit out a baseline pbxproj
//! once, after which we templatize and check it into the crate.
//! Subsequent renders are pure string substitution. Same pattern
//! Expo uses for its prebuild bare workflow.
//!
//! Trade-off: we own the pbxproj's compatibility with future Xcode
//! versions. `objectVersion = 77` is the current Xcode 15+ format;
//! if Xcode N+1 demands a new objectVersion, regenerate the
//! template via xcodegen once and re-templatize.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use whisker_app_config::AppConfig;

use crate::fingerprint;
use crate::render::render;

const PBXPROJ: &str =
    include_str!("templates/ios/Project.xcodeproj/project.pbxproj");
const XCWORKSPACEDATA: &str = include_str!(
    "templates/ios/Project.xcodeproj/project.xcworkspace/contents.xcworkspacedata"
);
const INFO_PLIST: &str = include_str!("templates/ios/Info.plist");
const APP_DELEGATE_SWIFT: &str = include_str!("templates/ios/Sources/AppDelegate.swift");

#[derive(Debug, Clone, serde::Serialize)]
pub struct IosInputs {
    pub app_name: String,
    pub version: String,
    pub build_number: u32,
    pub scheme: String,
    pub bundle_id: String,
    pub deployment_target: String,
    /// Path to the WhiskerRuntime SPM package — typically
    /// `<workspace>/native/ios`. Written into the rendered pbxproj
    /// as the `XCLocalSwiftPackageReference.relativePath` value
    /// (Xcode accepts an absolute path there) and as the `path` of
    /// the synthetic Packages-group `PBXFileReference` (with
    /// `sourceTree = "<absolute>"`). Both should be absolute so the
    /// generated project is portable regardless of where the
    /// consuming crate sits relative to the workspace root.
    pub whisker_runtime_path: PathBuf,
    pub template_version: u32,
}

/// Render the iOS project into `out_dir`. Returns whether files were
/// rewritten. See [`crate::android::sync`] for the fast-path / drift
/// rationale — same approach.
pub fn sync(out_dir: &Path, inputs: &IosInputs) -> Result<bool> {
    let new_fp = fingerprint::fingerprint(
        serde_json::to_vec(inputs)
            .context("serialize IosInputs for fingerprint")?
            .as_slice(),
    );
    let fp_path = out_dir.join(".whisker-fingerprint");
    if let Ok(existing) = std::fs::read_to_string(&fp_path) {
        if existing.trim() == new_fp {
            return Ok(false);
        }
    }

    write_files(out_dir, inputs).context("write iOS project files")?;
    std::fs::write(&fp_path, &new_fp)
        .with_context(|| format!("write fingerprint {}", fp_path.display()))?;
    Ok(true)
}

pub(crate) fn template_vars(inputs: &IosInputs) -> HashMap<&'static str, String> {
    let mut v = HashMap::new();
    v.insert("app_name", inputs.app_name.clone());
    v.insert("version", inputs.version.clone());
    v.insert("build_number", inputs.build_number.to_string());
    v.insert("ios_scheme", inputs.scheme.clone());
    v.insert("ios_bundle_id", inputs.bundle_id.clone());
    v.insert(
        "ios_deployment_target",
        inputs.deployment_target.clone(),
    );
    v.insert(
        "whisker_runtime_ios_path",
        inputs.whisker_runtime_path.display().to_string(),
    );
    v
}

fn write_files(out_dir: &Path, inputs: &IosInputs) -> Result<()> {
    let vars = template_vars(inputs);

    // Wipe the previous tree but keep the per-build output dir —
    // expensive to recreate and re-derivable by re-running xcodebuild.
    clean_managed_tree(out_dir, &inputs.scheme)
        .context("clean previous iOS gen tree")?;

    // Top-level text files (plain templates).
    let text_files: &[(PathBuf, &str)] = &[
        (out_dir.join("Info.plist"), INFO_PLIST),
        (out_dir.join("Sources/AppDelegate.swift"), APP_DELEGATE_SWIFT),
    ];
    for (path, template) in text_files {
        let rendered = render(template, &vars)
            .with_context(|| format!("render {}", path.display()))?;
        write_file(path, rendered.as_bytes())?;
    }

    // Xcode project tree. Filename includes the scheme, content is
    // rendered.
    let xcodeproj = out_dir.join(format!("{}.xcodeproj", inputs.scheme));
    let pbxproj = render(PBXPROJ, &vars).context("render project.pbxproj")?;
    write_file(&xcodeproj.join("project.pbxproj"), pbxproj.as_bytes())?;
    // xcworkspacedata has no placeholders — write as-is.
    write_file(
        &xcodeproj
            .join("project.xcworkspace")
            .join("contents.xcworkspacedata"),
        XCWORKSPACEDATA.as_bytes(),
    )?;

    Ok(())
}

fn clean_managed_tree(out_dir: &Path, scheme: &str) -> Result<()> {
    if !out_dir.exists() {
        return Ok(());
    }
    // The `.xcodeproj` directory is now CNG-owned (we render every
    // file inside it) so we clean it on each sync to avoid stale
    // content. `build` is xcodebuild's `-derivedDataPath` output
    // and is expensive to rebuild; preserve it.
    let xcodeproj_dir = format!("{scheme}.xcodeproj");
    let keep = ["build"];
    for entry in std::fs::read_dir(out_dir)
        .with_context(|| format!("read_dir {}", out_dir.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".whisker-fingerprint" {
            continue;
        }
        if keep.iter().any(|k| name.as_os_str() == *k) {
            continue;
        }
        // The xcodeproj itself is regenerated on every sync (because
        // its contents may template differently each time). Skip the
        // keep list and let `remove_path` blow it away.
        let _ = &xcodeproj_dir;
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn remove_path(p: &Path) -> Result<()> {
    if p.is_dir() {
        std::fs::remove_dir_all(p).with_context(|| format!("rm -rf {}", p.display()))
    } else {
        std::fs::remove_file(p).with_context(|| format!("rm {}", p.display()))
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

/// Pull the iOS-relevant subset of `AppConfig` into the renderer
/// input struct. Errors out on required fields. `scheme` defaults to
/// `name`; `bundle_id` defaults to the top-level `app.bundle_id`.
pub fn inputs_from(
    app_config: &AppConfig,
    whisker_runtime_path: PathBuf,
) -> Result<IosInputs> {
    let app_name = app_config
        .name
        .clone()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required"))?;
    let version = app_config
        .version
        .clone()
        .unwrap_or_else(|| "0.1.0".to_string());
    let build_number = app_config.build_number.unwrap_or(1);
    let scheme = app_config
        .ios
        .scheme
        .clone()
        .unwrap_or_else(|| app_name.clone());
    let bundle_id = app_config
        .ios
        .bundle_id
        .clone()
        .or_else(|| app_config.bundle_id.clone())
        .ok_or_else(|| anyhow!(
            "whisker.rs: app.ios(|i| i.bundle_id(\"…\")) (or app.bundle_id) is required for iOS"
        ))?;
    let deployment_target = app_config
        .ios
        .deployment_target
        .clone()
        .unwrap_or_else(|| "13.0".to_string());
    Ok(IosInputs {
        app_name,
        version,
        build_number,
        scheme,
        bundle_id,
        deployment_target,
        whisker_runtime_path,
        // Bumped from 1 → 2 alongside the xcodegen-removal cutover so
        // existing fingerprints invalidate and trigger a re-render.
        template_version: 2,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-cng-ios-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample_inputs() -> IosInputs {
        IosInputs {
            app_name: "HelloWorld".into(),
            version: "0.1.0".into(),
            build_number: 1,
            scheme: "HelloWorld".into(),
            bundle_id: "rs.whisker.examples.helloWorld".into(),
            deployment_target: "13.0".into(),
            whisker_runtime_path: PathBuf::from("/abs/native/ios"),
            template_version: 2,
        }
    }

    #[test]
    fn sync_writes_expected_files() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        let regenerated = sync(&out, &sample_inputs()).unwrap();
        assert!(regenerated);
        for expected in [
            "Info.plist",
            "Sources/AppDelegate.swift",
            "HelloWorld.xcodeproj/project.pbxproj",
            "HelloWorld.xcodeproj/project.xcworkspace/contents.xcworkspacedata",
            ".whisker-fingerprint",
        ] {
            assert!(out.join(expected).exists(), "missing: {expected}");
        }
        // No project.yml — xcodegen is gone.
        assert!(!out.join("project.yml").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_substitutes_placeholders_in_pbxproj() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        sync(&out, &sample_inputs()).unwrap();
        let pbxproj = std::fs::read_to_string(
            out.join("HelloWorld.xcodeproj/project.pbxproj"),
        )
        .unwrap();
        assert!(pbxproj.contains("PRODUCT_BUNDLE_IDENTIFIER = \"rs.whisker.examples.helloWorld\""));
        assert!(pbxproj.contains("IPHONEOS_DEPLOYMENT_TARGET = \"13.0\""));
        assert!(pbxproj.contains("relativePath = \"/abs/native/ios\""));
        assert!(pbxproj.contains("path = \"/abs/native/ios\""));
        assert!(pbxproj.contains("name = \"HelloWorld\""));
        assert!(pbxproj.contains("productName = \"HelloWorld\""));
        // Catch any unsubstituted placeholders.
        assert!(!pbxproj.contains("{{"));
    }

    #[test]
    fn sync_substitutes_placeholders_in_info_plist() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        sync(&out, &sample_inputs()).unwrap();
        let plist = std::fs::read_to_string(out.join("Info.plist")).unwrap();
        assert!(plist.contains("<string>HelloWorld</string>"));
        assert!(plist.contains("<string>0.1.0</string>"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_is_idempotent_when_fingerprint_matches() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        let first = sync(&out, &sample_inputs()).unwrap();
        assert!(first);
        let second = sync(&out, &sample_inputs()).unwrap();
        assert!(!second);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_regenerates_xcodeproj_when_inputs_change() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        sync(&out, &sample_inputs()).unwrap();
        let mut next = sample_inputs();
        next.scheme = "NewScheme".into();
        sync(&out, &next).unwrap();
        // New scheme dir exists; old one is gone (entire xcodeproj
        // is re-rendered).
        assert!(out.join("NewScheme.xcodeproj/project.pbxproj").exists());
        assert!(!out.join("HelloWorld.xcodeproj").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn inputs_from_errors_when_bundle_id_unset() {
        let mut cfg = AppConfig::default();
        cfg.name = Some("X".into());
        let err = inputs_from(&cfg, PathBuf::new()).unwrap_err();
        assert!(err.to_string().contains("bundle_id"), "got: {err:#}");
    }
}
