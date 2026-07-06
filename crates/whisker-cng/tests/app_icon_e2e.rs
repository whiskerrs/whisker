//! End-to-end check for the `whisker-app-icon` built-in: user Config
//! (declared via the `whisker_config::AppIcon` marker, exactly as
//! `whisker.rs` spells it) → engine → `inputs_from_with_engine` →
//! rendered `gen/{ios,android}` trees.
//!
//! Complements the IR-level unit tests in
//! `src/plugins/app_icon.rs` — here we verify the generated files
//! actually land on disk and the pbxproj/manifest reference them.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use whisker_cng::Engine;
use whisker_config::{AppIcon, Config};

fn unique_tempdir() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-cng-app-icon-e2e-{pid}-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Write a square PNG fixture the plugin accepts (1024×1024).
fn write_icon(crate_root: &Path) {
    let img = image::RgbaImage::from_pixel(1024, 1024, image::Rgba([30, 120, 240, 255]));
    std::fs::create_dir_all(crate_root.join("assets")).unwrap();
    image::DynamicImage::ImageRgba8(img)
        .save_with_format(crate_root.join("assets/icon.png"), image::ImageFormat::Png)
        .unwrap();
}

fn app_with_icon() -> Config {
    let mut a = Config::default();
    a.name("IconApp").bundle_id("rs.whisker.iconapp");
    a.android(|x| {
        x.application_id("rs.whisker.iconapp");
    });
    a.plugin::<AppIcon>(|c| {
        c.source("assets/icon.png");
    });
    a
}

fn engine_for(crate_root: &Path) -> Engine {
    Engine::with_builtins().with_app_crate_dir(crate_root)
}

