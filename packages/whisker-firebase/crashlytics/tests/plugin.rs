use whisker_cng::{Config, Engine, ProjectEngine, android, ios};
use whisker_firebase_crashlytics::{WhiskerFirebaseCrashlytics, WhiskerFirebaseCrashlyticsConfig};
use whisker_plugin::project::*;

fn config() -> Config {
    let mut config = Config::default();
    config
        .name("CrashlyticsTest")
        .bundle_id("rs.whisker.crashlyticstest");
    config
}

fn contribute(project: ProjectIr, config: &WhiskerFirebaseCrashlyticsConfig) -> ProjectIr {
    let context = ProjectContext {
        project,
        app_crate_dir: None,
    };
    match WhiskerFirebaseCrashlytics
        .contribute(&context, config)
        .unwrap()
    {
        ProjectUpdate::Replace { project, .. } => *project,
        _ => panic!("expected a replacement"),
    }
}

#[test]
fn android_app_applies_the_gradle_plugin_once() {
    let project = base(
        ProjectEngine::with_android_application(
            android::inputs_from_with_engine(
                &Engine::new(),
                &config(),
                "fixture".into(),
                "../..".into(),
                "fixture".into(),
                "0.1.21".into(),
                "0.1.0".into(),
                "https://example.invalid/maven".into(),
            )
            .unwrap(),
        ),
        ProjectIr::Android(Box::default()),
    );
    let config = WhiskerFirebaseCrashlyticsConfig {
        collection_enabled: Some(false),
        ..Default::default()
    };
    let once = contribute(project, &config);
    let ProjectIr::Android(android) = contribute(once, &config) else {
        panic!()
    };
    assert_eq!(
        android.settings.plugin_management.plugins["com.google.firebase.crashlytics"],
        "3.0.8"
    );
    let module = &android.modules[&android.application];
    let applied = module
        .build
        .plugins
        .iter()
        .filter(|p| p.id == "com.google.firebase.crashlytics" && p.apply);
    assert_eq!(applied.count(), 1);
    let AndroidModuleKind::Application(app) = &module.kind else {
        panic!()
    };
    assert!(
        format!("{:?}", app.android.source_sets["main"].manifest)
            .contains("firebase_crashlytics_collection_enabled")
    );
}

#[test]
fn ios_app_uploads_symbols_only_when_enabled() {
    let dir = std::env::temp_dir().join(format!("whisker-crashlytics-test-{}", std::process::id()));
    let project = || {
        base(
            ProjectEngine::with_ios_application(
                ios::inputs_from_with_engine(
                    &Engine::new(),
                    &config(),
                    dir.join("whisker_modules"),
                    dir.clone(),
                    "fixture".into(),
                )
                .unwrap(),
            ),
            ProjectIr::Ios(Default::default()),
        )
    };
    let ProjectIr::Ios(ios) = contribute(project(), &Default::default()) else {
        panic!()
    };
    let target = &ios.apple.targets[&ios.apple.application];
    let script = target
        .scripts
        .iter()
        .find(|s| s.name == "Upload Crashlytics symbols")
        .unwrap();
    assert!(script.script.contains("\"$CONFIGURATION\" = \"Release\""));
    assert!(script.script.contains("firebase-ios-sdk/Crashlytics/run"));
    let config = WhiskerFirebaseCrashlyticsConfig {
        upload_symbols: false,
        ..Default::default()
    };
    let ProjectIr::Ios(ios) = contribute(project(), &config) else {
        panic!()
    };
    assert!(
        ios.apple.targets[&ios.apple.application]
            .scripts
            .iter()
            .all(|s| s.name != "Upload Crashlytics symbols")
    );
}

fn base(engine: ProjectEngine, empty: ProjectIr) -> ProjectIr {
    engine.compose(&config(), &empty).unwrap().project
}
