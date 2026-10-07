use whisker_cng::{Config, Engine, ProjectEngine, android, ios};
use whisker_firebase_messaging::WhiskerFirebaseMessaging;
use whisker_plugin::project::*;

fn config() -> Config {
    let mut config = Config::default();
    config
        .name("MessagingTest")
        .bundle_id("rs.whisker.messagingtest");
    config
}

fn compose_ios(config: &Config) -> anyhow::Result<IosProjectIr> {
    let dir = std::env::temp_dir().join(format!("whisker-messaging-test-{}", std::process::id()));
    let mut engine = ProjectEngine::with_ios_application(ios::inputs_from_with_engine(
        &Engine::new(),
        &self::config(),
        dir.join("whisker_modules"),
        dir,
        "fixture".into(),
    )?);
    engine.register(WhiskerFirebaseMessaging);
    match engine
        .compose(config, &ProjectIr::Ios(Default::default()))?
        .project
    {
        ProjectIr::Ios(ios) => Ok(ios),
        _ => unreachable!(),
    }
}

#[test]
fn ios_app_gets_push_entitlement_and_background_mode() {
    let mut ios = compose_ios(&config()).unwrap();
    // Keep modes contributed by other plugins, such as whisker-audio.
    let app = ios.apple.application.clone();
    ios.apple.targets.get_mut(&app).unwrap().info_plist.insert(
        "UIBackgroundModes".into(),
        PropertyListValue::Array(vec![PropertyListValue::String("audio".into())]),
    );
    let context = ProjectContext {
        project: ProjectIr::Ios(ios),
        app_crate_dir: None,
    };
    let ProjectUpdate::Replace { project, .. } = WhiskerFirebaseMessaging
        .contribute(&context, &Default::default())
        .unwrap()
    else {
        panic!("expected a replacement")
    };
    let ProjectIr::Ios(ios) = *project else {
        panic!()
    };
    let target = &ios.apple.targets[&app];
    assert_eq!(
        target.entitlements["aps-environment"],
        PropertyListValue::String("development".into())
    );
    assert_eq!(
        target.info_plist["UIBackgroundModes"],
        PropertyListValue::Array(vec![
            PropertyListValue::String("audio".into()),
            PropertyListValue::String("remote-notification".into()),
        ])
    );
}

#[test]
fn configuration_selects_the_environment_and_rejects_typos() {
    let mut config = config();
    config.project_plugin::<WhiskerFirebaseMessaging>(|messaging| {
        messaging.aps_environment = "production".into();
        messaging.background_remote_notifications = false;
    });
    let ios = compose_ios(&config).unwrap();
    let target = &ios.apple.targets[&ios.apple.application];
    assert_eq!(
        target.entitlements["aps-environment"],
        PropertyListValue::String("production".into())
    );
    assert!(!target.info_plist.contains_key("UIBackgroundModes"));

    let mut config = self::config();
    config.project_plugin::<WhiskerFirebaseMessaging>(|messaging| {
        messaging.aps_environment = "prod".into();
    });
    assert!(compose_ios(&config).is_err());
}

#[test]
fn android_project_is_unchanged() {
    let config = config();
    let inputs = android::inputs_from_with_engine(
        &Engine::new(),
        &config,
        "fixture".into(),
        "../..".into(),
        "fixture".into(),
        "0.1.21".into(),
        "0.1.0".into(),
        "https://example.invalid/maven".into(),
    )
    .unwrap();
    let plain = ProjectEngine::with_android_application(inputs.clone());
    let mut engine = ProjectEngine::with_android_application(inputs);
    engine.register(WhiskerFirebaseMessaging);
    let empty = ProjectIr::Android(Box::default());
    assert_eq!(
        engine.compose(&config, &empty).unwrap().project,
        plain.compose(&config, &empty).unwrap().project
    );
}
