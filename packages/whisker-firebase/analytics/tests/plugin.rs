use whisker_cng::{Config, Engine, ProjectEngine, android, ios};
use whisker_firebase_analytics::{WhiskerFirebaseAnalytics, WhiskerFirebaseAnalyticsConfig};
use whisker_plugin::project::*;

fn config() -> Config {
    let mut config = Config::default();
    config
        .name("AnalyticsTest")
        .bundle_id("rs.whisker.analyticstest");
    config
}

fn ios_project() -> ProjectIr {
    let dir = std::env::temp_dir().join(format!("whisker-analytics-test-{}", std::process::id()));
    base(
        ProjectEngine::with_ios_application(
            ios::inputs_from_with_engine(
                &Engine::new(),
                &config(),
                dir.join("whisker_modules"),
                dir,
                "fixture".into(),
            )
            .unwrap(),
        ),
        ProjectIr::Ios(Default::default()),
    )
}

fn android_project() -> ProjectIr {
    base(
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
    )
}

fn contribute(project: ProjectIr, config: &WhiskerFirebaseAnalyticsConfig) -> Option<ProjectIr> {
    let context = ProjectContext {
        project,
        app_crate_dir: None,
    };
    match WhiskerFirebaseAnalytics
        .contribute(&context, config)
        .unwrap()
    {
        ProjectUpdate::Replace { project, .. } => Some(*project),
        ProjectUpdate::Keep => None,
        ProjectUpdate::Merge { .. } => panic!("unexpected merge"),
    }
}

#[test]
fn ios_links_with_objc_once_and_keeps_existing_flags() {
    let Some(ProjectIr::Ios(ios)) = contribute(ios_project(), &Default::default()) else {
        panic!()
    };
    let target = &ios.apple.targets[&ios.apple.application];
    let AppleBuildSetting::String(flags) = &target.build_settings["OTHER_LDFLAGS"] else {
        panic!()
    };
    assert!(flags.starts_with("$(inherited)") && flags.contains("-framework WhiskerDriver"));
    assert!(flags.ends_with(" -ObjC"));
    let again = contribute(ProjectIr::Ios(ios.clone()), &Default::default()).unwrap();
    let ProjectIr::Ios(again) = again else {
        panic!()
    };
    assert_eq!(
        again.apple.targets[&again.apple.application].build_settings["OTHER_LDFLAGS"],
        target.build_settings["OTHER_LDFLAGS"]
    );
    assert!(
        !target
            .info_plist
            .contains_key("FIREBASE_ANALYTICS_COLLECTION_ENABLED")
    );
}

#[test]
fn collection_default_reaches_plist_and_manifest() {
    let config = WhiskerFirebaseAnalyticsConfig {
        collection_enabled: Some(false),
    };
    let Some(ProjectIr::Ios(ios)) = contribute(ios_project(), &config) else {
        panic!()
    };
    assert_eq!(
        ios.apple.targets[&ios.apple.application].info_plist["FIREBASE_ANALYTICS_COLLECTION_ENABLED"],
        PropertyListValue::Boolean(false)
    );
    assert!(contribute(android_project(), &Default::default()).is_none());
    let Some(ProjectIr::Android(android)) = contribute(android_project(), &config) else {
        panic!()
    };
    let AndroidModuleKind::Application(app) = &android.modules[&android.application].kind else {
        panic!()
    };
    let manifest = format!("{:?}", app.android.source_sets["main"].manifest);
    assert!(
        manifest.contains("firebase_analytics_collection_enabled")
            && manifest.contains("\"false\"")
    );
}

fn base(engine: ProjectEngine, empty: ProjectIr) -> ProjectIr {
    engine.compose(&config(), &empty).unwrap().project
}
