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
//! │   ├── project.xcworkspace/
//! │   │   └── contents.xcworkspacedata
//! │   └── xcshareddata/xcschemes/
//! │       └── <scheme>.xcscheme
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
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use whisker_app_config::AppConfig;
use whisker_plugin::PlistValue;

use crate::compose::{EnabledTargets, Engine};
use crate::fingerprint;
use crate::render::render;

const PBXPROJ: &str = include_str!("templates/ios/Project.xcodeproj/project.pbxproj");
const XCWORKSPACEDATA: &str =
    include_str!("templates/ios/Project.xcodeproj/project.xcworkspace/contents.xcworkspacedata");
const XCSCHEME: &str =
    include_str!("templates/ios/Project.xcodeproj/xcshareddata/xcschemes/scheme.xcscheme");
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
    /// Path the generated pbxproj's `XCLocalSwiftPackageReference`
    /// for `WhiskerRuntime` points at — typically
    /// `<workspace>/platforms/ios` in the monorepo. The published
    /// `XCRemoteSwiftPackageReference` flow that root `Package.swift`
    /// supports is gated on every Whisker module's `Package.swift`
    /// moving off the env-var path-based `.package(path:)`
    /// resolution onto the same remote URL — until that lands,
    /// the cng output stays local-path to avoid product-name
    /// conflicts.
    pub whisker_runtime_path: PathBuf,
    /// Path to the auto-generated `WhiskerModules` SwiftPM package
    /// — typically `<crate_dir>/gen/ios/whisker_modules`. Pointed
    /// at the gen-tree-managed dir `whisker-build::ios::
    /// stage_module_swift_sources` populates with each module's
    /// `[ios].swift_sources` and the generated
    /// `WhiskerModuleBehaviors.swift`.
    pub whisker_modules_path: PathBuf,
    /// Absolute path to the cargo workspace root that contains the
    /// user app crate's top-level `Cargo.toml` (the one with
    /// `[workspace]`). Embedded into the pbxproj's Run Script Build
    /// Phase as `--workspace=...` so Xcode-driven builds invoke
    /// `whisker-build ios` without the user typing it. Step 7.
    pub workspace_root: PathBuf,
    /// Cargo package name (the user app crate) — the Rust side of
    /// `whisker-build ios --package=...`. Step 7.
    pub user_package: String,
    /// `(key, string-value)` pairs sourced from the engine's
    /// post-pipeline IR (`ctx.ios.info_plist`). Emitted into the
    /// rendered `Info.plist` just before the closing `</dict>`. The
    /// renderer XML-escapes values; keys are assumed safe because
    /// they come from Rust string constants in plugin Configs.
    ///
    /// Only `PlistValue::String` entries surface here for now;
    /// dict / array values are dropped because the Info.plist
    /// template is hand-rolled XML rather than a real plist
    /// serializer. Adding richer value support is a future
    /// renderer change, not a wire-format break.
    #[serde(default)]
    pub extra_info_plist_kvs: BTreeMap<String, String>,
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
    v.insert("ios_deployment_target", inputs.deployment_target.clone());
    v.insert(
        "whisker_runtime_ios_path",
        inputs.whisker_runtime_path.display().to_string(),
    );
    v.insert(
        "whisker_modules_ios_path",
        inputs.whisker_modules_path.display().to_string(),
    );
    v.insert(
        "whisker_workspace_root",
        inputs.workspace_root.display().to_string(),
    );
    v.insert("whisker_user_package", inputs.user_package.clone());
    v.insert(
        "extra_info_plist_kvs",
        render_extra_info_plist_kvs(&inputs.extra_info_plist_kvs),
    );
    v
}

