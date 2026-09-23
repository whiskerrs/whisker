#![cfg(feature = "generate")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use whisker_cng::{GenerationReport, GenerationTarget as Target, generate};

struct App(PathBuf);

impl App {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("whisker-generation-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        Self(root.canonicalize().unwrap())
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }

    fn manifest(&self) -> PathBuf {
        self.0.join("Cargo.toml")
    }

    fn cargo(&self, arguments: &[&str]) -> Output {
        Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(arguments)
            .current_dir(&self.0)
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .unwrap()
    }

    fn report(&self) -> GenerationReport {
        serde_json::from_slice(
            &std::fs::read(self.0.join("target/.whisker/generation.json")).unwrap(),
        )
        .unwrap()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn succeeded(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn direct_and_isolated_execution_generate_projects_with_plugins_and_feature_gating() {
    let app = App::new();
    let cng = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = cng.with_file_name("whisker-config");
    app.write(
        "Cargo.toml",
        &format!(
            r#"[package]
name = "generation-test-app"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
whisker-config = {{ path = {:?} }}
"#,
            config
        ),
    );
    app.write(
        "src/lib.rs",
        "compile_error!(\"the application must not be compiled\");",
    );
    app.write(
        "whisker.rs",
        r#"pub fn configure(app: &mut whisker_config::Config) {
    app.name("GenerationTest").bundle_id("test.generation");
}"#,
    );
    let legacy = generate(&app.manifest(), &[Target::Ios]).unwrap();
    assert!(
        legacy.projects[&Target::Ios]
            .gen_dir
            .join("GenerationTest.xcodeproj/project.pbxproj")
            .is_file()
    );
    assert!(!app.0.join("gen/android").exists());

    app.write(
        "Cargo.toml",
        &format!(
            r#"[package]
name = "generation-test-app"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
whisker-cng = {{ path = {:?}, default-features = false }}
cng-test-widget = {{ path = {:?}, optional = true }}
[[bin]]
name = "whisker-config"
path = "./whisker.rs"
required-features = ["whisker-config"]
test = false
bench = false
[features]
widgets = ["dep:cng-test-widget"]
extra = []
whisker-config = ["whisker-cng/generate"]
"#,
            cng,
            cng.join("../../tests/cng-module-fixture/widget")
        ),
    );
    app.write(
        "whisker.rs",
        r#"//! Project generation entry point.
fn main() {
    println!("configuration diagnostics need not be JSON");
    whisker_cng::run(|app| {
        app.name("GenerationTest").bundle_id("test.generation");
        app.android(|android| { android.min_sdk(24); });
    });
}"#,
    );
    let isolated = generate(&app.manifest(), &[]).unwrap();
    assert_eq!(isolated.projects.len(), 6);
    assert_eq!(isolated.config.android.min_sdk, Some(24));
    assert!(app.0.join("gen/android/settings.gradle.kts").is_file());
    assert!(app.0.join("gen/macos/Cargo.toml").is_file());
    assert!(app.0.join("gen/web/Cargo.toml").is_file());
    assert!(app.0.join("gen/windows/Cargo.toml").is_file());
    assert!(app.0.join("gen/linux/Cargo.toml").is_file());
    let project = app
        .0
        .join("gen/ios/GenerationTest.xcodeproj/project.pbxproj");
    let expected_project = std::fs::read(&project).unwrap();
    std::fs::remove_dir_all(app.0.join("gen/ios")).unwrap();
    let regenerated = generate(&app.manifest(), &[Target::Ios]).unwrap();
    assert!(regenerated.projects[&Target::Ios].regenerated);
    assert_eq!(std::fs::read(&project).unwrap(), expected_project);
    let reused = generate(&app.manifest(), &[Target::Ios]).unwrap();
    assert!(!reused.projects[&Target::Ios].regenerated);

    let selection = whisker_cng::CargoSelection {
        features: vec!["widgets".into(), "extra".into()],
        no_default_features: true,
        target: Some("aarch64-apple-ios".into()),
    };
    let enabled =
        whisker_cng::generate_with_selection(&app.manifest(), &[Target::Ios], &selection).unwrap();
    assert!(enabled.projects[&Target::Ios].regenerated);
    assert_eq!(enabled.selection, selection);
    let registrar = app
        .0
        .join("gen/ios/whisker_modules/Sources/WhiskerModules/RegisterAll.swift");
    assert!(
        std::fs::read_to_string(&registrar)
            .unwrap()
            .contains("CngTestWidget")
    );
    let lock = std::fs::read(app.0.join("Cargo.lock")).unwrap();
    let disabled = generate(&app.manifest(), &[Target::Ios]).unwrap();
    assert!(disabled.projects[&Target::Ios].regenerated);
    assert!(
        !std::fs::read_to_string(&registrar)
            .unwrap()
            .contains("CngTestWidget")
    );
    assert_eq!(std::fs::read(app.0.join("Cargo.lock")).unwrap(), lock);

    let gated = app.cargo(&["run", "--bin", "whisker-config"]);
    assert!(!gated.status.success());
    assert!(String::from_utf8_lossy(&gated.stderr).contains("requires the features"));
    let tree = succeeded(app.cargo(&["tree", "-e", "normal"]));
    let tree = String::from_utf8(tree.stdout).unwrap();
    assert!(!tree.contains("cargo_metadata v"), "{tree}");
    assert!(!tree.contains("image v"), "{tree}");

    app.write("src/lib.rs", "pub fn application() {}");
    let direct = succeeded(app.cargo(&[
        "run",
        "--quiet",
        "--bin",
        "whisker-config",
        "--features",
        "whisker-config",
        "--",
        "ios",
    ]));
    assert!(
        String::from_utf8(direct.stdout)
            .unwrap()
            .contains("configuration diagnostics")
    );
    assert_eq!(std::fs::read(&project).unwrap(), expected_project);
    assert_eq!(
        serde_json::to_value(&app.report().config).unwrap(),
        serde_json::to_value(&isolated.config).unwrap()
    );

    let asset = cng
        .join("../../packages/whisker-asset")
        .canonicalize()
        .unwrap();
    succeeded(app.cargo(&["add", "whisker-asset", "--path", asset.to_str().unwrap()]));
    app.write("assets/message.txt", "bundled by CNG");
    app.write(
        "whisker.rs",
        r#"fn main() {
    whisker_cng::run(|app| {
        app.name("GenerationTest").bundle_id("test.generation");
        app.project_plugin::<whisker_asset::WhiskerAsset>(|assets| { assets.dir("assets"); });
    });
}"#,
    );
    app.write(
        "src/lib.rs",
        "compile_error!(\"application must stay isolated\");",
    );
    app.write(
        ".cargo/config.toml",
        "[build]\ntarget = \"wasm32-unknown-unknown\"\n",
    );
    let assets = generate(&app.manifest(), &Target::ALL).unwrap();
    assert_eq!(assets.config.plugins["whisker-asset"]["dirs"][0], "assets");
    let ios = std::fs::read_to_string(&project).unwrap();
    assert!(ios.contains("whisker_assets"), "{ios}");
    assert_eq!(
        std::fs::read_to_string(app.0.join("gen/ios/whisker_assets/message.txt")).unwrap(),
        "bundled by CNG"
    );
    assert_eq!(
        std::fs::read_to_string(
            app.0
                .join("gen/android/app/src/main/assets/whisker/message.txt")
        )
        .unwrap(),
        "bundled by CNG"
    );

    let web_dir = app.0.join("gen/web");
    let plan = whisker_cng::web::load_build_plan(&web_dir).unwrap();
    let dist = whisker_cng::web::distribution_files(&web_dir, &plan).unwrap();
    assert_eq!(
        dist[&whisker_plugin::project::ProjectPath::new("message.txt").unwrap()]
            .to_bytes()
            .unwrap(),
        b"bundled by CNG"
    );

    check_project_plugin(&app, cng);

    app.write(
        "whisker.rs",
        r#"fn main() {
    whisker_cng::run(|app| {
        app.name("GenerationTest").bundle_id("test.generation");
        app.project_plugin::<whisker_asset::WhiskerAsset>(|assets| { assets.dir("missing-assets"); });
    });
}"#,
    );
    assert!(generate(&app.manifest(), &[Target::Ios]).is_err());
    assert!(!app.0.join("target/.whisker/generation.json").exists());

