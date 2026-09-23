//! Exercise the same application → asset plugin → renderer path as CNG.
use std::path::{Path, PathBuf};
use whisker_asset::{WhiskerAsset, WhiskerAssetConfig};
use whisker_cng::{Config, Engine, ProjectEngine, android, ios, web};
use whisker_plugin::{FileEntry, project::*};

struct App(PathBuf);
impl App {
    fn new() -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "whisker-asset-project-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    fn config(&self) -> Config {
        let mut cfg = Config::default();
        cfg.name("AssetDemo").bundle_id("rs.whisker.assetdemo");
        cfg.project_plugin::<WhiskerAsset>(|c| {
            c.dir("assets").file("branding/logo.bin");
        });
        cfg.web(|w| {
            w.base_path("/app/");
        });
        cfg
    }
    fn compose(&self, platform: &str, cfg: &Config) -> anyhow::Result<ProjectIr> {
        // Build application inputs with no dependency plugin execution; the
        // project engine invokes WhiskerAsset after the initializer.
        let inputs_cfg = Config {
            plugins: Default::default(),
            ..self.config()
        };
        let (mut engine, initial) = match platform {
            "android" => (
                ProjectEngine::with_android_application(android::inputs_from_with_engine(
                    &Engine::new(),
                    &inputs_cfg,
                    "asset_demo".into(),
                    "../..".into(),
                    "asset-demo".into(),
                    "0.1.0".into(),
                    "0.1.0".into(),
                    "https://example.invalid/maven".into(),
                )?),
                ProjectIr::Android(Box::default()),
            ),
            "ios" => (
                ProjectEngine::with_ios_application(ios::inputs_from_with_engine(
                    &Engine::new(),
                    &inputs_cfg,
                    self.0.join("gen/ios/whisker_modules"),
                    self.0.clone(),
                    "asset-demo".into(),
                )?),
                ProjectIr::Ios(Default::default()),
            ),
            "web" => (
                ProjectEngine::with_web_application(web::inputs_from(
                    &inputs_cfg,
                    "asset-demo".into(),
                    self.0.clone(),
                    "\"0.14\"".into(),
                )?),
                ProjectIr::Web(Box::default()),
            ),
            "macos" => (
                ProjectEngine::with_macos_application(whisker_cng::macos::inputs_from(
                    &inputs_cfg,
                    "asset-demo".into(),
                    self.0.clone(),
                    "\"0.14\"".into(),
                )?),
                ProjectIr::Macos(Default::default()),
            ),
            "windows" | "linux" => {
                let platform = platform.parse().unwrap();
                let inputs = whisker_cng::desktop::inputs_from(
                    &inputs_cfg,
                    platform,
                    "asset-demo".into(),
                    self.0.clone(),
                    "\"0.14\"".into(),
                )?;
                let empty = inputs.empty_project()?;
                (
                    if platform == whisker_cng::GenerationTarget::Windows {
                        ProjectEngine::with_windows_application(inputs)
                    } else {
                        ProjectEngine::with_linux_application(inputs)
                    },
                    empty,
                )
            }
            _ => unreachable!(),
        };
        engine = engine.with_app_crate_dir(&self.0);
        engine.register(WhiskerAsset);
        Ok(engine.compose(cfg, &initial)?.project)
    }
    fn web(&self) -> web::WebProjectInputs {
        let ProjectIr::Web(project) = self.compose("web", &self.config()).unwrap() else {
            panic!()
        };
        web::WebProjectInputs {
            project: *project,
            app_crate_dir: Some(self.0.clone()),
            cargo_selection: Default::default(),
            template_version: 16,
        }
    }
    fn fixtures(&self) {
        self.write("assets/photos/nested/pic.jpg", &[0, 255, 42]);
        self.write("assets/settings.json", b"{\"k\":1}");
        self.write("branding/logo.bin", &[0, 1, 128]);
    }
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn path(s: &str) -> ProjectPath {
    ProjectPath::new(s).unwrap()
}
fn text(files: &std::collections::BTreeMap<ProjectPath, FileEntry>, name: &str) -> String {
    String::from_utf8(files[&path(name)].to_bytes().unwrap()).unwrap()
}