/// Render the engine-supplied `(key, string)` pairs as XML
/// `<key>…</key><string>…</string>` rows ready to drop straight
/// into the Info.plist template just before `</dict>`. Empty map
/// → empty string (no whitespace) so the template still parses
/// cleanly.
fn render_extra_info_plist_kvs(entries: &BTreeMap<String, String>) -> String {
    if entries.is_empty() {
        return String::new();
    }
    // Indent matching the rest of the template (tab characters,
    // matching the hand-rolled Info.plist's existing style).
    let mut out = String::new();
    for (key, value) in entries {
        out.push_str(&format!(
            "\t<key>{}</key>\n\t<string>{}</string>\n",
            escape_xml(key),
            escape_xml(value),
        ));
    }
    // Strip the trailing newline so the template's own newline
    // before `</dict>` isn't doubled up.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

fn write_files(out_dir: &Path, inputs: &IosInputs) -> Result<()> {
    let vars = template_vars(inputs);

    // Wipe the previous tree but keep the per-build output dir —
    // expensive to recreate and re-derivable by re-running xcodebuild.
    clean_managed_tree(out_dir, &inputs.scheme).context("clean previous iOS gen tree")?;

    // Top-level text files (plain templates).
    let text_files: &[(PathBuf, &str)] = &[
        (out_dir.join("Info.plist"), INFO_PLIST),
        (
            out_dir.join("Sources/AppDelegate.swift"),
            APP_DELEGATE_SWIFT,
        ),
    ];
    for (path, template) in text_files {
        let rendered =
            render(template, &vars).with_context(|| format!("render {}", path.display()))?;
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
    // Shared xcscheme so opening the project in Xcode.app yields the
    // same Build / Run / Test / Profile / Analyze / Archive surface
    // every contributor sees. Without this, Xcode auto-creates a
    // per-user scheme on first open — works, but isn't shared via
    // source control and the user has to pick a destination on every
    // fresh checkout. Filename mirrors the scheme name so Xcode picks
    // it up by convention (it scans `xcshareddata/xcschemes/*.xcscheme`).
    let xcscheme = render(XCSCHEME, &vars).context("render xcscheme")?;
    write_file(
        &xcodeproj
            .join("xcshareddata/xcschemes")
            .join(format!("{}.xcscheme", inputs.scheme)),
        xcscheme.as_bytes(),
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
    for entry in
        std::fs::read_dir(out_dir).with_context(|| format!("read_dir {}", out_dir.display()))?
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
    whisker_modules_path: PathBuf,
    workspace_root: PathBuf,
    user_package: String,
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
        .ok_or_else(|| {
            anyhow!(
            "whisker.rs: app.ios(|i| i.bundle_id(\"…\")) (or app.bundle_id) is required for iOS"
        )
        })?;
    let deployment_target = app_config
        .ios
        .deployment_target
        .clone()
        .unwrap_or_else(|| "13.0".to_string());

    // Run the plugin pipeline with built-ins. Apps that never call
    // `app.plugin::<…>(…)` get an empty IR back; the resulting
    // `extra_info_plist_kvs` is empty and the Info.plist render is
    // bit-identical to the pre-Phase-3 output.
    let ctx = Engine::with_builtins()
        .compose(app_config, EnabledTargets::ios_only())
        .context("compose Whisker CNG plugin pipeline for iOS")?;
    let extra_info_plist_kvs = extract_info_plist_string_kvs(&ctx);

    Ok(IosInputs {
        app_name,
        version,
        build_number,
        scheme,
        bundle_id,
        deployment_target,
        whisker_runtime_path,
        whisker_modules_path,
        workspace_root,
        user_package,
        extra_info_plist_kvs,
        // Bumped 9 → 10 for RFC #164 Phase 2/3: the Info.plist
        // template gained the `{{extra_info_plist_kvs}}`
        // placeholder. Existing `gen/ios/` trees regenerate so the
        // placeholder substitutes correctly even before any
        // plugin contributes content.
        template_version: 10,
    })
}

/// Project the iOS info_plist BTreeMap (the IR layer) into the
/// `(key, string-value)` shape the template renderer accepts.
/// Non-string `PlistValue` variants are silently dropped — the
/// template is hand-rolled XML and can't represent dicts / arrays
/// safely. A future renderer that emits real plist XML can lift
/// this restriction without changing the IR.
fn extract_info_plist_string_kvs(
    ctx: &whisker_plugin::GenerateContext,
) -> BTreeMap<String, String> {
    let Some(ios) = ctx.ios.as_ref() else {
        return BTreeMap::new();
    };
    ios.info_plist
        .iter()
        .filter_map(|(k, v)| match v {
            PlistValue::String(s) => Some((k.clone(), s.clone())),
            _ => None,
        })
        .collect()
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
            whisker_runtime_path: PathBuf::from("/abs/platforms/ios"),
            whisker_modules_path: PathBuf::from("/abs/gen/ios/whisker_modules"),
            workspace_root: PathBuf::from("/abs/workspace"),
            user_package: "hello-world".into(),
            extra_info_plist_kvs: BTreeMap::new(),
            template_version: 10,
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
            "HelloWorld.xcodeproj/xcshareddata/xcschemes/HelloWorld.xcscheme",
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
        let pbxproj =
            std::fs::read_to_string(out.join("HelloWorld.xcodeproj/project.pbxproj")).unwrap();
        assert!(pbxproj.contains("PRODUCT_BUNDLE_IDENTIFIER = \"rs.whisker.examples.helloWorld\""));
        assert!(pbxproj.contains("IPHONEOS_DEPLOYMENT_TARGET = \"13.0\""));
        // XCLocalSwiftPackageReference for WhiskerRuntime (monorepo
        // workflow). The XCRemoteSwiftPackageReference form behind
        // root Package.swift stays available for future
        // published-consumer flows.
        assert!(pbxproj.contains("relativePath = \"/abs/platforms/ios\""));
        // WhiskerModules resolves through the per-app gen-tree dir.
        assert!(pbxproj.contains("relativePath = \"/abs/gen/ios/whisker_modules\""));
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
        let cfg = AppConfig {
            name: Some("X".into()),
            ..AppConfig::default()
        };
        let err = inputs_from(
            &cfg,
            PathBuf::new(),
            PathBuf::new(),
            PathBuf::new(),
            String::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("bundle_id"), "got: {err:#}");
    }
}
