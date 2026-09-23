//! Render the iOS host project under `gen/ios/` from an
//! [`Config`].
//!
//! Application and feature plugins compose a declarative target graph. The
//! renderer serializes that graph into PBX objects, schemes, and staged files;
//! no external project generator is needed. Legacy inputs use the same pipeline.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use whisker_config::Config;
use whisker_plugin::{FileEntry, PbxprojOp, PlistValue};

use crate::compose::{EnabledTargets, Engine};
use crate::fingerprint;
use crate::modules::ResolvedModule;
use crate::render::{escape_xml, render};

pub mod application;
pub(crate) mod builtins;
mod project_render;
pub use project_render::{IosProjectInputs, render_project, sync_project};
const XCWORKSPACEDATA: &str =
    include_str!("templates/ios/Project.xcodeproj/project.xcworkspace/contents.xcworkspacedata");
const APP_DELEGATE_SWIFT: &str = include_str!("templates/ios/Sources/AppDelegate.swift");
const LAUNCH_SCREEN_STORYBOARD: &str =
    include_str!("templates/ios/Resources/LaunchScreen.storyboard");
const ASSET_CATALOG: &str = include_str!("templates/ios/Resources/Assets.xcassets/Contents.json");
const BACKGROUND_COLORSET: &str = include_str!(
    "templates/ios/Resources/Assets.xcassets/WhiskerBackground.colorset/Contents.json"
);

#[derive(Debug, Clone, serde::Serialize)]
pub struct IosInputs {
    pub app_name: String,
    /// Static Host background configured in `whisker.rs` (`#RRGGBB`).
    pub background: String,
    pub version: String,
    pub build_number: u32,
    pub scheme: String,
    pub bundle_id: String,
    pub deployment_target: String,
    /// Legacy input retained for compatibility. The application plugin stages
    /// and references the package at the canonical `whisker_modules` path.
    pub whisker_modules_path: PathBuf,
    /// Absolute path to the cargo workspace root holding the user app
    /// crate's `[workspace]` `Cargo.toml`. Embedded into the pbxproj's
    /// Run Script Build Phase as `--workspace=...` so Xcode-driven
    /// builds invoke `whisker build-ios` without the user typing it.
    pub workspace_root: PathBuf,
    /// Cargo package name (the user app crate) — the Rust side of
    /// `whisker build-ios --package=...`.
    pub user_package: String,
    /// Cargo-resolved Whisker modules materialized into the SwiftPM
    /// aggregator as part of this CNG transaction.
    #[serde(default)]
    pub modules: Vec<ResolvedModule>,
    /// Legacy plugin plist entries merged into the application declaration.
    /// Nested dictionaries and mixed arrays are preserved by the plist serializer.
    #[serde(default)]
    pub extra_info_plist: BTreeMap<String, PlistValue>,
    /// Plugin-supplied additional files dropped into `gen/ios/`.
    /// Keys are relative paths (validated to be relative + free of
    /// `..` traversal at write time); values are
    /// [`FileEntry`]s — UTF-8 contents + optional POSIX mode.
    #[serde(default)]
    pub extra_files: BTreeMap<PathBuf, FileEntry>,
    /// Legacy operations translated into the main target's declarations by the
    /// application plugin. PBX IDs are assigned by the renderer from stable IDs.
    #[serde(default)]
    pub pbxproj_ops: Vec<PbxprojOp>,
    pub template_version: u32,
    /// Application Cargo inputs included in the generation fingerprint.
    pub cargo_selection: crate::CargoSelection,
}

/// Render the iOS project into `out_dir`. Returns whether files were
/// rewritten. See [`crate::android::sync`] for the fast-path / drift
/// rationale — same approach.
pub fn sync(out_dir: &Path, inputs: &IosInputs) -> Result<bool> {
    let composition =
        crate::ProjectEngine::with_initializer(application::ApplicationPlugin::new(inputs.clone()))
            .compose(
                &Config::default(),
                &whisker_plugin::project::ProjectIr::Ios(Default::default()),
            )?;
    let whisker_plugin::project::ProjectIr::Ios(project) = composition.project else {
        unreachable!()
    };
    sync_project(
        out_dir,
        &IosProjectInputs {
            project,
            project_name: inputs.scheme.clone(),
            app_crate_dir: None,
            cargo_selection: inputs.cargo_selection.clone(),
            template_version: inputs.template_version,
        },
    )
}

