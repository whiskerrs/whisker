use std::path::{Path, PathBuf};
use whisker_cng::{
    CargoSelection, Config, Engine, ProjectDependencyGraph, ProjectEngine, android, ios,
};
use whisker_firebase::WhiskerFirebase;
use whisker_plugin::project::*;

struct App(PathBuf);
impl App {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "whisker-firebase-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("example");
        for name in ["google-services.json", "GoogleService-Info.plist"] {
            std::fs::copy(fixture.join(name), path.join(name)).unwrap();
        }
        Self(path.canonicalize().unwrap())
    }
    fn config(&self) -> Config {
        let mut config = Config::default();
        config
            .name("FirebaseTest")
            .bundle_id("rs.whisker.firebaseexample");
        config.android(|android| {
            android.min_sdk(21);
        });
        config
    }
    fn compose(&self, platform: &str, config: &Config) -> anyhow::Result<ProjectIr> {
        self.compose_with(platform, config, &[])
    }
    fn compose_with(
        &self,
        platform: &str,
        config: &Config,
        services: &[&str],
    ) -> anyhow::Result<ProjectIr> {
        let (engine, empty) = if platform == "android" {
            (
                ProjectEngine::with_android_application(android::inputs_from_with_engine(
                    &Engine::new(),
                    &self.config(),
                    "fixture".into(),
                    "../..".into(),
                    "fixture".into(),
                    "0.1.21".into(),
                    "0.1.0".into(),
                    "https://example.invalid/maven".into(),
                )?),
                ProjectIr::Android(Box::default()),
            )
        } else {
            (
                ProjectEngine::with_ios_application(ios::inputs_from_with_engine(
                    &Engine::new(),
                    &self.config(),
                    self.0.join("whisker_modules"),
                    self.0.clone(),
                    "fixture".into(),
                )?),
                ProjectIr::Ios(Default::default()),
            )
        };
        let mut engine = engine.with_app_crate_dir(&self.0);
        if services.contains(&"crashlytics") {
            engine.register(whisker_firebase_crashlytics::WhiskerFirebaseCrashlytics);
        }
        if services.contains(&"auth") {
            engine.register(whisker_firebase_auth::WhiskerFirebaseAuth);
        }
        engine.register(WhiskerFirebase);
        Ok(engine.compose(config, &empty)?.project)
    }
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn configuration_reaches_native_renderers_and_preserves_the_application() {
    let app = App::new();
    let ProjectIr::Android(android) = app.compose("android", &app.config()).unwrap() else {
        panic!()
    };
    let main = &android.modules[&android.application];
    let AndroidModuleKind::Application(application) = &main.kind else {
        panic!()
    };
    assert_eq!(
        application.android.sdk.min,
        Some(AndroidApiLevel::Release(23))
    );
    assert!(
        main.build
            .plugins
            .iter()
            .any(|p| p.id == "com.google.gms.google-services" && p.apply)
    );
    let inputs = android::AndroidProjectInputs {
        project: *android,
        app_crate_dir: Some(app.0.clone()),
        cargo_selection: Default::default(),
        template_version: 1,
    };
    let files = android::render_project(&inputs).unwrap();
    assert_eq!(
        files[&ProjectPath::new("app/google-services.json").unwrap()]
            .to_bytes()
            .unwrap(),
        std::fs::read(app.0.join("google-services.json")).unwrap()
    );

    let ProjectIr::Ios(ios) = app.compose("ios", &app.config()).unwrap() else {
        panic!()
    };
    let target = &ios.apple.targets[&ios.apple.application];
    assert!(!target.sources.is_empty());
    assert!(target.resources.iter().any(|r| matches!(r, AppleResource::Copy { resource } if resource.destination.as_str() == "GoogleService-Info.plist")));
    let files = ios::render_project(&ios::IosProjectInputs {
        project: ios,
        project_name: "FirebaseTest".into(),
        app_crate_dir: Some(app.0.clone()),
        cargo_selection: Default::default(),
        template_version: 1,
    })
    .unwrap();
    assert!(files.contains_key(&ProjectPath::new("firebase/GoogleService-Info.plist").unwrap()));
}

