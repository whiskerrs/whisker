//! End-to-end: `WhiskerAsset` through the real `whisker-cng` engine +
//! renderers. Builds a fake app crate with an `assets/` tree, runs the
//! plugin pipeline in-process (registering `WhiskerAsset` like the
//! subprocess discovery path would), then renders `gen/ios` +
//! `gen/android` and asserts the assets land at the paths Phase 1's
//! resolver expects.
//!
//! This mirrors what `whisker build`'s generation step does, minus the
//! device toolchain: `platforms.rs` discovers the plugin, stamps the
//! app crate dir onto the engine, and feeds it to
//! `inputs_from_with_engine`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use whisker_asset::WhiskerAsset;
use whisker_cng::{EnabledTargets, Engine};
use whisker_config::Config;

fn unique_tempdir(label: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-asset-e2e-{label}-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn app_config() -> Config {
    let mut a = Config::default();
    a.name("AssetDemo").bundle_id("rs.whisker.assetdemo");
    a.plugin::<WhiskerAsset>(|c| {
        c.dir("assets");
    });
    a
}

/// Build an engine that knows the plugin + the app crate dir, exactly
/// like `platforms.rs::build_engine_with_discovered_plugins` does for a
/// discovered subprocess plugin.
fn engine_for(crate_dir: &Path) -> Engine {
    let mut engine = Engine::with_builtins().with_app_crate_dir(crate_dir);
    engine.register(WhiskerAsset);
    engine
}

#[test]
fn ios_generation_lands_assets_and_folder_reference() {
    let crate_dir = unique_tempdir("ios-crate");
    write(
        &crate_dir,
        "assets/images/logo.png",
        &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a],
    );
    write(&crate_dir, "assets/data/config.json", b"{\"k\":1}");

    let engine = engine_for(&crate_dir);
    let inputs = whisker_cng::ios::inputs_from_with_engine(
        &engine,
        &app_config(),
        PathBuf::from("/abs/platforms/ios"),
        crate_dir.join("gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "asset-demo".into(),
    )
    .unwrap();

    let r#gen = unique_tempdir("ios-gen").join("gen/ios");
    whisker_cng::ios::sync(&r#gen, &inputs).unwrap();

    // Assets written under whisker_assets/<rel>, bytes intact.
    let logo = r#gen.join("whisker_assets/images/logo.png");
    assert!(logo.exists(), "logo not written to {}", logo.display());
    assert_eq!(
        std::fs::read(&logo).unwrap(),
        vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a],
    );
    assert!(r#gen.join("whisker_assets/data/config.json").exists());

    // pbxproj carries the folder reference so the bundle keeps subdirs.
    let pbxproj =
        std::fs::read_to_string(r#gen.join(format!("{}.xcodeproj/project.pbxproj", inputs.scheme)))
            .unwrap();
    assert!(
        pbxproj.contains("lastKnownFileType = folder;")
            && pbxproj.contains("path = \"whisker_assets\""),
        "folder reference missing from pbxproj",
    );
    assert!(pbxproj.contains("whisker_assets in Resources"));
    assert!(!pbxproj.contains("{{"), "unsubstituted placeholder");

    let _ = std::fs::remove_dir_all(&crate_dir);
    let _ = std::fs::remove_dir_all(r#gen.parent().unwrap());
}

#[test]
fn android_generation_lands_assets_under_whisker_namespace() {
    let crate_dir = unique_tempdir("android-crate");
    write(&crate_dir, "assets/images/logo.png", &[0x00, 0xff, 0x10]);
    write(&crate_dir, "assets/sound.bin", &[1, 2, 3, 4]);

    let engine = engine_for(&crate_dir);
    let inputs = whisker_cng::android::inputs_from_with_engine(
        &engine,
        &app_config(),
        "asset_demo".into(),
        PathBuf::from("../.."),
        "asset-demo".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://example.invalid/maven".into(),
        "https://example.invalid/lynx".into(),
    )
    .unwrap();

    let r#gen = unique_tempdir("android-gen").join("gen/android");
    whisker_cng::android::sync(&r#gen, &inputs).unwrap();

    // AGP source set: app/src/main/assets/whisker/<rel>
    let logo = r#gen.join("app/src/main/assets/whisker/images/logo.png");
    assert!(logo.exists(), "logo not written to {}", logo.display());
    assert_eq!(std::fs::read(&logo).unwrap(), vec![0x00, 0xff, 0x10]);
    assert!(r#gen.join("app/src/main/assets/whisker/sound.bin").exists());

    let _ = std::fs::remove_dir_all(&crate_dir);
    let _ = std::fs::remove_dir_all(r#gen.parent().unwrap());
}

#[test]
fn engine_compose_records_collision_error_message() {
    // The validate/collision path through the real engine compose.
    let crate_dir = unique_tempdir("collide-crate");
    write(&crate_dir, "assets/logo.png", b"a");
    write(&crate_dir, "branding/logo.png", b"b");

    let mut cfg = Config::default();
    cfg.name("X").bundle_id("rs.whisker.x");
    cfg.plugin::<WhiskerAsset>(|c| {
        c.dir("assets").file("branding/logo.png");
    });

    let engine = engine_for(&crate_dir);
    let err = engine.compose(&cfg, EnabledTargets::both()).unwrap_err();
    assert!(format!("{err:#}").contains("collide"), "{err:#}");
    let _ = std::fs::remove_dir_all(&crate_dir);
}