#[test]
fn ios_gen_tree_gets_asset_catalog_and_pbxproj_reference() {
    let tmp = unique_tempdir();
    let crate_root = tmp.join("app");
    std::fs::create_dir_all(&crate_root).unwrap();
    write_icon(&crate_root);

    let inputs = whisker_cng::ios::inputs_from_with_engine(
        &engine_for(&crate_root),
        &app_with_icon(),
        PathBuf::from("/abs/platforms/ios"),
        PathBuf::from("/abs/gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "icon-app".into(),
    )
    .unwrap();

    let out = tmp.join("gen/ios");
    whisker_cng::ios::sync(&out, &inputs).unwrap();

    // Catalog files landed.
    let icon_png = out.join("Assets.xcassets/AppIcon.appiconset/AppIcon.png");
    assert!(icon_png.is_file(), "missing {}", icon_png.display());
    let decoded = image::open(&icon_png).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (1024, 1024));
    let contents =
        std::fs::read_to_string(out.join("Assets.xcassets/AppIcon.appiconset/Contents.json"))
            .unwrap();
    assert!(contents.contains("\"size\" : \"1024x1024\""), "{contents}");
    assert!(out.join("Assets.xcassets/Contents.json").is_file());

    // pbxproj references the catalog in the Resources phase with the
    // asset-catalog file type (so actool compiles it), and the AppIcon
    // build setting the template ships still names it.
    // The rendered project dir is `<scheme>.xcodeproj`; the scheme
    // defaults to the app name when `ios.scheme` is unset.
    let pbxproj = std::fs::read_to_string(out.join("IconApp.xcodeproj/project.pbxproj")).unwrap();
    assert!(
        pbxproj.contains("Assets.xcassets in Resources"),
        "{pbxproj}"
    );
    assert!(pbxproj.contains("folder.assetcatalog"), "{pbxproj}");
    assert!(pbxproj.contains("ASSETCATALOG_COMPILER_APPICON_NAME = AppIcon"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn android_gen_tree_gets_mipmaps_and_manifest_icon() {
    let tmp = unique_tempdir();
    let crate_root = tmp.join("app");
    std::fs::create_dir_all(&crate_root).unwrap();
    write_icon(&crate_root);

    let inputs = whisker_cng::android::inputs_from_with_engine(
        &engine_for(&crate_root),
        &app_with_icon(),
        "icon_app".into(),
        PathBuf::from("../.."),
        "icon-app".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://whiskerrs.github.io/whisker/maven".into(),
        "https://whiskerrs.github.io/lynx/maven".into(),
    )
    .unwrap();

    let out = tmp.join("gen/android");
    whisker_cng::android::sync(&out, &inputs).unwrap();

    for (qualifier, px) in [
        ("mdpi", 48u32),
        ("hdpi", 72),
        ("xhdpi", 96),
        ("xxhdpi", 144),
        ("xxxhdpi", 192),
    ] {
        let p = out.join(format!(
            "app/src/main/res/mipmap-{qualifier}/ic_launcher.png"
        ));
        assert!(p.is_file(), "missing {}", p.display());
        let decoded = image::open(&p).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (px, px), "{qualifier}");
    }

    // Adaptive icon: definition XML + 108dp foreground layers +
    // default white background color resource.
    let xml =
        std::fs::read_to_string(out.join("app/src/main/res/mipmap-anydpi-v26/ic_launcher.xml"))
            .unwrap();
    assert!(xml.contains("@mipmap/ic_launcher_foreground"), "{xml}");
    assert!(xml.contains("@color/ic_launcher_background"), "{xml}");
    for (qualifier, px) in [
        ("mdpi", 108u32),
        ("hdpi", 162),
        ("xhdpi", 216),
        ("xxhdpi", 324),
        ("xxxhdpi", 432),
    ] {
        let p = out.join(format!(
            "app/src/main/res/mipmap-{qualifier}/ic_launcher_foreground.png"
        ));
        assert!(p.is_file(), "missing {}", p.display());
        let decoded = image::open(&p).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (px, px), "{qualifier}");
    }
    let colors =
        std::fs::read_to_string(out.join("app/src/main/res/values/ic_launcher_background.xml"))
            .unwrap();
    assert!(colors.contains("#FFFFFF"), "{colors}");

    let manifest = std::fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap();
    assert!(
        manifest.contains(r#"android:icon="@mipmap/ic_launcher""#),
        "{manifest}",
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ios_icon_bundle_reaches_gen_tree_and_pbxproj() {
    let tmp = unique_tempdir();
    let crate_root = tmp.join("app");
    std::fs::create_dir_all(&crate_root).unwrap();
    write_icon(&crate_root);
    // Minimal Icon Composer bundle fixture.
    let bundle = crate_root.join("assets/Fancy.icon");
    std::fs::create_dir_all(bundle.join("Assets")).unwrap();
    std::fs::write(
        bundle.join("icon.json"),
        r#"{"fill":{"solid":"srgb:1,1,1,1"},"groups":[{"layers":[{"image-name":"glyph.png","name":"glyph"}]}],"supported-platforms":{"squares":"shared"}}"#,
    )
    .unwrap();
    std::fs::copy(
        crate_root.join("assets/icon.png"),
        bundle.join("Assets/glyph.png"),
    )
    .unwrap();

    let mut app = app_with_icon();
    app.plugin::<AppIcon>(|c| {
        c.source("assets/icon.png").ios_icon("assets/Fancy.icon");
    });

    let inputs = whisker_cng::ios::inputs_from_with_engine(
        &engine_for(&crate_root),
        &app,
        PathBuf::from("/abs/platforms/ios"),
        PathBuf::from("/abs/gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "icon-app".into(),
    )
    .unwrap();
    let out = tmp.join("gen/ios");
    whisker_cng::ios::sync(&out, &inputs).unwrap();

    // Bundle staged under the fixed AppIcon.icon name; no xcassets.
    assert!(out.join("AppIcon.icon/icon.json").is_file());
    assert!(out.join("AppIcon.icon/Assets/glyph.png").is_file());
    assert!(!out.join("Assets.xcassets").exists());

    // pbxproj: Icon Composer file type + Resources membership; the
    // template's AppIcon build setting resolves to the staged name.
    let pbxproj = std::fs::read_to_string(out.join("IconApp.xcodeproj/project.pbxproj")).unwrap();
    assert!(pbxproj.contains("AppIcon.icon in Resources"), "{pbxproj}");
    assert!(pbxproj.contains("folder.iconcomposer.icon"), "{pbxproj}");
    assert!(pbxproj.contains("ASSETCATALOG_COMPILER_APPICON_NAME = AppIcon"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn no_declaration_leaves_gen_trees_icon_free() {
    let tmp = unique_tempdir();
    let mut app = Config::default();
    app.name("PlainApp").bundle_id("rs.whisker.plain");

    let inputs = whisker_cng::ios::inputs_from(
        &app,
        PathBuf::from("/abs/platforms/ios"),
        PathBuf::from("/abs/gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "plain-app".into(),
    )
    .unwrap();
    let out = tmp.join("gen/ios");
    whisker_cng::ios::sync(&out, &inputs).unwrap();
    assert!(!out.join("Assets.xcassets").exists());

    let _ = std::fs::remove_dir_all(&tmp);
}
