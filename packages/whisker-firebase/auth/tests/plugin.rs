use std::path::{Path, PathBuf};
use whisker_cng::{Config, Engine, ProjectEngine, android, ios};
use whisker_firebase_auth::WhiskerFirebaseAuth;
use whisker_plugin::project::*;

const SCHEME: &str = "app-1-1234567890-ios-0123456789abcdef0123456789abcdef";

struct App(PathBuf);

impl App {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "whisker-firebase-auth-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../example/GoogleService-Info.plist"),
            path.join("GoogleService-Info.plist"),
        )
        .unwrap();
        Self(path)
    }

    fn compose_ios(&self, config: &Config) -> anyhow::Result<IosProjectIr> {
        let mut engine = ProjectEngine::with_ios_application(ios::inputs_from_with_engine(
            &Engine::new(),
            &self::config(),
            self.0.join("whisker_modules"),
            self.0.clone(),
            "fixture".into(),
        )?)
        .with_app_crate_dir(&self.0);
        engine.register(WhiskerFirebaseAuth);
        match engine
            .compose(config, &ProjectIr::Ios(Default::default()))?
            .project
        {
            ProjectIr::Ios(ios) => Ok(ios),
            _ => unreachable!(),
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn config() -> Config {
    let mut config = Config::default();
    config
        .name("AuthTest")
        .bundle_id("rs.whisker.firebaseexample");
    config
}

fn schemes(ios: &IosProjectIr) -> Vec<PropertyListValue> {
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

#[test]
fn ios_app_registers_the_phone_sign_in_url_scheme_once() {
    let app = App::new();
    let mut ios = app.compose_ios(&config()).unwrap();
    let scheme = PropertyListValue::String(SCHEME.into());
    assert_eq!(schemes(&ios), vec![scheme.clone()]);

    // Keep schemes contributed by other plugins and do not add ours twice.
    let target = ios.apple.application.clone();
    let PropertyListValue::Array(types) = ios
        .apple
        .targets
        .get_mut(&target)
        .unwrap()
        .info_plist
        .get_mut("CFBundleURLTypes")
        .unwrap()
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
    let ProjectUpdate::Replace { project, .. } = WhiskerFirebaseAuth
        .contribute(&context, &Default::default())
        .unwrap()
    else {
        panic!("expected a replacement")
    };
    let ProjectIr::Ios(ios) = *project else {
        panic!()
    };
    assert_eq!(
        schemes(&ios),
        vec![PropertyListValue::String("other".into()), scheme]
    );
}

#[test]
fn configuration_turns_the_scheme_off_and_reports_a_missing_plist() {
    let app = App::new();
    let mut config = config();
    config.project_plugin::<WhiskerFirebaseAuth>(|auth| {
        auth.phone_url_scheme = false;
    });
    assert!(schemes(&app.compose_ios(&config).unwrap()).is_empty());

    std::fs::remove_file(app.0.join("GoogleService-Info.plist")).unwrap();
    assert!(app.compose_ios(&self::config()).is_err());
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
    engine.register(WhiskerFirebaseAuth);
    let empty = ProjectIr::Android(Box::default());
    assert_eq!(
        engine.compose(&config, &empty).unwrap().project,
        plain.compose(&config, &empty).unwrap().project
    );
}
