#![cfg(feature = "generate")]
use std::path::PathBuf;
use whisker_cng::{Config, ProjectEngine, macos::*};
use whisker_plugin::{FileEntry, PluginConfig, project::*};
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "whisker-macos-ir-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn path(p: &str) -> ProjectPath {
    ProjectPath::new(p).unwrap()
}
fn base() -> MacosProjectInputs {
    let mut cfg = Config::default();
    cfg.name("Mac Test").bundle_id("test.whisker.mac");
    let inputs = inputs_from(
        &cfg,
        "fixture".into(),
        "/fixture".into(),
        "/workspace".into(),
    )
    .unwrap();
    let result = ProjectEngine::with_macos_application(inputs)
        .compose(&cfg, &ProjectIr::Macos(Default::default()))
        .unwrap();
    assert!(
        matches!(&result.steps[0],whisker_cng::ProjectStep::Merge { plugin } if plugin == application::ApplicationPlugin::NAME)
    );
    let ProjectIr::Macos(project) = result.project else {
        panic!()
    };
    MacosProjectInputs {
        project,
        project_name: "Mac Test".into(),
        app_crate_dir: None,
        cargo_selection: Default::default(),
        template_version: 13,
    }
}
fn resource(input: &mut MacosProjectInputs, source: &str, destination: &str) {
    input.project.apple.files.insert(
        path(source),
        ProjectFile::Generated {
            entry: FileEntry::text("hello"),
        },
    );
    input
        .project
        .apple
        .targets
        .get_mut("app")
        .unwrap()
        .resources
        .push(AppleResource::Copy {
            resource: Resource {
                source: path(source),
                destination: path(destination),
                kind: ResourceKind::File,
            },
        });
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Options;
impl PluginConfig for Options {
    const NAME: &'static str = "mac-feature";
}
struct Feature;
impl ProjectPlugin for Feature {
    type Config = Options;
    fn contribute(&self, ctx: &ProjectContext, _: &Options) -> anyhow::Result<ProjectUpdate> {
        let ProjectIr::Macos(current) = &ctx.project else {
            return Ok(ProjectUpdate::Keep);
        };
        let mut project = current.clone();
        let target = project
            .apple
            .targets
            .get_mut(&project.apple.application)
            .unwrap();
        target.info_plist.insert(
            "NSCameraUsageDescription".into(),
            PropertyListValue::String("Scan <codes> & labels".into()),
        );
        target.entitlements.insert(
            "com.apple.security.device.camera".into(),
            PropertyListValue::Boolean(true),
        );
        target.resource_plists.insert(
            path("PrivacyInfo.xcprivacy"),
            PropertyListValue::Dict(
                [(
                    "NSPrivacyTracking".into(),
                    PropertyListValue::Boolean(false),
                )]
                .into(),
            ),
        );
        Ok(ProjectUpdate::Merge {
            project: Box::new(ProjectIr::Macos(project)),
        })
    }
}
#[test]
fn metadata_entitlements_and_resources_reach_the_xcode_project() {
    let mut input = base();
    let mut engine = ProjectEngine::new();
    engine.register(Feature);
    let result = engine
        .compose(&Config::default(), &ProjectIr::Macos(input.project.clone()))
        .unwrap();
    let ProjectIr::Macos(project) = result.project else {
        panic!()
    };
    input.project = project;
    resource(&mut input, "staged/message.txt", "nested/message.txt");
    let root = Root::new();
    sync_project(&root.0, &input).unwrap();
    let plist = plist::Value::from_file(root.0.join("Metadata/app/Info.plist")).unwrap();
    let plist = plist.as_dictionary().unwrap();
    assert_eq!(
        plist["NSCameraUsageDescription"].as_string(),
        Some("Scan <codes> & labels")
    );
    assert_eq!(plist["NSPrincipalClass"].as_string(), Some("NSApplication"));
    let ent = plist::Value::from_file(root.0.join("Metadata/app/App.entitlements")).unwrap();
    assert_eq!(
        ent.as_dictionary().unwrap()["com.apple.security.device.camera"].as_boolean(),
        Some(true)
    );
    let pbx = std::fs::read_to_string(root.0.join("Mac Test.xcodeproj/project.pbxproj")).unwrap();
    assert!(pbx.contains("nested/message.txt"));
    assert!(pbx.contains("PrivacyInfo.xcprivacy"));
    assert!(
        root.0
            .join("Mac Test.xcodeproj/xcshareddata/xcschemes/Mac Test.xcscheme")
            .is_file()
    );
}
#[test]
fn application_links_and_embeds_the_rust_driver() {
    let root = Root::new();
    sync_project(&root.0, &base()).unwrap();
    let pbx = std::fs::read_to_string(root.0.join("Mac Test.xcodeproj/project.pbxproj")).unwrap();
    assert!(pbx.contains("SDKROOT = \"macosx\";"));
    assert!(pbx.contains("MACOSX_DEPLOYMENT_TARGET = \"14.0\";"));
    assert!(pbx.contains("@executable_path/../Frameworks"));
    assert!(pbx.contains("libWhiskerDriver.dylib in Frameworks"));
    assert!(pbx.contains("libWhiskerDriver.dylib in Embed Frameworks"));
    assert!(pbx.contains("build-ios") && pbx.contains("$PLATFORM_NAME"));
    let package = std::fs::read_to_string(root.0.join("whisker_modules/Package.swift")).unwrap();
    assert!(package.contains("platforms: [.macOS(.v14)]"));
    let delegate = std::fs::read_to_string(root.0.join("Sources/AppDelegate.swift")).unwrap();
    assert!(delegate.contains("window.title = \"Mac Test\""));
}
#[test]
fn deployment_targets_below_the_appkit_host_minimum_are_rejected() {
    let mut cfg = Config::default();
    cfg.name("Mac Test").bundle_id("test.whisker.mac");
    let mut inputs = inputs_from(
        &cfg,
        "fixture".into(),
        "/fixture".into(),
        "/workspace".into(),
    )
    .unwrap();
    inputs.minimum_system_version = "13.0".into();
    let error = ProjectEngine::with_macos_application(inputs)
        .compose(&cfg, &ProjectIr::Macos(Default::default()))
        .unwrap_err();
    assert!(format!("{error:#}").contains("below the AppKit Host minimum"));
}
#[test]
fn file_changes_invalidate_cache_and_missing_inputs_preserve_prior_output() {
    let mut input = base();
    let root = Root::new();
    let out = root.0.join("gen");
    std::fs::write(root.0.join("data"), "one").unwrap();
    resource(&mut input, "staged/data", "data");
    input.project.apple.files.insert(
        path("staged/data"),
        ProjectFile::AppFile {
            source: path("data"),
        },
    );
    input.app_crate_dir = Some(root.0.clone());
    assert!(sync_project(&out, &input).unwrap());
    assert!(!sync_project(&out, &input).unwrap());
    std::fs::write(root.0.join("data"), "two").unwrap();
    assert!(sync_project(&out, &input).unwrap());
    assert_eq!(std::fs::read(out.join("staged/data")).unwrap(), b"two");
    std::fs::remove_file(root.0.join("data")).unwrap();
    assert!(sync_project(&out, &input).is_err());
    assert_eq!(std::fs::read(out.join("staged/data")).unwrap(), b"two");
}
#[test]
fn case_collisions_are_rejected_before_staging() {
    for name in [
        "resources",
        "Resources",
        "a",
        "sources/AppDelegate.swift",
        "WHISKER_MODULES/Package.swift",
    ] {
        let mut input = base();
        input.project.apple.files.insert(
            path(name),
            ProjectFile::Generated {
                entry: FileEntry::text("wrong"),
            },
        );
        input.project.apple.files.insert(
            path("A/file"),
            ProjectFile::Generated {
                entry: FileEntry::text("nested"),
            },
        );
        assert!(render_project(&input).is_err(), "{name}");
    }
}
