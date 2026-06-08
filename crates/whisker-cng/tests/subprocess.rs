//! End-to-end check on `Engine::register_subprocess`.
//!
//! Spawns the `whisker-cng-fixture-echo-plugin` binary (declared as
//! `[[bin]]` in this crate's Cargo.toml) and verifies the
//! request → spawn → response → context-merge round-trip works.
//! Unit tests in `compose.rs` cover the pure pieces; this test is
//! the only place we actually fork() a real process.

use std::path::PathBuf;
use whisker_app_config::AppConfig;
use whisker_cng::{EnabledTargets, Engine, SubprocessPlugin};

fn fixture_binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_whisker-cng-fixture-echo-plugin"))
}

#[test]
fn subprocess_plugin_pushes_permission_through_a_real_spawn() {
    let plugin = SubprocessPlugin::new("fixture-echo-plugin", fixture_binary_path());

    let mut engine = Engine::new();
    engine.register_subprocess(plugin);

    // User-side config: simulate what `app.plugin::<EchoConfig>(|c| ...)`
    // would have stored into AppConfig.plugins.
    let mut app = AppConfig::default();
    app.name("Demo");
    app.plugins.insert(
        "fixture-echo-plugin".into(),
        serde_json::json!({"permission": "android.permission.CAMERA"}),
    );

    let ctx = engine
        .compose(&app, EnabledTargets::android_only())
        .expect("subprocess plugin should run cleanly");

    let android = ctx.android.expect("android IR should be populated");
    assert_eq!(
        android.manifest.permissions,
        vec!["android.permission.CAMERA".to_string()],
    );
    assert_eq!(ctx.journal.records.len(), 1);
    let r = &ctx.journal.records[0];
    assert_eq!(r.plugin, "fixture-echo-plugin");
    assert_eq!(r.path, "manifest.permissions");
}

#[test]
fn subprocess_plugin_runs_with_default_config_when_user_omitted_declaration() {
    // No app.plugins entry → engine sends a Null config; the
    // fixture's EchoConfig::default() has an empty permission, so
    // nothing should be added to the Android manifest.
    let plugin = SubprocessPlugin::new("fixture-echo-plugin", fixture_binary_path());
    let mut engine = Engine::new();
    engine.register_subprocess(plugin);

    let mut app = AppConfig::default();
    app.name("Demo");

    let ctx = engine
        .compose(&app, EnabledTargets::android_only())
        .expect("subprocess plugin should still run when undeclared");

    assert!(ctx.android.unwrap().manifest.permissions.is_empty());
    assert!(ctx.journal.records.is_empty());
}

#[test]
fn subprocess_plugin_with_a_bad_binary_path_surfaces_spawn_error() {
    let plugin = SubprocessPlugin::new(
        "fixture-echo-plugin",
        // Path that definitely does not exist anywhere on PATH.
        PathBuf::from("/does/not/exist/whisker-fixture-no-such-binary"),
    );
    let mut engine = Engine::new();
    engine.register_subprocess(plugin);

    let mut app = AppConfig::default();
    app.name("Demo");

    let err = engine
        .compose(&app, EnabledTargets::android_only())
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("fixture-echo-plugin"), "{msg}");
    assert!(msg.contains("spawn"), "{msg}");
}
