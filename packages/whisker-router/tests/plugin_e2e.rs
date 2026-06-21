//! End-to-end: `RouterPlugin` through the real `whisker-cng` engine +
//! Android renderer, asserting the generated `AndroidManifest.xml` opts
//! into the predictive-back API.
//!
//! Two paths are covered, because they diverge:
//!
//! - **in-process** (`engine.register(RouterPlugin)`) — calls `apply`
//!   directly. Cheap, but does NOT exercise the JSON subprocess boundary.
//! - **subprocess** (`engine.register_subprocess(...)` pointing at the
//!   real built `whisker-router-plugin` binary) — exactly what
//!   `whisker run` does: spawn the bin, exchange `PluginRequest` /
//!   `PluginResponse` JSON, merge the response context back. This is the
//!   one that catches "the mutation didn't survive the wire".

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use whisker_cng::{Engine, SubprocessPlugin};
use whisker_config::Config;
use whisker_router::RouterPlugin;

fn unique_tempdir(label: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-router-e2e-{label}-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn app_config() -> Config {
    let mut a = Config::default();
    a.name("RouterSmoke").bundle_id("rs.whisker.routersmoke");
    a
}

/// Render `gen/android` with `engine` and return the generated manifest.
fn render_manifest(engine: &Engine, label: &str) -> String {
    let inputs = whisker_cng::android::inputs_from_with_engine(
        engine,
        &app_config(),
        "router_smoke".into(),
        PathBuf::from("../.."),
        "router-smoke".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://example.invalid/maven".into(),
        "https://example.invalid/lynx".into(),
    )
    .unwrap();
    let r#gen = unique_tempdir(label).join("gen/android");
    whisker_cng::android::sync(&r#gen, &inputs).unwrap();
    let manifest = std::fs::read_to_string(r#gen.join("app/src/main/AndroidManifest.xml")).unwrap();
    let _ = std::fs::remove_dir_all(r#gen.parent().unwrap());
    manifest
}

/// Assert the opt-in appears exactly once, inside the `<application …>`
/// open tag.
fn assert_opt_in(manifest: &str) {
    assert!(
        manifest.contains("android:enableOnBackInvokedCallback=\"true\""),
        "manifest must opt into predictive back:\n{manifest}"
    );
    assert_eq!(
        manifest.matches("enableOnBackInvokedCallback").count(),
        1,
        "rendered exactly once"
    );
    let app_open = manifest.find("<application").unwrap();
    let app_close = app_open + manifest[app_open..].find('>').unwrap();
    let attr_pos = manifest.find("enableOnBackInvokedCallback").unwrap();
    assert!(
        attr_pos > app_open && attr_pos < app_close,
        "opt-in must be an <application> attribute"
    );
}

#[test]
fn in_process_plugin_enables_on_back_invoked_callback() {
    let mut engine = Engine::with_builtins();
    engine.register(RouterPlugin);
    assert_opt_in(&render_manifest(&engine, "in-process"));
}

/// The path `whisker run` actually takes: the plugin runs as a spawned
/// subprocess and its mutation has to survive the `PluginResponse` JSON
/// round trip. Cargo builds the bin and hands us its path via
/// `CARGO_BIN_EXE_<bin-name>`.
#[test]
fn subprocess_plugin_enables_on_back_invoked_callback() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_whisker-router-plugin"));
    let mut engine = Engine::with_builtins();
    engine.register_subprocess(SubprocessPlugin::new("whisker-router", bin));
    assert_opt_in(&render_manifest(&engine, "subprocess"));
}