pub(crate) fn template_vars(inputs: &IosInputs) -> HashMap<&'static str, String> {
    let mut v = HashMap::new();
    v.insert("app_name", inputs.app_name.clone());
    let background = crate::background::AppBackground::parse(&inputs.background)
        .expect("IosInputs background is validated when it is resolved");
    let [red, green, blue] = background.rgb();
    v.insert("background_red", color_component(red));
    v.insert("background_green", color_component(green));
    v.insert("background_blue", color_component(blue));
    v.insert("version", inputs.version.clone());
    v.insert("build_number", inputs.build_number.to_string());
    v.insert("ios_scheme", inputs.scheme.clone());
    v.insert("ios_bundle_id", inputs.bundle_id.clone());
    v.insert("ios_deployment_target", inputs.deployment_target.clone());
    v.insert(
        "whisker_modules_ios_path",
        inputs.whisker_modules_path.display().to_string(),
    );
    v.insert(
        "whisker_workspace_root",
        inputs.workspace_root.display().to_string(),
    );
    v.insert("whisker_user_package", inputs.user_package.clone());
    v
}

fn color_component(value: u8) -> String {
    format!("{:.6}", f32::from(value) / 255.0)
}

/// Pick a `lastKnownFileType` for a file path, by extension. Falls
/// back to `text` for anything unknown — Xcode tolerates a wrong
/// guess; it just affects the navigator icon.
fn last_known_file_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("swift") => "sourcecode.swift",
        Some("m") => "sourcecode.c.objc",
        Some("mm") => "sourcecode.cpp.objcpp",
        Some("h") => "sourcecode.c.h",
        Some("plist") => "text.plist.xml",
        Some("json") => "text.json",
        Some("png") => "image.png",
        Some("jpg") | Some("jpeg") => "image.jpeg",
        Some("xcassets") => "folder.assetcatalog",
        // Icon Composer bundle (Xcode 26+). The dedicated type is
        // what makes xcodebuild hand the bundle to actool for
        // Liquid Glass appearance generation instead of copying the
        // directory verbatim.
        Some("icon") => "folder.iconcomposer.icon",
        Some("storyboard") => "file.storyboard",
        Some("xib") => "file.xib",
        _ => "text",
    }
}

/// Deterministic 24-hex-char UUID for a stable string seed, matching
/// the canonical shape Xcode produces (96-bit): two differently-salted
/// FNV-1a hashes spliced and truncated. A collision within one sync
/// would make the rendered pbxproj fail to parse rather than corrupt
/// silently.
fn pbxproj_uuid(seed: &str) -> String {
    let a = crate::fingerprint::fingerprint(seed.as_bytes());
    let b = crate::fingerprint::fingerprint(format!("{seed}-salt").as_bytes());
    format!("{a}{}", &b[..8]).to_uppercase()
}