#[test]
fn android_stages_assets_and_registers_the_main_source_set() {
    let app = App::new();
    app.fixtures();
    let ProjectIr::Android(project) = app.compose("android", &app.config()).unwrap() else {
        panic!()
    };
    let input = android::AndroidProjectInputs {
        project: *project,
        app_crate_dir: Some(app.0.clone()),
        cargo_selection: Default::default(),
        template_version: 38,
    };
    let out = app.0.join("gen/android");
    android::sync_project(&out, &input).unwrap();
    assert_eq!(
        std::fs::read(out.join("app/src/main/assets/whisker/photos/nested/pic.jpg")).unwrap(),
        [0, 255, 42]
    );
    assert_eq!(
        std::fs::read(out.join("app/src/main/assets/whisker/logo.bin")).unwrap(),
        [0, 1, 128]
    );
    let gradle = std::fs::read_to_string(out.join("app/build.gradle.kts")).unwrap();
    assert!(
        gradle.contains("assets") && gradle.contains("src/main/assets"),
        "{gradle}"
    );
    // Input bytes, not only the list of paths, affect the generation fingerprint.
    assert!(!android::sync_project(&out, &input).unwrap());
    app.write("assets/settings.json", b"changed");
    assert!(android::sync_project(&out, &input).unwrap());
    assert_eq!(
        std::fs::read(out.join("app/src/main/assets/whisker/settings.json")).unwrap(),
        b"changed"
    );
}
#[test]
fn ios_stages_assets_and_copies_one_folder_preserving_subdirectories() {
    let app = App::new();
    app.fixtures();
    let ProjectIr::Ios(project) = app.compose("ios", &app.config()).unwrap() else {
        panic!()
    };
    let input = ios::IosProjectInputs {
        project,
        project_name: "AssetDemo".into(),
        app_crate_dir: Some(app.0.clone()),
        cargo_selection: Default::default(),
        template_version: 40,
    };
    let out = app.0.join("gen/ios");
    ios::sync_project(&out, &input).unwrap();
    assert_eq!(
        std::fs::read(out.join("whisker_assets/photos/nested/pic.jpg")).unwrap(),
        [0, 255, 42]
    );
    assert_eq!(
        std::fs::read(out.join("whisker_assets/logo.bin")).unwrap(),
        [0, 1, 128]
    );
    let pbx = std::fs::read_to_string(out.join("AssetDemo.xcodeproj/project.pbxproj")).unwrap();
    assert!(
        pbx.contains("lastKnownFileType = folder;") && pbx.contains("path = \"whisker_assets\""),
        "{pbx}"
    );
    assert!(pbx.contains("whisker_assets in Resources"));
    assert_eq!(input.project.apple.targets[&input.project.apple.application].resources.iter().filter(|r| matches!(r, AppleResource::Copy { resource } if resource.destination == path("whisker_assets"))).count(), 1);
}
#[test]
fn web_distributes_only_declared_assets_and_regeneration_removes_stale_files() {
    let app = App::new();
    app.fixtures();
    app.write("private.txt", b"private");
    let input = app.web();
    let out = app.0.join("gen/web");
    web::sync_project(&out, &input).unwrap();
    let dist = web::distribution_files(&out, &web::load_build_plan(&out).unwrap()).unwrap();
    assert_eq!(
        dist[&path("photos/nested/pic.jpg")].to_bytes().unwrap(),
        [0, 255, 42]
    );
    assert_eq!(dist[&path("logo.bin")].to_bytes().unwrap(), [0, 1, 128]);
    let html = text(&dist, "index.html");
    assert!(
        html.contains("content=\"/app/\" name=\"whisker-asset-base\""),
        "{html}"
    );
    assert!(!dist.contains_key(&path("Cargo.toml")) && !dist.contains_key(&path("private.txt")));
    assert!(!web::sync_project(&out, &input).unwrap());
    app.write("assets/settings.json", b"changed");
    assert!(web::sync_project(&out, &input).unwrap());
    std::fs::remove_file(app.0.join("assets/settings.json")).unwrap();
    app.write("assets/new.txt", b"new");
    web::sync_project(&out, &app.web()).unwrap();
    let dist = web::distribution_files(&out, &web::load_build_plan(&out).unwrap()).unwrap();
    assert!(!dist.contains_key(&path("settings.json")));
    assert!(!out.join("whisker_assets/settings.json").exists());
    assert_eq!(text(&dist, "new.txt"), "new");
}
#[test]
fn bad_inputs_and_output_collisions_fail_without_overwriting_a_project() {
    let app = App::new();
    app.fixtures();
    for (dir, file, expected) in [
        ("absent", "branding/logo.bin", "does not exist"),
        ("assets", "absent", "does not exist"),
        ("assets", "assets/settings.json", "collide"),
    ] {
        let mut cfg = app.config();
        cfg.project_plugin::<WhiskerAsset>(|c| {
            c.dir(dir).file(file);
        });
        let err = app.compose("web", &cfg).unwrap_err();
        assert!(format!("{err:#}").contains(expected), "{err:#}");
    }
    let out = app.0.join("gen/web");
    web::sync_project(&out, &app.web()).unwrap();
    let before = std::fs::read(out.join(".whisker-fingerprint")).unwrap();
    app.write("assets/index.html", b"collision");
    assert!(app.compose("web", &app.config()).is_err());
    assert_eq!(
        std::fs::read(out.join(".whisker-fingerprint")).unwrap(),
        before
    );
    std::fs::remove_file(app.0.join("assets/index.html")).unwrap();
    let mut input = app.web();
    input.project.files.insert(
        path("whisker_assets/settings.json"),
        ProjectFile::Generated {
            entry: FileEntry::text("another owner"),
        },
    );
    let mut current = ProjectIr::Web(Box::new(input.project));
    let update = WhiskerAsset
        .contribute(
            &ProjectContext {
                project: current.clone(),
                app_crate_dir: Some(app.0.clone()),
            },
            &WhiskerAssetConfig {
                dirs: vec!["assets".into()],
                files: vec![],
            },
        )
        .unwrap();
    let before = current.clone();
    assert!(update.apply_to(&mut current).is_err());
    assert_eq!(current, before);
}
#[test]
fn no_assets_is_a_noop_and_paths_must_be_relative() {
    let ctx = ProjectContext {
        project: ProjectIr::Web(Box::default()),
        app_crate_dir: None,
    };
    assert_eq!(
        WhiskerAsset
            .contribute(&ctx, &WhiskerAssetConfig::default())
            .unwrap(),
        ProjectUpdate::Keep
    );
    for name in ["../assets", "/assets", "C:\\assets", "assets/../outside"] {
        let mut cfg = WhiskerAssetConfig::default();
        cfg.dir(name);
        assert!(WhiskerAsset.validate(&cfg).is_err(), "{name}");
    }
    let mut cfg = WhiskerAssetConfig::default();
    cfg.dir("./assets");
    WhiskerAsset.validate(&cfg).unwrap();
    assert!(
        WhiskerAsset
            .contribute(&ctx, &cfg)
            .unwrap_err()
            .to_string()
            .contains("app crate dir")
    );
    let app = App::new();
    std::fs::create_dir(app.0.join("assets")).unwrap();
    let ctx = ProjectContext {
        app_crate_dir: Some(app.0.clone()),
        ..ctx
    };
    assert_eq!(
        WhiskerAsset.contribute(&ctx, &cfg).unwrap(),
        ProjectUpdate::Keep
    );
}
#[cfg(unix)]
#[test]
fn symlink_roots_children_and_ancestors_are_rejected() {
    use std::os::unix::fs::symlink;
    let app = App::new();
    app.fixtures();
    symlink(app.0.join("assets"), app.0.join("linked")).unwrap();
    for name in ["linked", "linked/photos"] {
        let mut cfg = app.config();
        cfg.project_plugin::<WhiskerAsset>(|c| {
            c.dir(name);
        });
        assert!(format!("{:#}", app.compose("web", &cfg).unwrap_err()).contains("symlink"));
    }
    symlink(Path::new(".."), app.0.join("assets/loop")).unwrap();
    assert!(format!("{:#}", app.compose("web", &app.config()).unwrap_err()).contains("symlink"));
}
#[test]
fn android_uses_current_application_identity_and_directory() {
    let app = App::new();
    app.fixtures();
    let ProjectIr::Android(mut project) = app.compose("android", &Config::default()).unwrap()
    else {
        panic!()
    };
    let mut module = project.modules.remove(&project.application).unwrap();
    module.directory = path("custom-app");
    project.application = ":custom".into();
    project.modules.insert(":custom".into(), module);
    let mut current = ProjectIr::Android(project);
    let update = WhiskerAsset
        .contribute(
            &ProjectContext {
                project: current.clone(),
                app_crate_dir: Some(app.0.clone()),
            },
            &WhiskerAssetConfig {
                dirs: vec!["assets".into()],
                files: vec![],
            },
        )
        .unwrap();
    update.apply_to(&mut current).unwrap();
    let ProjectIr::Android(project) = current else {
        panic!()
    };
    assert!(
        project
            .files
            .contains_key(&path("custom-app/src/main/assets/whisker/settings.json"))
    );
    assert!(!project.modules.contains_key(":app"));
}