    std::fs::remove_file(app.0.join(".cargo/config.toml")).unwrap();
    app.write("src/lib.rs", "pub fn application() {}");
    app.write(
        "whisker.rs",
        "compile_error!(\"generator must be skipped\"); fn main() {}",
    );
    succeeded(app.cargo(&["build", "--quiet"]));
    succeeded(app.cargo(&[
        "rustc",
        "--quiet",
        "--crate-type",
        "cdylib",
        "--",
        "-C",
        "debuginfo=0",
    ]));
}

// Exercise actual Cargo discovery, binary compilation, protocol handshake, IR
// composition and renderer through the standard generator, including features.
fn check_project_plugin(app: &App, cng: &Path) {
    let original_manifest = std::fs::read_to_string(app.manifest()).unwrap();
    let original_config = std::fs::read_to_string(app.0.join("whisker.rs")).unwrap();
    let plugin_path = cng.with_file_name("whisker-plugin");
    app.write(
        "fixture-project-plugin/Cargo.toml",
        &format!(
            r#"
[package]
name = "fixture-project-plugin"
version = "0.0.0"
edition = "2024"
[dependencies]
whisker-plugin = {{ path = {:?} }}
anyhow = "1"
serde = {{ version = "1", features = ["derive"] }}
[package.metadata.whisker.plugins.fixture-project]
bin = "fixture-project-plugin"
protocol = "project"
"#,
            plugin_path
        ),
    );
    app.write("fixture-project-plugin/src/lib.rs", r#"
use whisker_plugin::{PluginConfig, project::*};
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Options;
impl PluginConfig for Options { const NAME: &'static str = "fixture-project"; }
pub struct Fixture;
impl ProjectPlugin for Fixture {
    type Config = Options;
    fn contribute(&self, context: &ProjectContext, _: &Options) -> anyhow::Result<ProjectUpdate> {
        if matches!(context.project, ProjectIr::Windows(_) | ProjectIr::Linux(_)) {
            let mut project = context.project.clone();
            let files = match &mut project { ProjectIr::Windows(w) => &mut w.files, ProjectIr::Linux(l) => &mut l.files, _ => unreachable!() };
            files.insert(ProjectPath::new("feature-selected.txt")?, ProjectFile::Generated { entry: whisker_plugin::FileEntry::text("selected") });
            return Ok(ProjectUpdate::Merge { project: Box::new(project) });
        }
        if let ProjectIr::Macos(macos) = &context.project {
            let mut project = macos.clone();
            project.apple.targets.get_mut(&project.apple.application).unwrap().info_plist.insert("CngProjectFeature".into(), PropertyListValue::Boolean(true));
            return Ok(ProjectUpdate::Merge { project: Box::new(ProjectIr::Macos(project)) });
        }
        if let ProjectIr::Web(web) = &context.project {
            anyhow::ensure!(web.wasm.is_some() && !web.body.is_empty(), "application must precede feature plugins");
            let mut project = web.clone();
            project.title = "ProjectPluginWeb".into();
            return Ok(ProjectUpdate::Replace { project: Box::new(ProjectIr::Web(project)), reason: "override Web title".into() });
        }
        if let ProjectIr::Ios(ios) = &context.project {
            anyhow::ensure!(ios.apple.targets.contains_key("app"), "application must precede feature plugins");
            let mut project = ios.clone();
            project.apple.targets.insert("plugin-task".into(), AppleTarget {
                product_name: "PluginTask".into(), kind: AppleTargetKind::Aggregate,
                ..Default::default()
            });
            return Ok(ProjectUpdate::Merge { project: Box::new(ProjectIr::Ios(project)) });
        }
        let ProjectIr::Android(android) = &context.project else { return Ok(ProjectUpdate::Keep); };
        anyhow::ensure!(android.application == ":app" && android.modules.contains_key(":app"), "application must be declared before feature plugins");
        let mut project = android.clone();
        project.modules.insert(":extra".into(), AndroidModule {
            directory: ProjectPath::new("extra")?, kind: AndroidModuleKind::Custom,
            build: GradleBuildScript { statements: vec!["tasks.register(\"fromProjectPlugin\")".into()], ..Default::default() },
            dependencies: vec![],
        });
        Ok(ProjectUpdate::Merge { project: Box::new(ProjectIr::Android(project)) })
    }
}
"#);
    // Keep the executable independent of whisker-cng and the runtime application.
    app.write("fixture-project-plugin/src/main.rs", "fn main() -> anyhow::Result<()> { whisker_plugin::project::protocol::run_as_subprocess(fixture_project_plugin::Fixture) }");
    let mut manifest: toml::Value = toml::from_str(&original_manifest).unwrap();
    manifest["dependencies"].as_table_mut().unwrap().insert(
        "fixture-project-plugin".into(),
        toml::Value::try_from(serde_json::json!({"path":"fixture-project-plugin","optional":true}))
            .unwrap(),
    );
    manifest["features"].as_table_mut().unwrap().insert(
        "project-ir".into(),
        toml::Value::Array(vec!["dep:fixture-project-plugin".into()]),
    );
    app.write("Cargo.toml", &toml::to_string(&manifest).unwrap());
    generate(&app.manifest(), &Target::ALL).unwrap();
    assert!(!app.0.join("gen/android/extra").exists());
    let ios_has_plugin = |scheme: &str| {
        std::fs::read_to_string(
            app.0
                .join(format!("gen/ios/{scheme}.xcodeproj/project.pbxproj")),
        )
        .unwrap()
        .contains("PluginTask")
    };
    let initial_scheme = std::fs::read_dir(app.0.join("gen/ios"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "xcodeproj"))
        .unwrap()
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(!ios_has_plugin(&initial_scheme));
    let selection = whisker_cng::CargoSelection {
        features: vec!["project-ir".into()],
        ..Default::default()
    };
    whisker_cng::generate_with_selection(&app.manifest(), &Target::ALL, &selection).unwrap();
    assert!(
        std::fs::read_to_string(app.0.join("gen/android/extra/build.gradle.kts"))
            .unwrap()
            .contains("fromProjectPlugin")
    );
    assert!(
        app.0
            .join("gen/android/app/src/main/assets/whisker/message.txt")
            .exists()
    );
    assert!(ios_has_plugin(&initial_scheme));
    assert!(
        std::fs::read_to_string(app.0.join("gen/web/index.html"))
            .unwrap()
            .contains("<title>ProjectPluginWeb</title>")
    );
    assert!(app.0.join("gen/ios/whisker_assets/message.txt").is_file());
    assert!(
        std::fs::read_to_string(app.0.join("gen/macos/Info.plist"))
            .unwrap()
            .contains("CngProjectFeature")
    );
    assert!(app.0.join("gen/macos/whisker_assets/message.txt").is_file());
    for platform in ["windows", "linux"] {
        let out = app.0.join("gen").join(platform);
        assert!(out.join("feature-selected.txt").is_file());
        assert!(out.join("whisker_assets/message.txt").is_file());
        let plan = whisker_cng::desktop::load_build_plan(&out).unwrap();
        assert!(
            plan.files
                .keys()
                .any(|p| p.as_str().ends_with("whisker_assets/message.txt"))
        );
    }
    // Removing a discovered project plugin removes its declarations too.
    manifest["dependencies"]
        .as_table_mut()
        .unwrap()
        .remove("whisker-asset");
    app.write("Cargo.toml", &toml::to_string(&manifest).unwrap());
    app.write(
        "whisker.rs",
        r#"fn main() { whisker_cng::run(|app| { app.name("Pure").bundle_id("test.pure"); }); }"#,
    );
    whisker_cng::generate_with_selection(&app.manifest(), &Target::ALL, &selection).unwrap();
    assert!(app.0.join("gen/android/extra/build.gradle.kts").exists());
    assert!(ios_has_plugin("Pure"));
    generate(&app.manifest(), &Target::ALL).unwrap();
    assert!(!app.0.join("gen/android/extra").exists());
    assert!(!ios_has_plugin("Pure"));
    assert!(
        !std::fs::read_to_string(app.0.join("gen/macos/Info.plist"))
            .unwrap()
            .contains("CngProjectFeature")
    );
    assert!(!app.0.join("gen/macos/whisker_assets/message.txt").exists());
    assert!(
        !std::fs::read_to_string(app.0.join("gen/web/index.html"))
            .unwrap()
            .contains("ProjectPluginWeb")
    );
    for platform in ["windows", "linux"] {
        let out = app.0.join("gen").join(platform);
        assert!(!out.join("feature-selected.txt").exists());
        assert!(!out.join("whisker_assets/message.txt").exists());
    }
    app.write("Cargo.toml", &original_manifest);
    app.write("whisker.rs", &original_config);
}