#[cfg(unix)]
fn apply_mode(path: &Path, mode: Option<u32>) -> Result<()> {
    if let Some(m) = mode {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .with_context(|| format!("stat {} for chmod", path.display()))?
            .permissions();
        perms.set_mode(m);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("chmod {:o} on {}", m, path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: Option<u32>) -> Result<()> {
    // POSIX mode bits don't translate cleanly to Windows ACLs, and the
    // IR is platform-agnostic, so the field is accepted and ignored.
    Ok(())
}

fn clean_managed_tree(out_dir: &Path, scheme: &str) -> Result<()> {
    if !out_dir.exists() {
        return Ok(());
    }
    // Everything here is CNG-rendered and safe to wipe except
    // `build`, xcodebuild's `-derivedDataPath` output.
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

/// Pull the iOS-relevant subset of `Config` into the renderer input
/// struct. Errors out on required fields. `scheme` defaults to `name`;
/// `bundle_id` defaults to the top-level `app.bundle_id`.
///
/// Thin wrapper over [`inputs_from_with_engine`] using
/// [`Engine::with_builtins`] — call that form directly to register
/// additional plugins.
pub fn inputs_from(
    app_config: &Config,
    whisker_modules_path: PathBuf,
    workspace_root: PathBuf,
    user_package: String,
) -> Result<IosInputs> {
    inputs_from_with_engine(
        &Engine::with_builtins(),
        app_config,
        whisker_modules_path,
        workspace_root,
        user_package,
    )
}

/// Like [`inputs_from`] but takes a pre-built [`Engine`] so the
/// caller can register additional plugins (e.g. subprocess plugins
/// discovered from `[package.metadata.whisker.plugins]`).
pub fn inputs_from_with_engine(
    engine: &Engine,
    app_config: &Config,
    whisker_modules_path: PathBuf,
    workspace_root: PathBuf,
    user_package: String,
) -> Result<IosInputs> {
    // The engine seeds the IR from `Config` and plugins may override
    // any of it, so everything below is extraction plus ergonomic
    // defaults for whatever the pipeline left as `None`.
    let ctx = engine
        .compose(app_config, EnabledTargets::ios_only())
        .context("compose Whisker CNG plugin pipeline for iOS")?;
    let ios_ir = ctx
        .ios
        .as_ref()
        .expect("EnabledTargets::ios_only guarantees Some");

    let crate::plugins::application::IosApplication {
        app_name,
        version,
        build_number,
        scheme,
        bundle_id,
        deployment_target,
    } = crate::plugins::application::ios(ios_ir)?;
    let background = crate::background::AppBackground::resolve(app_config)?;

    let extra_info_plist = ios_ir.info_plist.clone();
    let extra_files = ios_ir.extra_files.clone();
    let pbxproj_ops = ios_ir.pbxproj_ops.clone();

    Ok(IosInputs {
        app_name,
        background: background.hex().to_string(),
        version,
        build_number,
        scheme,
        bundle_id,
        deployment_target,
        whisker_modules_path,
        workspace_root,
        user_package,
        modules: Vec::new(),
        extra_info_plist,
        extra_files,
        pbxproj_ops,
        // Bump on any template or renderer change: it feeds the sync
        // fingerprint, and without it existing `gen/ios/` trees keep
        // their stale output.
        template_version: 40,
        cargo_selection: crate::CargoSelection::default(),
    })
}

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
            background: "#FFFFFF".into(),
            version: "0.1.0".into(),
            build_number: 1,
            scheme: "HelloWorld".into(),
            bundle_id: "rs.whisker.examples.helloWorld".into(),
            deployment_target: "13.0".into(),
            whisker_modules_path: PathBuf::from("/abs/gen/ios/whisker_modules"),
            workspace_root: PathBuf::from("/abs/workspace"),
            user_package: "hello-world".into(),
            modules: Vec::new(),
            extra_info_plist: BTreeMap::new(),
            extra_files: BTreeMap::new(),
            pbxproj_ops: Vec::new(),
            template_version: 40,
            cargo_selection: crate::CargoSelection::default(),
        }
    }

    #[test]
    fn generated_delegate_only_composes_the_sdk_view() {
        assert!(APP_DELEGATE_SWIFT.contains("WhiskerView(frame:"));
        assert!(!APP_DELEGATE_SWIFT.contains("class WhiskerView"));
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
            "Resources/LaunchScreen.storyboard",
            "Resources/Assets.xcassets/Contents.json",
            "Resources/Assets.xcassets/WhiskerBackground.colorset/Contents.json",
            "HelloWorld.xcodeproj/project.pbxproj",
            "HelloWorld.xcodeproj/project.xcworkspace/contents.xcworkspacedata",
            "HelloWorld.xcodeproj/xcshareddata/xcschemes/HelloWorld.xcscheme",
            ".whisker-fingerprint",
        ] {
            assert!(out.join(expected).exists(), "missing: {expected}");
        }
        assert!(
            !out.join("Sources/WhiskerView.swift").exists(),
            "the generated app must consume WhiskerView from the iOS SDK"
        );
        assert!(!out.join("project.yml").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_projects_static_background_into_ios_startup_and_container() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        let mut inputs = sample_inputs();
        inputs.background = "#101018".into();
        sync(&out, &inputs).unwrap();

        let colorset = std::fs::read_to_string(
            out.join("Resources/Assets.xcassets/WhiskerBackground.colorset/Contents.json"),
        )
        .unwrap();
        let plist = std::fs::read_to_string(out.join("Info.plist")).unwrap();
        let storyboard =
            std::fs::read_to_string(out.join("Resources/LaunchScreen.storyboard")).unwrap();
        let delegate = std::fs::read_to_string(out.join("Sources/AppDelegate.swift")).unwrap();
        let pbxproj =
            std::fs::read_to_string(out.join("HelloWorld.xcodeproj/project.pbxproj")).unwrap();

        assert!(colorset.contains("\"red\" : \"0.062745\""));
        assert!(colorset.contains("\"green\" : \"0.062745\""));
        assert!(colorset.contains("\"blue\" : \"0.094118\""));
        assert!(plist.contains("<key>UILaunchStoryboardName</key>"));
        assert!(plist.contains("<string>LaunchScreen</string>"));
        assert!(storyboard.contains("red=\"0.062745\""));
        assert!(storyboard.contains("green=\"0.062745\""));
        assert!(storyboard.contains("blue=\"0.094118\""));
        assert!(delegate.contains("UIColor(named: \"WhiskerBackground\")"));
        assert!(delegate.contains("root.view.addSubview(whiskerView)"));
        assert!(pbxproj.contains("Assets.xcassets in Resources"));
        assert!(
            !pbxproj.contains("ASSETCATALOG_COMPILER_APPICON_NAME"),
            "a background color catalog must not require an AppIcon catalog",
        );

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
        assert!(!pbxproj.contains("XCRemoteSwiftPackageReference"));
        assert!(!pbxproj.contains("WhiskerRuntime"));
        assert!(!pbxproj.contains("Lynx"));
        assert!(pbxproj.contains("XCLocalSwiftPackageReference \"whisker_modules\""));
        assert!(pbxproj.contains("WhiskerModules in Frameworks"));
        assert!(pbxproj.contains("WhiskerDriver.framework in Embed Frameworks"));
        assert!(pbxproj.contains("Whisker Build Rust App"));
        assert!(pbxproj.contains("WHISKER_CLI=\\\"${WHISKER_CLI:-whisker}\\\""));
        assert!(pbxproj.contains("\\\"$WHISKER_CLI\\\" build-ios"));
        assert!(pbxproj.contains("@executable_path/Frameworks"));
        assert!(pbxproj.contains("name = \"HelloWorld\""));
        assert!(pbxproj.contains("productName = \"HelloWorld\""));
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
        assert!(out.join("NewScheme.xcodeproj/project.pbxproj").exists());
        assert!(!out.join("HelloWorld.xcodeproj").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn add_resource_folder_renders_into_pbxproj_resources_phase() {
        let mut inputs = sample_inputs();
        inputs.pbxproj_ops = vec![PbxprojOp::AddResourceFolder {
            path: PathBuf::from("whisker_assets"),
        }];
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        sync(&out, &inputs).unwrap();
        let pbxproj =
            std::fs::read_to_string(out.join("HelloWorld.xcodeproj/project.pbxproj")).unwrap();
        assert!(pbxproj.contains("lastKnownFileType = folder;"));
        assert!(pbxproj.contains("whisker_assets in Resources"));
        assert!(!pbxproj.contains("{{"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extra_files_writes_binary_contents_via_base64() {
        let mut inputs = sample_inputs();
        let raw = vec![0x89u8, 0x50, 0x4e, 0x47, 0x00, 0xff];
        inputs.extra_files.insert(
            PathBuf::from("whisker_assets/images/logo.png"),
            FileEntry::binary(&raw),
        );
        let tmp = unique_tempdir();
        let out = tmp.join("gen/ios");
        sync(&out, &inputs).unwrap();
        let written = std::fs::read(out.join("whisker_assets/images/logo.png")).unwrap();
        assert_eq!(written, raw);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn inputs_from_errors_when_bundle_id_unset() {
        let cfg = Config {
            name: Some("X".into()),
            ..Config::default()
        };
        let err = inputs_from(&cfg, PathBuf::new(), PathBuf::new(), String::new()).unwrap_err();
        assert!(err.to_string().contains("bundle_id"), "got: {err:#}");
    }
}
