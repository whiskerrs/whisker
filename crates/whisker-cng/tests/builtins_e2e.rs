//! End-to-end check on the Phase 2/3 wiring: built-in plugin →
//! engine → `inputs_from` → template substitution → rendered file.
//!
//! Complements the per-plugin unit tests in
//! `crates/whisker-cng/src/plugins/*` (which only check IR-level
//! mutations) and the per-renderer tests in `src/ios.rs` /
//! `src/android.rs` (which only check default-config rendering).
//! Here we verify the whole pipeline writes the expected XML.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use whisker_cng::plugins::android_meta_data::AndroidMetaData;
use whisker_cng::plugins::android_permissions::AndroidPermissions;
use whisker_cng::plugins::info_plist_extra::InfoPlistExtra;
use whisker_config::Config;

fn unique_tempdir() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-cng-builtins-e2e-{pid}-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn base_ios_app() -> Config {
    let mut a = Config::default();
    a.name("HelloWorld")
        .bundle_id("rs.whisker.examples.helloWorld");
    a
}

fn base_android_app() -> Config {
    let mut a = Config::default();
    a.name("HelloWorld").android(|x| {
        x.application_id("rs.whisker.examples.helloworld");
    });
    a
}

// ============================================================================
// iOS: InfoPlistExtra
// ============================================================================