#[test]
fn macos_asset_resource_is_part_of_the_bundle_plan() {
    let app = App::new();
    app.fixtures();
    let ProjectIr::Macos(project) = app.compose("macos", &app.config()).unwrap() else {
        panic!()
    };
    let inputs = whisker_cng::macos::MacosProjectInputs {
        project,
        app_crate_dir: Some(app.0.clone()),
        cargo_selection: Default::default(),
        template_version: 12,
    };
    let out = app.0.join("gen/macos");
    whisker_cng::macos::sync_project(&out, &inputs).unwrap();
    let plan = whisker_cng::macos::load_build_plan(&out).unwrap();
    let files = whisker_cng::macos::bundle_files(&out, &plan).unwrap();
    assert_eq!(
        files[&path("Contents/Resources/whisker_assets/photos/nested/pic.jpg")]
            .to_bytes()
            .unwrap(),
        [0, 255, 42]
    );
    assert_eq!(
        files[&path("Contents/Resources/whisker_assets/logo.bin")]
            .to_bytes()
            .unwrap(),
        [0, 1, 128]
    );
}

#[test]
fn windows_and_linux_distribute_assets_for_runtime_relative_to_the_executable() {
    let app = App::new();
    app.fixtures();
    for platform in ["windows", "linux"] {
        let inputs = whisker_cng::desktop::DesktopProjectInputs {
            project: app.compose(platform, &app.config()).unwrap(),
            app_crate_dir: Some(app.0.clone()),
            cargo_selection: Default::default(),
        };
        let out = app.0.join(platform);
        assert!(whisker_cng::desktop::sync_project(&out, &inputs).unwrap());
        let plan = whisker_cng::desktop::load_build_plan(&out).unwrap();
        let files = whisker_cng::desktop::distribution_files(&out, &plan).unwrap();
        let prefix = if platform == "windows" {
            "whisker_assets"
        } else {
            "share/asset-demo-whisker-linux/whisker_assets"
        };
        assert_eq!(
            files[&path(&format!("{prefix}/photos/nested/pic.jpg"))]
                .to_bytes()
                .unwrap(),
            [0, 255, 42]
        );
        assert_eq!(
            files[&path(&format!("{prefix}/logo.bin"))]
                .to_bytes()
                .unwrap(),
            [0, 1, 128]
        );
        assert!(!plan.files.keys().any(|p| p.as_str() == "Cargo.toml"));
    }
}
