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
    let inputs = inputs_from(&cfg, "fixture".into(), "/fixture".into(), "\"0.14\"".into()).unwrap();
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
        app_crate_dir: None,
        cargo_selection: Default::default(),
        template_version: 12,
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
fn metadata_entitlements_and_resources_reach_the_bundle_plan() {
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
    input.project.apple.files.insert(
        path("private.txt"),
        ProjectFile::Generated {
            entry: FileEntry::text("private"),
        },
    );
    let root = Root::new();
    sync_project(&root.0, &input).unwrap();
    let plan = load_build_plan(&root.0).unwrap();
    let files = bundle_files(&root.0, &plan).unwrap();
    assert_eq!(
        files[&path("Contents/Resources/nested/message.txt")]
            .to_bytes()
            .unwrap(),
        b"hello"
    );
    assert!(!files.contains_key(&path("private.txt")));
    let plist = plist::Value::from_reader_xml(
        files[&path("Contents/Info.plist")]
            .to_bytes()
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    assert_eq!(
        plist.as_dictionary().unwrap()["NSCameraUsageDescription"].as_string(),
        Some("Scan <codes> & labels")
    );
    assert!(files.contains_key(&path("Contents/Resources/PrivacyInfo.xcprivacy")));
    let ent =
        plist::Value::from_reader_xml(signing_entitlements(&root.0, &plan).unwrap().as_slice())
            .unwrap();
    assert_eq!(
        ent.as_dictionary().unwrap()["com.apple.security.device.camera"].as_boolean(),
        Some(true)
    );
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
fn backend_rejects_native_build_work_it_cannot_execute() {
    let input = base();
    let mut extra = input.clone();
    extra.project.apple.targets.insert(
        "extension".into(),
        AppleTarget {
            product_name: "Share".into(),
            product_type: Some("com.apple.product-type.app-extension".into()),
            ..Default::default()
        },
    );
    assert!(
        render_project(&extra)
            .unwrap_err()
            .to_string()
            .contains("one application")
    );
    let mut settings = input.clone();
    settings.project.apple.build_settings.insert(
        "SWIFT_VERSION".into(),
        AppleBuildSetting::String("5.9".into()),
    );
    assert!(
        render_project(&settings)
            .unwrap_err()
            .to_string()
            .contains("cannot apply")
    );
    let mut source = input.clone();
    source
        .project
        .apple
        .targets
        .get_mut("app")
        .unwrap()
        .sources
        .push(AppleSource {
            path: AppleBuildPath::Project(path("Extra.swift")),
            platform_filters: vec![],
            compiler_flags: vec![],
        });
    assert!(render_project(&source).is_err());
    let mut cargo = input;
    cargo
        .project
        .apple
        .targets
        .get_mut("app")
        .unwrap()
        .rust
        .as_mut()
        .unwrap()
        .package = "other".into();
    assert!(
        render_project(&cargo)
            .unwrap_err()
            .to_string()
            .contains("Cargo package")
    );
}
#[test]
fn bundle_files_cannot_replace_metadata_executables_or_resource_subtrees() {
    for destination in [
        "Contents/Info.plist",
        "contents/info.plist",
        "Contents/MacOS/fixture-whisker-macos",
        "Contents/Resources/data/child",
        "Contents/_CodeSignature/file",
    ] {
        let mut input = base();
        resource(&mut input, "source", "data");
        input
            .project
            .apple
            .targets
            .get_mut("app")
            .unwrap()
            .bundle_files
            .push(Resource {
                source: path("source"),
                destination: path(destination),
                kind: ResourceKind::File,
            });
        assert!(render_project(&input).is_err(), "{destination}");
    }
}

#[test]
fn case_collisions_are_rejected_before_staging() {
    for name in [
        "info.plist",
        "resources",
        "Resources",
        "a",
        "TARGET/file",
        ".WHISKER/other",
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