#[test]
fn ios_info_plist_extra_keys_reach_the_rendered_plist() {
    let mut app = base_ios_app();
    app.plugin::<InfoPlistExtra>(|c| {
        c.add("NSCameraUsageDescription", "Take photos.")
            .add("LSApplicationQueriesSchemes", "comgooglemaps");
    });

    let inputs = whisker_cng::ios::inputs_from(
        &app,
        PathBuf::from("/abs/platforms/ios"),
        PathBuf::from("/abs/gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "hello-world".into(),
    )
    .unwrap();

    let tmp = unique_tempdir();
    let out = tmp.join("gen/ios");
    whisker_cng::ios::sync(&out, &inputs).unwrap();

    let plist = std::fs::read_to_string(out.join("Info.plist")).unwrap();
    assert!(
        plist.contains("<key>NSCameraUsageDescription</key>"),
        "{plist}",
    );
    assert!(plist.contains("<string>Take photos.</string>"), "{plist}");
    assert!(
        plist.contains("<key>LSApplicationQueriesSchemes</key>"),
        "{plist}",
    );
    assert!(plist.contains("<string>comgooglemaps</string>"), "{plist}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ios_info_plist_extra_escapes_xml_in_values() {
    let mut app = base_ios_app();
    app.plugin::<InfoPlistExtra>(|c| {
        c.add("NSCameraUsageDescription", "Photos for <Foo & Bar>.");
    });

    let inputs = whisker_cng::ios::inputs_from(
        &app,
        PathBuf::from("/abs/platforms/ios"),
        PathBuf::from("/abs/gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "hello-world".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/ios");
    whisker_cng::ios::sync(&out, &inputs).unwrap();

    let plist = std::fs::read_to_string(out.join("Info.plist")).unwrap();
    assert!(
        plist.contains("Photos for &lt;Foo &amp; Bar&gt;."),
        "{plist}",
    );
    // Raw `<` from the user value must not leak in.
    assert!(
        !plist.contains("Photos for <Foo & Bar>"),
        "raw < should have been escaped: {plist}",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ios_no_plugin_declared_means_no_extra_keys_in_plist() {
    let app = base_ios_app(); // no app.plugin::<…>(…)

    let inputs = whisker_cng::ios::inputs_from(
        &app,
        PathBuf::from("/abs/platforms/ios"),
        PathBuf::from("/abs/gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "hello-world".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/ios");
    whisker_cng::ios::sync(&out, &inputs).unwrap();

    let plist = std::fs::read_to_string(out.join("Info.plist")).unwrap();
    // Sanity: the rendered plist still has the baseline keys.
    assert!(plist.contains("<key>CFBundleDisplayName</key>"));
    // And nothing from any plugin's NAME landed.
    assert!(!plist.contains("NSCameraUsageDescription"));
    let _ = std::fs::remove_dir_all(&tmp);
}

// ============================================================================
// Android: Permissions + MetaData
// ============================================================================

#[test]
fn android_extra_permissions_reach_the_rendered_manifest() {
    let mut app = base_android_app();
    app.plugin::<AndroidPermissions>(|c| {
        c.add("android.permission.CAMERA")
            .add("android.permission.RECORD_AUDIO");
    });

    let inputs = whisker_cng::android::inputs_from(
        &app,
        "hello_world".into(),
        PathBuf::from("../.."),
        "hello-world".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://whiskerrs.github.io/whisker/maven".into(),
        "https://whiskerrs.github.io/lynx/maven".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/android");
    whisker_cng::android::sync(&out, &inputs).unwrap();

    let manifest = std::fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap();
    assert!(
        manifest.contains("<uses-permission android:name=\"android.permission.INTERNET\""),
        "{manifest}",
    );
    assert!(
        manifest.contains("<uses-permission android:name=\"android.permission.CAMERA\""),
        "{manifest}",
    );
    assert!(
        manifest.contains("<uses-permission android:name=\"android.permission.RECORD_AUDIO\""),
        "{manifest}",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn android_duplicate_permissions_are_dedup_in_the_rendered_manifest() {
    let mut app = base_android_app();
    app.plugin::<AndroidPermissions>(|c| {
        c.add("android.permission.CAMERA")
            .add("android.permission.CAMERA");
    });

    let inputs = whisker_cng::android::inputs_from(
        &app,
        "hello_world".into(),
        PathBuf::from("../.."),
        "hello-world".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://whiskerrs.github.io/whisker/maven".into(),
        "https://whiskerrs.github.io/lynx/maven".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/android");
    whisker_cng::android::sync(&out, &inputs).unwrap();

    let manifest = std::fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap();
    let count = manifest
        .matches("<uses-permission android:name=\"android.permission.CAMERA\"")
        .count();
    assert_eq!(count, 1, "CAMERA appeared {count} times: {manifest}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn android_meta_data_reaches_the_rendered_manifest_inside_application() {
    let mut app = base_android_app();
    app.plugin::<AndroidMetaData>(|c| {
        c.add("com.google.android.geo.API_KEY", "AIza-XYZ");
    });

    let inputs = whisker_cng::android::inputs_from(
        &app,
        "hello_world".into(),
        PathBuf::from("../.."),
        "hello-world".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://whiskerrs.github.io/whisker/maven".into(),
        "https://whiskerrs.github.io/lynx/maven".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/android");
    whisker_cng::android::sync(&out, &inputs).unwrap();

    let manifest = std::fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap();
    assert!(manifest.contains("<application"), "{manifest}");
    let app_open = manifest.find("<application").unwrap();
    let app_close = manifest.find("</application>").unwrap();
    let inside_application = &manifest[app_open..app_close];
    assert!(
        inside_application.contains("com.google.android.geo.API_KEY"),
        "meta-data should be inside <application>: {manifest}",
    );
    assert!(inside_application.contains("AIza-XYZ"), "{manifest}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn android_no_plugin_declared_means_only_baseline_internet_permission() {
    let app = base_android_app();
    let inputs = whisker_cng::android::inputs_from(
        &app,
        "hello_world".into(),
        PathBuf::from("../.."),
        "hello-world".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://whiskerrs.github.io/whisker/maven".into(),
        "https://whiskerrs.github.io/lynx/maven".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/android");
    whisker_cng::android::sync(&out, &inputs).unwrap();

    let manifest = std::fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap();
    // The hardcoded INTERNET permission must still be there.
    assert!(manifest.contains("android.permission.INTERNET"));
    // Nothing else.
    assert!(!manifest.contains("android.permission.CAMERA"));
    assert!(!manifest.contains("<meta-data"));
    let _ = std::fs::remove_dir_all(&tmp);
}
