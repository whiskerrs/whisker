#![cfg(feature = "generate")]
use std::{collections::BTreeMap, path::PathBuf};
use whisker_cng::{Config, Engine, ProjectEngine, ProjectStep, ios::*};
use whisker_plugin::{FileEntry, PluginConfig, project::*};

fn inputs() -> IosInputs {
    let mut config = Config::default();
    config
        .name("NativeFixture")
        .bundle_id("test.whisker.fixture");
    inputs_from_with_engine(
        &Engine::new(),
        &config,
        "whisker_modules".into(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        "native-fixture".into(),
    )
    .unwrap()
}
fn base() -> IosProjectInputs {
    let result = ProjectEngine::with_ios_application(inputs())
        .compose(&Config::default(), &ProjectIr::Ios(Default::default()))
        .unwrap();
    assert_eq!(
        result.steps[0],
        ProjectStep::Merge {
            plugin: application::ApplicationPlugin::NAME.into()
        }
    );
    let ProjectIr::Ios(project) = result.project else {
        panic!()
    };
    IosProjectInputs {
        project,
        project_name: "NativeFixture".into(),
        app_crate_dir: None,
        cargo_selection: Default::default(),
        template_version: 40,
    }
}
fn text(files: &BTreeMap<ProjectPath, FileEntry>, path: &str) -> String {
    String::from_utf8(files[&ProjectPath::new(path).unwrap()].to_bytes().unwrap()).unwrap()
}
fn string(v: &str) -> PropertyListValue {
    PropertyListValue::String(v.into())
}
fn setting(v: &str) -> AppleBuildSetting {
    AppleBuildSetting::String(v.into())
}
fn temp() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "whisker-ios-ir-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct ShareConfig;
impl PluginConfig for ShareConfig {
    const NAME: &'static str = "a-share";
}
struct Share;
impl ProjectPlugin for Share {
    type Config = ShareConfig;
    fn contribute(&self, ctx: &ProjectContext, _: &ShareConfig) -> anyhow::Result<ProjectUpdate> {
        let ProjectIr::Ios(original) = &ctx.project else {
            return Ok(ProjectUpdate::Keep);
        };
        assert!(original.apple.targets.contains_key("app"));
        let mut project = IosProjectIr::default();
        let source = ProjectPath::new("Share/ShareViewController.swift")?;
        project.apple.targets.insert(
            "share".into(),
            AppleTarget {
                product_name: "Share".into(),
                product_type: Some("com.apple.product-type.app-extension".into()),
                sources: vec![AppleSource {
                    path: AppleBuildPath::Project(source.clone()),
                    compiler_flags: vec![],
                    platform_filters: vec![],
                }],
                build_settings: [
                    (
                        "PRODUCT_BUNDLE_IDENTIFIER".into(),
                        setting("test.whisker.fixture.share"),
                    ),
                    ("SWIFT_VERSION".into(), setting("5.9")),
                    ("APPLICATION_EXTENSION_API_ONLY".into(), setting("YES")),
                    ("SKIP_INSTALL".into(), setting("YES")),
                    ("TARGETED_DEVICE_FAMILY".into(), setting("1,2")),
                ]
                .into(),
                info_plist: [
                    (
                        "CFBundleIdentifier".into(),
                        string("$(PRODUCT_BUNDLE_IDENTIFIER)"),
                    ),
                    ("CFBundleExecutable".into(), string("$(EXECUTABLE_NAME)")),
                    ("CFBundleName".into(), string("$(PRODUCT_NAME)")),
                    ("CFBundlePackageType".into(), string("XPC!")),
                    ("CFBundleShortVersionString".into(), string("0.1.0")),
                    ("CFBundleVersion".into(), string("1")),
                    (
                        "NSExtension".into(),
                        PropertyListValue::Dict(
                            [
                                (
                                    "NSExtensionPointIdentifier".into(),
                                    string("com.apple.share-services"),
                                ),
                                (
                                    "NSExtensionPrincipalClass".into(),
                                    string("$(PRODUCT_MODULE_NAME).ShareViewController"),
                                ),
                                (
                                    "NSExtensionAttributes".into(),
                                    PropertyListValue::Dict(
                                        [(
                                            "NSExtensionActivationRule".into(),
                                            PropertyListValue::Boolean(true),
                                        )]
                                        .into(),
                                    ),
                                ),
                            ]
                            .into(),
                        ),
                    ),
                ]
                .into(),
                entitlements: [(
                    "com.apple.security.application-groups".into(),
                    PropertyListValue::Array(vec![string("group.test.whisker.fixture")]),
                )]
                .into(),
                ..Default::default()
            },
        );
        project.apple.files.insert(
            source,
            ProjectFile::Generated {
                entry: FileEntry::text(
                    "import UIKit\nfinal class ShareViewController: UIViewController {}\n",
                ),
            },
        );
        // An additive contribution may reference the app declared earlier.
        let mut app = original.apple.targets["app"].clone();
        app.embeds.push(AppleEmbed {
            source: AppleEmbedSource::Target {
                target: "share".into(),
            },
            destination: ProjectPath::new("PlugIns")?,
            code_sign_on_copy: false,
            remove_headers_on_copy: false,
            platform_filters: vec![],
        });
        project.apple.targets.insert("app".into(), app);
        Ok(ProjectUpdate::Merge {
            project: Box::new(ProjectIr::Ios(project)),
        })
    }
}
fn with_share() -> IosProjectInputs {
    let mut engine = ProjectEngine::with_ios_application(inputs());
    engine.register(Share);
    let result = engine
        .compose(&Config::default(), &ProjectIr::Ios(Default::default()))
        .unwrap();
    let ProjectIr::Ios(project) = result.project else {
        panic!()
    };
    IosProjectInputs { project, ..base() }
}
#[test]
fn extension_is_a_separate_product_with_sources_metadata_dependency_and_embed() {
    let input = with_share();
    let files = render_project(&input).unwrap();
    let source = text(&files, "NativeFixture.xcodeproj/project.pbxproj");
    let pbx = plist::Value::from_reader_ascii(source.as_bytes()).unwrap();
    let objects = pbx.as_dictionary().unwrap()["objects"]
        .as_dictionary()
        .unwrap();
    let native: Vec<_> = objects
        .values()
        .filter(|o| {
            o.as_dictionary()
                .unwrap()
                .get("isa")
                .and_then(|v| v.as_string())
                == Some("PBXNativeTarget")
        })
        .collect();
    assert_eq!(native.len(), 2);
    assert!(source.contains("PBXTargetDependency") && source.contains("dstSubfolderSpec = 13"));
    assert!(source.contains("Share/ShareViewController.swift in Sources"));
    assert!(source.contains("CodeSignOnCopy") && source.contains("WhiskerDriver.framework"));
    let info = plist::Value::from_reader_xml(text(&files, "Metadata/share/Info.plist").as_bytes())
        .unwrap();
    assert_eq!(
        info.as_dictionary().unwrap()["NSExtension"]
            .as_dictionary()
            .unwrap()["NSExtensionPointIdentifier"]
            .as_string(),
        Some("com.apple.share-services")
    );
    assert!(files.contains_key(&ProjectPath::new("Metadata/share/App.entitlements").unwrap()));
    assert!(
        text(
            &files,
            "NativeFixture.xcodeproj/xcshareddata/xcschemes/NativeFixture.xcscheme"
        )
        .contains("BlueprintIdentifier")
    );
    assert_eq!(files, render_project(&input).unwrap());
}
#[test]
fn ios_helpers_edit_application_ir_and_preserve_other_targets() {
    use whisker_cng::plugins::{info_plist_extra::InfoPlistExtra, ios_pbxproj_ops::IosPbxprojOps};
    let mut engine = ProjectEngine::with_ios_application(inputs());
    engine.register(Share);
    let mut config = Config::default();
    config.plugin::<InfoPlistExtra>(|c| {
        c.add("CFBundleDisplayName", "Edited & App");
    });
    config.plugin::<IosPbxprojOps>(|c| {
        c.set_build_setting("SWIFT_VERSION", "6.0");
    });
    let result = engine
        .compose(&config, &ProjectIr::Ios(Default::default()))
        .unwrap();
    let ProjectIr::Ios(project) = result.project else {
        panic!()
    };
    assert_eq!(
        project.apple.targets["app"].info_plist["CFBundleDisplayName"],
        string("Edited & App")
    );
    assert_eq!(
        project.apple.targets["share"].build_settings["SWIFT_VERSION"],
        setting("5.9")
    );
}
#[test]
fn staged_inputs_affect_fingerprint_and_invalid_output_preserves_existing_project() {
    let root = temp();
    std::fs::write(root.join("input.txt"), "one").unwrap();
    let mut input = base();
    input.app_crate_dir = Some(root.clone());
    input.project.apple.files.insert(
        ProjectPath::new("data.txt").unwrap(),
        ProjectFile::AppFile {
            source: ProjectPath::new("input.txt").unwrap(),
        },
    );
    let out = root.join("gen");
    assert!(sync_project(&out, &input).unwrap());
    assert!(!sync_project(&out, &input).unwrap());
    std::fs::write(root.join("input.txt"), "two").unwrap();
    assert!(sync_project(&out, &input).unwrap());
    let old = std::fs::read(out.join(".whisker-fingerprint")).unwrap();
    input.project.apple.files.insert(
        ProjectPath::new("Info.plist").unwrap(),
        ProjectFile::Generated {
            entry: FileEntry::text("bad"),
        },
    );
    assert!(sync_project(&out, &input).is_err());
    assert_eq!(
        std::fs::read(out.join(".whisker-fingerprint")).unwrap(),
        old
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn configuration_and_plist_values_are_serialized_without_loss() {
    let mut input = base();
    let app = input.project.apple.targets.get_mut("app").unwrap();
    app.info_plist
        .insert("Data".into(), PropertyListValue::Data(vec![0, 255]));
    app.info_plist.insert(
        "Date".into(),
        PropertyListValue::Date("2026-01-01T00:00:00Z".into()),
    );
    app.info_plist.insert(
        "Mixed".into(),
        PropertyListValue::Array(vec![string("a"), PropertyListValue::Integer(2)]),
    );
    app.configurations.insert(
        "Release".into(),
        AppleBuildConfiguration {
            xcconfig: Some(ProjectPath::new("Release.xcconfig").unwrap()),
            settings: [("SWIFT_VERSION".into(), setting("6.0"))].into(),
        },
    );
    let files = render_project(&input).unwrap();
    let plist = plist::Value::from_reader_xml(text(&files, "Info.plist").as_bytes()).unwrap();
    assert_eq!(
        plist.as_dictionary().unwrap()["Data"].as_data(),
        Some(&[0, 255][..])
    );
    assert!(
        text(&files, "NativeFixture.xcodeproj/project.pbxproj")
            .contains("baseConfigurationReference")
    );
    input
        .project
        .apple
        .targets
        .get_mut("app")
        .unwrap()
        .info_plist
        .insert("Bad".into(), PropertyListValue::Real(f64::INFINITY));
    assert!(render_project(&input).is_err());
}
#[test]
fn conditional_links_and_embeds_preserve_the_union_of_target_dependencies() {
    let mut input = with_share();
    let app = input.project.apple.targets.get_mut("app").unwrap();
    app.embeds.last_mut().unwrap().platform_filters = vec!["ios".into()];
    app.dependencies.push(AppleDependency::Target {
        target: "share".into(),
        link: false,
        weak: false,
        platform_filters: vec!["maccatalyst".into()],
    });
    let filters = |input: &IosProjectInputs| {
        let files = render_project(input).unwrap();
        let source = text(&files, "NativeFixture.xcodeproj/project.pbxproj");
        let pbx = plist::Value::from_reader_ascii(source.as_bytes()).unwrap();
        let objects = pbx.as_dictionary().unwrap()["objects"]
            .as_dictionary()
            .unwrap();
        let dep = objects
            .values()
            .find(|v| v.as_dictionary().unwrap()["isa"].as_string() == Some("PBXTargetDependency"))
            .unwrap()
            .as_dictionary()
            .unwrap();
        dep.get("platformFilters").map(|v| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_string().unwrap().to_owned())
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(
        filters(&input),
        Some(vec!["maccatalyst".into(), "ios".into()])
    );
    input
        .project
        .apple
        .targets
        .get_mut("app")
        .unwrap()
        .embeds
        .last_mut()
        .unwrap()
        .platform_filters
        .clear();
    assert_eq!(filters(&input), None);
}
#[test]
fn implicit_configurations_do_not_silently_drop_custom_target_or_scheme_settings() {
    let mut input = base();
    input.project.apple.configurations.clear();
    input.project.apple.default_configuration = None;
    input
        .project
        .apple
        .targets
        .get_mut("app")
        .unwrap()
        .configurations
        .insert("Staging".into(), Default::default());
    assert!(
        render_project(&input)
            .unwrap_err()
            .to_string()
            .contains("unknown target configuration")
    );
    input
        .project
        .apple
        .targets
        .get_mut("app")
        .unwrap()
        .configurations
        .clear();
    input
        .project
        .apple
        .schemes
        .get_mut("NativeFixture")
        .unwrap()
        .run_configuration = "Staging".into();
    assert!(
        render_project(&input)
            .unwrap_err()
            .to_string()
            .contains("unknown scheme configuration")
    );
    input
        .project
        .apple
        .configurations
        .insert("Staging".into(), Default::default());
    input
        .project
        .apple
        .configurations
        .insert("Release".into(), Default::default());
    input
        .project
        .apple
        .configurations
        .insert("Debug".into(), Default::default());
    render_project(&input).unwrap();
}
#[test]
#[ignore = "requires Xcode and an iOS Simulator SDK; run explicitly on macOS"]
fn xcode_builds_app_and_embedded_share_extension() {
    let root = temp();
    let mut input = with_share();
    // Compile the target graph in isolation from the separately tested Whisker
    // SDK/Cargo integration; no remote package resolution or signing is needed.
    input.project.apple.swift_packages.clear();
    input.project.apple.files.clear();
    let app = input.project.apple.targets.get_mut("app").unwrap();
    app.dependencies.clear();
    app.scripts.clear();
    app.resources.clear();
    app.embeds
        .retain(|e| matches!(e.source, AppleEmbedSource::Target { .. }));
    app.embeds[0].platform_filters = vec!["ios".into()];
    for k in [
        "OTHER_LDFLAGS",
        "FRAMEWORK_SEARCH_PATHS",
        "CODE_SIGN_IDENTITY",
    ] {
        app.build_settings.remove(k);
    }
    app.info_plist.remove("UILaunchStoryboardName");
    for (path, code) in [
        (
            "Sources/AppDelegate.swift",
            "import UIKit\n@main final class AppDelegate: UIResponder, UIApplicationDelegate {}\n",
        ),
        (
            "Share/ShareViewController.swift",
            "import UIKit\nfinal class ShareViewController: UIViewController {}\n",
        ),
    ] {
        input.project.apple.files.insert(
            ProjectPath::new(path).unwrap(),
            ProjectFile::Generated {
                entry: FileEntry::text(code),
            },
        );
    }
    sync_project(&root, &input).unwrap();
    let output = std::process::Command::new("xcodebuild")
        .args(["-project"])
        .arg(root.join("NativeFixture.xcodeproj"))
        .args([
            "-scheme",
            "NativeFixture",
            "-configuration",
            "Debug",
            "-sdk",
            "iphonesimulator",
            "-destination",
            "generic/platform=iOS Simulator",
            "-derivedDataPath",
        ])
        .arg(root.join("build"))
        .args(["CODE_SIGNING_ALLOWED=NO", "build"])
        .output()
        .unwrap();
    std::fs::write(
        root.join("xcodebuild.log"),
        [output.stdout, output.stderr].concat(),
    )
    .unwrap();
    assert!(
        output.status.success(),
        "xcodebuild failed; inspect {}",
        root.display()
    );
    assert!(
        root.join(
            "build/Build/Products/Debug-iphonesimulator/NativeFixture.app/PlugIns/Share.appex/Share"
        )
        .is_file()
    );
    eprintln!("native fixture built at {}", root.display());
}