#[test]
fn missing_malformed_and_mismatched_configuration_fail_before_generation() {
    let app = App::new();
    std::fs::write(app.0.join("google-services.json"), "not json").unwrap();
    assert!(
        format!("{:#}", app.compose("android", &app.config()).unwrap_err())
            .contains("invalid google-services.json")
    );
    std::fs::remove_file(app.0.join("GoogleService-Info.plist")).unwrap();
    assert!(app.compose("ios", &app.config()).is_err());
    let app = App::new();
    let file = app.0.join("google-services.json");
    let wrong = std::fs::read_to_string(&file)
        .unwrap()
        .replace("rs.whisker.firebaseexample", "com.other.app");
    std::fs::write(file, wrong).unwrap();
    assert!(
        format!("{:#}", app.compose("android", &app.config()).unwrap_err()).contains("no client")
    );
    let file = app.0.join("GoogleService-Info.plist");
    let wrong = std::fs::read_to_string(&file)
        .unwrap()
        .replace("rs.whisker.firebaseexample", "com.other.app");
    std::fs::write(file, wrong).unwrap();
    assert!(format!("{:#}", app.compose("ios", &app.config()).unwrap_err()).contains("BUNDLE_ID"));
    let app = App::new();
    for name in ["google-services.json", "GoogleService-Info.plist"] {
        let file = app.0.join(name);
        let long_key = std::fs::read_to_string(&file)
            .unwrap()
            .replace("_12345", "_1234567");
        std::fs::write(file, long_key).unwrap();
    }
    for platform in ["android", "ios"] {
        assert!(
            format!("{:#}", app.compose(platform, &app.config()).unwrap_err())
                .contains("malformed API key"),
            "{platform}"
        );
    }
}

#[test]
fn service_features_control_the_actual_native_module_graph() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("example/Cargo.toml");
    let services = [
        "firestore",
        "auth",
        "storage",
        "messaging",
        "analytics",
        "crashlytics",
    ];
    for target in ["aarch64-apple-ios", "aarch64-linux-android"] {
        for enabled in [
            &[][..],
            &["firestore"],
            &["auth", "storage"],
            &["messaging"],
            &["analytics", "crashlytics"],
            &services,
        ] {
            let graph = ProjectDependencyGraph::resolve_with_selection(
                &manifest,
                "whisker-firebase-example",
                &CargoSelection {
                    features: enabled.iter().map(|s| s.to_string()).collect(),
                    no_default_features: true,
                    target: Some(target.into()),
                },
            )
            .unwrap();
            let has = |package: &str| graph.modules.iter().any(|m| m.package == package);
            assert!(has("whisker-firebase-core"));
            for service in services {
                assert_eq!(
                    has(&format!("whisker-firebase-{service}")),
                    enabled.contains(&service),
                    "{target} {enabled:?} {service}"
                );
            }
            let plugin = |name: &str| graph.cng_plugins.iter().any(|p| p.name == name);
            assert!(plugin("whisker-firebase"));
            for service in ["auth", "messaging", "analytics", "crashlytics"] {
                assert_eq!(
                    plugin(&format!("whisker-firebase-{service}")),
                    enabled.contains(&service),
                    "{target} {enabled:?} {service} plugin"
                );
            }
        }
    }
}

#[test]
fn crashlytics_gradle_plugin_is_applied_after_google_services() {
    let app = App::new();
    let ProjectIr::Android(android) = app
        .compose_with("android", &app.config(), &["crashlytics"])
        .unwrap()
    else {
        panic!()
    };
    let files = android::render_project(&android::AndroidProjectInputs {
        project: *android,
        app_crate_dir: Some(app.0.clone()),
        cargo_selection: Default::default(),
        template_version: 1,
    })
    .unwrap();
    let build = String::from_utf8(
        files[&ProjectPath::new("app/build.gradle.kts").unwrap()]
            .to_bytes()
            .unwrap(),
    )
    .unwrap();
    let services = build.find("com.google.gms.google-services").unwrap();
    let crashlytics = build.find("com.google.firebase.crashlytics").unwrap();
    assert!(services < crashlytics, "{build}");
}

const PHONE_SCHEME: &str = "app-1-1234567890-ios-0123456789abcdef0123456789abcdef";

