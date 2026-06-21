//! End-to-end: `RouterPlugin` through the real `whisker-cng` engine +
//! Android renderer. Mirrors what `whisker build`'s generation step does
//! (minus the device toolchain): registers the plugin like the subprocess
//! discovery path would, renders `gen/android`, and asserts the generated
//! `AndroidManifest.xml` opts into the predictive-back API.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use whisker_cng::Engine;
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

#[test]
fn generated_manifest_enables_on_back_invoked_callback() {
    // Engine with the built-ins + the router plugin registered, exactly
    // like `platforms.rs` does for a discovered subprocess plugin.
    let mut engine = Engine::with_builtins();
    engine.register(RouterPlugin);

    let inputs = whisker_cng::android::inputs_from_with_engine(
        &engine,
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

    let r#gen = unique_tempdir("android-gen").join("gen/android");
    whisker_cng::android::sync(&r#gen, &inputs).unwrap();

    let manifest = std::fs::read_to_string(r#gen.join("app/src/main/AndroidManifest.xml")).unwrap();
    assert!(
        manifest.contains("android:enableOnBackInvokedCallback=\"true\""),
        "router plugin must add the predictive-back opt-in to <application>:\n{manifest}"
    );
    // Exactly once, inside the `<application …>` open tag.
    let app_open = manifest.find("<application").unwrap();
    let app_close = app_open + manifest[app_open..].find('>').unwrap();
    let attr_pos = manifest.find("enableOnBackInvokedCallback").unwrap();
    assert!(
        attr_pos > app_open && attr_pos < app_close,
        "opt-in must be an <application> attribute"
    );

    let _ = std::fs::remove_dir_all(r#gen.parent().unwrap());
}
