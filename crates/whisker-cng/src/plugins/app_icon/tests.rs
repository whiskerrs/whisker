use super::*;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use whisker_plugin::{AndroidProjectIr, IosProjectIr};

fn unique_tempdir(label: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-app-icon-test-{label}-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Write a solid-color square PNG fixture of the given edge.
fn write_png(root: &Path, rel: &str, px: u32, rgba: [u8; 4]) {
    let img = image::RgbaImage::from_pixel(px, px, image::Rgba(rgba));
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    DynamicImage::ImageRgba8(img)
        .save_with_format(&p, image::ImageFormat::Png)
        .unwrap();
}

fn ctx_both(crate_root: &Path) -> GenerateContext {
    GenerateContext {
        ios: Some(IosProjectIr::default()),
        android: Some(AndroidProjectIr::default()),
        app_crate_dir: Some(crate_root.to_path_buf()),
        ..Default::default()
    }
}

fn cfg_with(source: &str) -> AppIconConfig {
    let mut c = AppIconConfig::default();
    c.source(source);
    c
}

#[test]
fn default_config_contributes_nothing() {
    let root = unique_tempdir("noop");
    let mut ctx = ctx_both(&root);
    AppIcon.apply(&mut ctx, &AppIconConfig::default()).unwrap();
    assert!(ctx.ios.unwrap().extra_files.is_empty());
    assert!(ctx.android.unwrap().extra_files.is_empty());
    assert!(ctx.journal.records.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn validate_rejects_parent_traversal_and_non_png() {
    let err = AppIcon.validate(&cfg_with("../icon.png")).unwrap_err();
    assert!(err.to_string().contains(".."), "{err}");
    let err = AppIcon.validate(&cfg_with("assets/icon.jpg")).unwrap_err();
    assert!(err.to_string().contains("not a .png"), "{err}");
    AppIcon.validate(&cfg_with("assets/icon.png")).unwrap();
    AppIcon.validate(&AppIconConfig::default()).unwrap();
}

#[test]
fn apply_errors_on_missing_source() {
    let root = unique_tempdir("missing");
    let mut ctx = ctx_both(&root);
    let err = AppIcon
        .apply(&mut ctx, &cfg_with("assets/icon.png"))
        .unwrap_err();
    assert!(err.to_string().contains("could not be read"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_errors_on_non_square_source() {
    let root = unique_tempdir("nonsquare");
    let img = image::RgbaImage::from_pixel(1024, 512, image::Rgba([1, 2, 3, 255]));
    std::fs::create_dir_all(root.join("assets")).unwrap();
    DynamicImage::ImageRgba8(img)
        .save_with_format(root.join("assets/icon.png"), image::ImageFormat::Png)
        .unwrap();
    let mut ctx = ctx_both(&root);
    let err = AppIcon
        .apply(&mut ctx, &cfg_with("assets/icon.png"))
        .unwrap_err();
    assert!(err.to_string().contains("must be square"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_errors_on_undersized_source() {
    let root = unique_tempdir("small");
    write_png(&root, "assets/icon.png", 512, [1, 2, 3, 255]);
    let mut ctx = ctx_both(&root);
    let err = AppIcon
        .apply(&mut ctx, &cfg_with("assets/icon.png"))
        .unwrap_err();
    assert!(err.to_string().contains("1024×1024"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_populates_ios_catalog_and_resource_op() {
    let root = unique_tempdir("ios");
    write_png(&root, "assets/icon.png", 1024, [10, 20, 30, 255]);
    let mut ctx = ctx_both(&root);
    AppIcon
        .apply(&mut ctx, &cfg_with("assets/icon.png"))
        .unwrap();

    let ios = ctx.ios.as_ref().unwrap();
    assert!(
        ios.extra_files
            .contains_key(Path::new("Assets.xcassets/Contents.json"))
    );
    let contents = ios.extra_files[Path::new("Assets.xcassets/AppIcon.appiconset/Contents.json")]
        .to_bytes()
        .unwrap();
    let contents = String::from_utf8(contents).unwrap();
    assert!(contents.contains("\"size\" : \"1024x1024\""), "{contents}");
    assert!(contents.contains("AppIcon.png"), "{contents}");

    let png = ios.extra_files[Path::new("Assets.xcassets/AppIcon.appiconset/AppIcon.png")]
        .to_bytes()
        .unwrap();
    let decoded = image::load_from_memory(&png).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (1024, 1024));
    assert!(
        !decoded.color().has_alpha(),
        "iOS icon must be emitted without an alpha channel"
    );

    assert!(ios.pbxproj_ops.iter().any(|op| {
        matches!(op, PbxprojOp::AddResource { path } if path == Path::new("Assets.xcassets"))
    }));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_flattens_transparency_onto_white_for_ios() {
    let root = unique_tempdir("alpha");
    write_png(&root, "assets/icon.png", 1024, [200, 10, 10, 0]);
    let mut ctx = ctx_both(&root);
    AppIcon
        .apply(&mut ctx, &cfg_with("assets/icon.png"))
        .unwrap();
    let png = ctx.ios.as_ref().unwrap().extra_files
        [Path::new("Assets.xcassets/AppIcon.appiconset/AppIcon.png")]
    .to_bytes()
    .unwrap();
    let decoded = image::load_from_memory(&png).unwrap().to_rgb8();
    assert_eq!(decoded.get_pixel(512, 512).0, [255, 255, 255]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_populates_android_mipmaps_and_icon_attribute() {
    let root = unique_tempdir("android");
    write_png(&root, "assets/icon.png", 2048, [10, 20, 30, 255]);
    let mut ctx = ctx_both(&root);
    AppIcon
        .apply(&mut ctx, &cfg_with("assets/icon.png"))
        .unwrap();

    let android = ctx.android.as_ref().unwrap();
    for (qualifier, px) in ANDROID_DENSITIES {
        let path = PathBuf::from(format!(
            "app/src/main/res/mipmap-{qualifier}/ic_launcher.png"
        ));
        let bytes = android
            .extra_files
            .get(&path)
            .unwrap_or_else(|| panic!("missing {}", path.display()))
            .to_bytes()
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (*px, *px));
    }

    let attrs = &android.manifest.application_attributes;
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0].name, "android:icon");
    assert_eq!(attrs[0].value, "@mipmap/ic_launcher");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_defaults_generate_adaptive_icon_from_source() {
    let root = unique_tempdir("adaptive-default");
    write_png(&root, "assets/icon.png", 1024, [10, 20, 30, 255]);
    let mut ctx = ctx_both(&root);
    AppIcon
        .apply(&mut ctx, &cfg_with("assets/icon.png"))
        .unwrap();

    let android = ctx.android.as_ref().unwrap();
    for (qualifier, px) in ADAPTIVE_DENSITIES {
        let path = PathBuf::from(format!(
            "app/src/main/res/mipmap-{qualifier}/ic_launcher_foreground.png"
        ));
        let bytes = android
            .extra_files
            .get(&path)
            .unwrap_or_else(|| panic!("missing {}", path.display()))
            .to_bytes()
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (*px, *px));
    }

    let xml = String::from_utf8(
        android.extra_files[Path::new("app/src/main/res/mipmap-anydpi-v26/ic_launcher.xml")]
            .to_bytes()
            .unwrap(),
    )
    .unwrap();
    assert!(xml.contains(r#"@color/ic_launcher_background"#), "{xml}");
    assert!(xml.contains(r#"@mipmap/ic_launcher_foreground"#), "{xml}");
    assert!(!xml.contains("monochrome"), "{xml}");

    let colors = String::from_utf8(
        android.extra_files[Path::new("app/src/main/res/values/ic_launcher_background.xml")]
            .to_bytes()
            .unwrap(),
    )
    .unwrap();
    assert!(colors.contains("#FFFFFF"), "{colors}");

    assert!(!android.extra_files.contains_key(Path::new(
        "app/src/main/res/mipmap-mdpi/ic_launcher_background.png"
    )));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_custom_adaptive_layers_and_color() {
    let root = unique_tempdir("adaptive-custom");
    write_png(&root, "assets/icon.png", 1024, [10, 20, 30, 255]);
    write_png(&root, "assets/fg.png", 512, [1, 2, 3, 128]);
    write_png(&root, "assets/mono.png", 432, [255, 255, 255, 200]);
    let mut cfg = cfg_with("assets/icon.png");
    cfg.android_foreground("assets/fg.png")
        .android_background_color("#1E90FF")
        .android_monochrome("assets/mono.png");
    let mut ctx = ctx_both(&root);
    AppIcon.apply(&mut ctx, &cfg).unwrap();

    let android = ctx.android.as_ref().unwrap();
    let xml = String::from_utf8(
        android.extra_files[Path::new("app/src/main/res/mipmap-anydpi-v26/ic_launcher.xml")]
            .to_bytes()
            .unwrap(),
    )
    .unwrap();
    assert!(xml.contains(r#"@mipmap/ic_launcher_monochrome"#), "{xml}");

    let colors = String::from_utf8(
        android.extra_files[Path::new("app/src/main/res/values/ic_launcher_background.xml")]
            .to_bytes()
            .unwrap(),
    )
    .unwrap();
    assert!(colors.contains("#1E90FF"), "{colors}");

    for (qualifier, _) in ADAPTIVE_DENSITIES {
        assert!(android.extra_files.contains_key(&PathBuf::from(format!(
            "app/src/main/res/mipmap-{qualifier}/ic_launcher_monochrome.png"
        ))));
    }
    let fg = image::load_from_memory(
        &android.extra_files
            [Path::new("app/src/main/res/mipmap-xxxhdpi/ic_launcher_foreground.png")]
        .to_bytes()
        .unwrap(),
    )
    .unwrap();
    assert!(fg.color().has_alpha(), "foreground must keep transparency");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_background_image_replaces_color_resource() {
    let root = unique_tempdir("adaptive-bgimg");
    write_png(&root, "assets/icon.png", 1024, [10, 20, 30, 255]);
    write_png(&root, "assets/bg.png", 512, [0, 60, 120, 255]);
    let mut cfg = cfg_with("assets/icon.png");
    cfg.android_background("assets/bg.png");
    let mut ctx = ctx_both(&root);
    AppIcon.apply(&mut ctx, &cfg).unwrap();

    let android = ctx.android.as_ref().unwrap();
    let xml = String::from_utf8(
        android.extra_files[Path::new("app/src/main/res/mipmap-anydpi-v26/ic_launcher.xml")]
            .to_bytes()
            .unwrap(),
    )
    .unwrap();
    assert!(xml.contains(r#"@mipmap/ic_launcher_background"#), "{xml}");
    assert!(
        !android.extra_files.contains_key(Path::new(
            "app/src/main/res/values/ic_launcher_background.xml"
        )),
        "color resource must not be emitted alongside a background image",
    );
    for (qualifier, _) in ADAPTIVE_DENSITIES {
        assert!(android.extra_files.contains_key(&PathBuf::from(format!(
            "app/src/main/res/mipmap-{qualifier}/ic_launcher_background.png"
        ))));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn validate_rejects_adaptive_misconfiguration() {
    let mut cfg = cfg_with("assets/icon.png");
    cfg.android_background("assets/bg.png")
        .android_background_color("#FFFFFF");
    let err = AppIcon.validate(&cfg).unwrap_err();
    assert!(err.to_string().contains("pick one"), "{err}");

    let mut cfg = cfg_with("assets/icon.png");
    cfg.android_background_color("blue");
    let err = AppIcon.validate(&cfg).unwrap_err();
    assert!(err.to_string().contains("hex color"), "{err}");

    let mut cfg = AppIconConfig::default();
    cfg.android_foreground("assets/fg.png");
    let err = AppIcon.validate(&cfg).unwrap_err();
    assert!(err.to_string().contains("`source` is not"), "{err}");

    let mut cfg = cfg_with("assets/icon.png");
    cfg.android_monochrome("../mono.png");
    let err = AppIcon.validate(&cfg).unwrap_err();
    assert!(err.to_string().contains(".."), "{err}");
}

#[test]
fn apply_errors_on_undersized_adaptive_layer() {
    let root = unique_tempdir("adaptive-small");
    write_png(&root, "assets/icon.png", 1024, [10, 20, 30, 255]);
    write_png(&root, "assets/fg.png", 256, [1, 2, 3, 255]);
    let mut cfg = cfg_with("assets/icon.png");
    cfg.android_foreground("assets/fg.png");
    let mut ctx = ctx_both(&root);
    let err = AppIcon.apply(&mut ctx, &cfg).unwrap_err();
    assert!(err.to_string().contains("432"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Write a minimal Icon Composer bundle fixture.
fn write_icon_bundle(root: &Path, rel: &str) {
    let bundle = root.join(rel);
    std::fs::create_dir_all(bundle.join("Assets")).unwrap();
    std::fs::write(
            bundle.join("icon.json"),
            r#"{"fill":{"solid":"srgb:1,1,1,1"},"groups":[{"layers":[{"image-name":"glyph.png","name":"glyph"}]}],"supported-platforms":{"squares":"shared"}}"#,
        )
        .unwrap();
    let img = image::RgbaImage::from_pixel(512, 512, image::Rgba([0, 90, 200, 255]));
    DynamicImage::ImageRgba8(img)
        .save_with_format(bundle.join("Assets/glyph.png"), image::ImageFormat::Png)
        .unwrap();
}

#[test]
fn apply_ios_icon_bundle_replaces_asset_catalog() {
    let root = unique_tempdir("ios-icon");
    write_png(&root, "assets/icon.png", 1024, [10, 20, 30, 255]);
    write_icon_bundle(&root, "assets/MyFancy.icon");
    let mut cfg = cfg_with("assets/icon.png");
    cfg.ios_icon("assets/MyFancy.icon");
    let mut ctx = ctx_both(&root);
    AppIcon.apply(&mut ctx, &cfg).unwrap();

    let ios = ctx.ios.as_ref().unwrap();
    assert!(
        ios.extra_files
            .contains_key(Path::new("AppIcon.icon/icon.json"))
    );
    assert!(
        ios.extra_files
            .contains_key(Path::new("AppIcon.icon/Assets/glyph.png"))
    );
    assert!(
        !ios.extra_files
            .keys()
            .any(|p| p.starts_with("Assets.xcassets")),
        "asset catalog must not be emitted when ios_icon is set",
    );
    assert!(ios.pbxproj_ops.iter().any(|op| {
        matches!(op, PbxprojOp::AddResource { path } if path == Path::new("AppIcon.icon"))
    }));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_errors_on_bad_ios_icon_bundle() {
    let root = unique_tempdir("ios-icon-bad");
    write_png(&root, "assets/icon.png", 1024, [10, 20, 30, 255]);

    let mut cfg = cfg_with("assets/icon.png");
    cfg.ios_icon("assets/Nope.icon");
    let mut ctx = ctx_both(&root);
    let err = AppIcon.apply(&mut ctx, &cfg).unwrap_err();
    assert!(err.to_string().contains("not a directory"), "{err}");

    std::fs::create_dir_all(root.join("assets/Empty.icon")).unwrap();
    let mut cfg = cfg_with("assets/icon.png");
    cfg.ios_icon("assets/Empty.icon");
    let mut ctx = ctx_both(&root);
    let err = AppIcon.apply(&mut ctx, &cfg).unwrap_err();
    assert!(err.to_string().contains("icon.json"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn validate_rejects_non_icon_extension_for_ios_icon() {
    let mut cfg = cfg_with("assets/icon.png");
    cfg.ios_icon("assets/AppIcon.png");
    let err = AppIcon.validate(&cfg).unwrap_err();
    assert!(err.to_string().contains(".icon"), "{err}");

    let mut cfg = cfg_with("assets/icon.png");
    cfg.ios_icon("../AppIcon.icon");
    let err = AppIcon.validate(&cfg).unwrap_err();
    assert!(err.to_string().contains(".."), "{err}");

    let mut cfg = cfg_with("assets/icon.png");
    cfg.ios_icon("assets/AppIcon.icon");
    AppIcon.validate(&cfg).unwrap();
}

#[test]
fn apply_android_only_skips_ios() {
    let root = unique_tempdir("android-only");
    write_png(&root, "assets/icon.png", 1024, [1, 2, 3, 255]);
    let mut ctx = GenerateContext {
        android: Some(AndroidProjectIr::default()),
        app_crate_dir: Some(root.clone()),
        ..Default::default()
    };
    AppIcon
        .apply(&mut ctx, &cfg_with("assets/icon.png"))
        .unwrap();
    assert!(ctx.ios.is_none());
    assert_eq!(
        ctx.android.unwrap().extra_files.len(),
        ANDROID_DENSITIES.len() + ADAPTIVE_DENSITIES.len() + 2
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn runs_before_application_attributes_builtin() {
    assert_eq!(
        AppIcon.before(),
        &["whisker-android-application-attributes"]
    );
}