fn url_schemes(ios: &IosProjectIr) -> Vec<PropertyListValue> {
    match ios.apple.targets[&ios.apple.application]
        .info_plist
        .get("CFBundleURLTypes")
    {
        Some(PropertyListValue::Array(types)) => types
            .iter()
            .flat_map(|url_type| match url_type {
                PropertyListValue::Dict(fields) => match fields.get("CFBundleURLSchemes") {
                    Some(PropertyListValue::Array(schemes)) => schemes.clone(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn compose_auth_ios(app: &App, config: &Config) -> anyhow::Result<IosProjectIr> {
    match app.compose_with("ios", config, &["auth"])? {
        ProjectIr::Ios(ios) => Ok(ios),
        _ => unreachable!(),
    }
}

#[test]
fn auth_registers_the_phone_url_scheme_once_from_the_shared_ios_config() {
    let app = App::new();
    std::fs::create_dir(app.0.join("config")).unwrap();
    std::fs::rename(
        app.0.join("GoogleService-Info.plist"),
        app.0.join("config/Custom-Info.plist"),
    )
    .unwrap();
    let mut config = app.config();
    config.project_plugin::<WhiskerFirebase>(|firebase| {
        firebase.ios_config = "config/Custom-Info.plist".into();
    });
    let mut ios = compose_auth_ios(&app, &config).unwrap();
    let scheme = PropertyListValue::String(PHONE_SCHEME.into());
    assert_eq!(url_schemes(&ios), vec![scheme.clone()]);

    // Keep schemes from other plugins and do not add ours twice.
    let application = ios.apple.application.clone();
    let Some(PropertyListValue::Array(types)) = ios
        .apple
        .targets
        .get_mut(&application)
        .unwrap()
        .info_plist
        .get_mut("CFBundleURLTypes")
    else {
        panic!()
    };
    types.insert(
        0,
        PropertyListValue::Dict(
            [(
                "CFBundleURLSchemes".into(),
                PropertyListValue::Array(vec![PropertyListValue::String("other".into())]),
            )]
            .into(),
        ),
    );
    let context = ProjectContext {
        project: ProjectIr::Ios(ios),
        app_crate_dir: Some(app.0.clone()),
    };
    let ProjectUpdate::Replace { project, .. } = whisker_firebase_auth::WhiskerFirebaseAuth
        .contribute(&context, &Default::default())
        .unwrap()
    else {
        panic!("expected a replacement")
    };
    let ProjectIr::Ios(ios) = *project else {
        panic!()
    };
    assert_eq!(
        url_schemes(&ios),
        vec![PropertyListValue::String("other".into()), scheme]
    );
}

#[test]
fn auth_scheme_can_be_turned_off_and_needs_the_firebase_plist() {
    let app = App::new();
    let mut config = app.config();
    config.project_plugin::<whisker_firebase_auth::WhiskerFirebaseAuth>(|auth| {
        auth.phone_url_scheme = false;
    });
    assert!(url_schemes(&compose_auth_ios(&app, &config).unwrap()).is_empty());

    let context = ProjectContext {
        project: ProjectIr::Ios(Default::default()),
        app_crate_dir: Some(app.0.clone()),
    };
    let error = whisker_firebase_auth::WhiskerFirebaseAuth
        .contribute(&context, &Default::default())
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("whisker-firebase-auth"),
        "{error:#}"
    );
}

#[cfg(unix)]
#[test]
fn configuration_in_a_symlinked_directory_is_rejected() {
    let app = App::new();
    std::fs::create_dir(app.0.join("config")).unwrap();
    std::fs::rename(
        app.0.join("GoogleService-Info.plist"),
        app.0.join("config/GoogleService-Info.plist"),
    )
    .unwrap();
    std::os::unix::fs::symlink(app.0.join("config"), app.0.join("linked")).unwrap();
    let mut config = app.config();
    config.project_plugin::<WhiskerFirebase>(|firebase| {
        firebase.ios_config = "linked/GoogleService-Info.plist".into();
    });
    let error = compose_auth_ios(&app, &config).unwrap_err();
    assert!(format!("{error:#}").contains("symlinks"), "{error:#}");
}

#[test]
fn auth_leaves_the_android_project_unchanged() {
    let app = App::new();
    assert_eq!(
        app.compose_with("android", &app.config(), &["auth"])
            .unwrap(),
        app.compose("android", &app.config()).unwrap()
    );
}
