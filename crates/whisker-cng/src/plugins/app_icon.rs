//! `whisker-app-icon` — generate the app's launcher/home-screen icon
//! for both platforms from a single square source PNG.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! app.plugin::<AppIcon>(|c| {
//!     c.source("assets/icon.png"); // square PNG, 1024×1024+
//! });
//! ```
//!
//! The user-facing `AppIcon` marker + `AppIconConfig` live in
//! `whisker-config` (the only crate the config probe can name types
//! from); this module is the engine-side implementation registered
//! under the same `PluginConfig::NAME`.
//!
//! ## What `apply` produces
//!
//! - **iOS** — one of two shapes, both registered via a
//!   `PbxprojOp::AddResource` and resolved by the template's
//!   hardcoded `ASSETCATALOG_COMPILER_APPICON_NAME = AppIcon`:
//!   - default: `Assets.xcassets/AppIcon.appiconset/` with a
//!     *single-size* `Contents.json` + one 1024×1024 PNG. actool
//!     derives every runtime size (120×120, 180×180, …) and the
//!     Info.plist icon entries during xcodebuild. Alpha is flattened
//!     onto white first — App Store validation rejects transparent
//!     marketing icons.
//!   - with `ios_icon`: the user's Icon Composer bundle staged as
//!     `AppIcon.icon` (renamed so the build setting matches). actool
//!     renders the Liquid Glass appearances (default / dark / clear /
//!     tinted) on iOS 26+ and flattened fallbacks for older OS
//!     versions. Requires Xcode 26+ to build.
//! - **Android** — via `ctx.android.extra_files`, plus an
//!   `android:icon` attribute on the `<application>` tag (AGP picks
//!   the `res/` tree up from the default source set — no gradle
//!   changes):
//!   - legacy `mipmap-{m,h,xh,xxh,xxxh}dpi/ic_launcher.png`
//!     (48/72/96/144/192 px, Lanczos3 downscale) for API ≤ 25;
//!   - an adaptive icon (API 26+): `mipmap-anydpi-v26/ic_launcher.xml` +
//!     `mipmap-*/ic_launcher_foreground.png` (108dp canvas: 108/162/216/324/432
//!     px). The foreground defaults to `source` over a white background (Expo's
//!     default); users can supply `android_foreground` /
//!     `android_background`(image) / `android_background_color` /
//!     `android_monochrome` (Android 13+ themed icons) explicitly.
//!
//! iOS 18-style dark/tinted PNG variants are deliberately not
//! supported: the `.icon` route covers every appearance (with OS
//! fallbacks), and the single-size catalog is sufficient to ship
//! and to pass App Store submission checks.

use image::DynamicImage;
use image::imageops::FilterType;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use whisker_config::AppIconConfig;
use whisker_plugin::{
    ApplicationAttribute, FileEntry, GenerateContext, Operation, PbxprojOp, Plugin, PluginConfig,
    Target,
};

/// Minimum source edge. 1024 is what the App Store's marketing-icon
/// slot requires verbatim; anything smaller would have to upscale
/// (blurry) so we reject it instead.
const MIN_SOURCE_PX: u32 = 1024;

/// iOS single-size catalog edge.
const IOS_ICON_PX: u32 = 1024;

/// Android legacy launcher densities: `mipmap-<qualifier>/ic_launcher.png`
/// edge in px (48dp × density scale).
const ANDROID_DENSITIES: &[(&str, u32)] = &[
    ("mdpi", 48),
    ("hdpi", 72),
    ("xhdpi", 96),
    ("xxhdpi", 144),
    ("xxxhdpi", 192),
];

/// Adaptive-icon layer densities: `mipmap-<qualifier>/ic_launcher_*.png`
/// edge in px (108dp × density scale — the adaptive canvas, of which
/// launchers show the central ~66%).
const ADAPTIVE_DENSITIES: &[(&str, u32)] = &[
    ("mdpi", 108),
    ("hdpi", 162),
    ("xhdpi", 216),
    ("xxhdpi", 324),
    ("xxxhdpi", 432),
];

/// Minimum edge for explicitly-supplied adaptive layers: the largest
/// rendered density (xxxhdpi). Smaller would upscale.
const MIN_ADAPTIVE_LAYER_PX: u32 = 432;

/// Top-level `Assets.xcassets/Contents.json`. actool wants the
/// catalog root to be a valid container.
const XCASSETS_ROOT_CONTENTS: &str = r#"{
  "info" : {
    "author" : "whisker",
    "version" : 1
  }
}
"#;

/// Single-size appiconset (Xcode 14+): one universal 1024×1024 entry;
/// actool generates every device size from it.
const APPICONSET_CONTENTS: &str = r#"{
  "images" : [
    {
      "filename" : "AppIcon.png",
      "idiom" : "universal",
      "platform" : "ios",
      "size" : "1024x1024"
    }
  ],
  "info" : {
    "author" : "whisker",
    "version" : 1
  }
}
"#;

/// Engine-side implementation of the `whisker-app-icon` built-in.
/// Distinct from `whisker_config::AppIcon` (the declaration marker
/// users name in `app.plugin::<AppIcon>(…)`); both resolve to the
/// same `AppIconConfig::NAME`, which is how a user's declaration
/// reaches this plugin.
pub struct AppIcon;

impl Plugin for AppIcon {
    type Config = AppIconConfig;

    /// Run before the application-attributes built-in so an explicit
    /// user `android:icon` (render-time dedup is last-writer-wins)
    /// overrides the one we contribute.
    fn before(&self) -> &'static [&'static str] {
        &["whisker-android-application-attributes"]
    }

    fn validate(&self, cfg: &AppIconConfig) -> anyhow::Result<()> {
        if cfg.source.is_none() {
            // No source → the whole plugin no-ops, so android_* only
            // configs are a mistake worth flagging.
            let adaptive_set = cfg.android_foreground.is_some()
                || cfg.android_background.is_some()
                || cfg.android_background_color.is_some()
                || cfg.android_monochrome.is_some();
            if adaptive_set {
                anyhow::bail!(
                    "whisker-app-icon: android_* adaptive-icon options are set but \
                     `source` is not — nothing would be generated. Set \
                     `c.source(\"assets/icon.png\")` too (it feeds the iOS icon and \
                     the Android legacy mipmaps).",
                );
            }
            return Ok(());
        }

        let paths = [
            (cfg.source.as_deref(), "source"),
            (cfg.android_foreground.as_deref(), "android_foreground"),
            (cfg.android_background.as_deref(), "android_background"),
            (cfg.android_monochrome.as_deref(), "android_monochrome"),
        ];
        for (path, field) in paths {
            let Some(path) = path else { continue };
            if path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                anyhow::bail!(
                    "whisker-app-icon: {field} `{}` contains `..` — icon paths must be \
                     relative to the app crate root and may not escape it.",
                    path.display(),
                );
            }
            if path.extension().and_then(|e| e.to_str()) != Some("png") {
                anyhow::bail!(
                    "whisker-app-icon: {field} `{}` is not a .png — only PNG sources \
                     are supported.",
                    path.display(),
                );
            }
        }

        if let Some(icon) = &cfg.ios_icon {
            if icon
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                anyhow::bail!(
                    "whisker-app-icon: ios_icon `{}` contains `..` — icon paths must be \
                     relative to the app crate root and may not escape it.",
                    icon.display(),
                );
            }
            if icon.extension().and_then(|e| e.to_str()) != Some("icon") {
                anyhow::bail!(
                    "whisker-app-icon: ios_icon `{}` is not a `.icon` bundle — expected \
                     an Icon Composer export (Xcode 26's Icon Composer app produces a \
                     `Something.icon` folder).",
                    icon.display(),
                );
            }
        }

        if cfg.android_background.is_some() && cfg.android_background_color.is_some() {
            anyhow::bail!(
                "whisker-app-icon: android_background (image) and \
                 android_background_color are both set — an adaptive icon has one \
                 background layer, pick one.",
            );
        }
        if let Some(color) = &cfg.android_background_color {
            let hex = color.strip_prefix('#').unwrap_or("");
            let hex_ok = matches!(hex.len(), 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
            if !hex_ok {
                anyhow::bail!(
                    "whisker-app-icon: android_background_color `{color}` is not a \
                     `#RRGGBB` / `#AARRGGBB` hex color.",
                );
            }
        }
        Ok(())
    }

    fn apply(&self, ctx: &mut GenerateContext, cfg: &AppIconConfig) -> anyhow::Result<()> {
        let Some(source) = &cfg.source else {
            return Ok(());
        };

        let crate_root = ctx.app_crate_dir.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "whisker-app-icon: the engine did not supply the app crate dir, so \
                 `c.source(\"assets/icon.png\")` can't be resolved. This is a Whisker \
                 bug — the plugin runtime must populate `GenerateContext::app_crate_dir`."
            )
        })?;

        let abs = crate_root.join(source);
        let bytes = std::fs::read(&abs).map_err(|e| {
            anyhow::anyhow!(
                "whisker-app-icon: source `{}` could not be read (resolved to `{}`, \
                 relative to the app crate root): {e}",
                source.display(),
                abs.display(),
            )
        })?;
        let img = image::load_from_memory(&bytes).map_err(|e| {
            anyhow::anyhow!(
                "whisker-app-icon: source `{}` is not a decodable PNG: {e}",
                source.display(),
            )
        })?;

        let (w, h) = (img.width(), img.height());
        if w != h {
            anyhow::bail!(
                "whisker-app-icon: source `{}` is {w}×{h} — the icon must be square. \
                 Both stores mask/scale from a square source.",
                source.display(),
            );
        }
        if w < MIN_SOURCE_PX {
            anyhow::bail!(
                "whisker-app-icon: source `{}` is {w}×{h} — at least \
                 {MIN_SOURCE_PX}×{MIN_SOURCE_PX} is required (the App Store's \
                 marketing icon uses the 1024px image verbatim; upscaling would blur it).",
                source.display(),
            );
        }

        // ----- iOS: Icon Composer bundle OR single-size asset catalog ------
        if let Some(ios) = ctx.ios.as_mut() {
            let (resource, count) = if let Some(icon_bundle) = &cfg.ios_icon {
                // Icon Composer bundle: stage the whole tree under
                // the fixed name `AppIcon.icon` so the template's
                // hardcoded `ASSETCATALOG_COMPILER_APPICON_NAME =
                // AppIcon` resolves to it regardless of what the
                // user called their export. actool renders every
                // Liquid Glass appearance (+ pre-iOS-26 fallbacks)
                // from the bundle.
                let files = collect_icon_bundle(&crate_root, icon_bundle)?;
                let count = files.len();
                for (rel, bytes) in files {
                    ios.extra_files.insert(
                        Path::new("AppIcon.icon").join(rel),
                        FileEntry::binary(&bytes),
                    );
                }
                ("AppIcon.icon", count)
            } else {
                let ios_png = encode_png(&flatten_onto_white(&img, IOS_ICON_PX))?;
                ios.extra_files.insert(
                    PathBuf::from("Assets.xcassets/Contents.json"),
                    FileEntry::text(XCASSETS_ROOT_CONTENTS),
                );
                ios.extra_files.insert(
                    PathBuf::from("Assets.xcassets/AppIcon.appiconset/Contents.json"),
                    FileEntry::text(APPICONSET_CONTENTS),
                );
                ios.extra_files.insert(
                    PathBuf::from("Assets.xcassets/AppIcon.appiconset/AppIcon.png"),
                    FileEntry::binary(&ios_png),
                );
                ("Assets.xcassets", 3)
            };
            ctx.journal.record(
                AppIconConfig::NAME,
                Target::Ios,
                "extra_files",
                Operation::ArrayPush { count },
            );

            // A *file* reference (not a folder reference): the
            // extension-derived type (`folder.assetcatalog` /
            // `folder.iconcomposer.icon`) is what makes xcodebuild
            // run actool over the bundle instead of copying the
            // directory verbatim.
            ios.pbxproj_ops.push(PbxprojOp::AddResource {
                path: PathBuf::from(resource),
            });
            ctx.journal.record(
                AppIconConfig::NAME,
                Target::Ios,
                "pbxproj_ops",
                Operation::ArrayPush { count: 1 },
            );
        }

        // ----- Android: legacy mipmaps + adaptive icon + android:icon ------
        if let Some(android) = ctx.android.as_mut() {
            let mut count = 0usize;

            // Legacy launcher icons — the only thing API ≤ 25 reads,
            // and the fallback launchers use when they ignore the
            // adaptive definition.
            for (qualifier, px) in ANDROID_DENSITIES {
                let scaled = img.resize_exact(*px, *px, FilterType::Lanczos3);
                android.extra_files.insert(
                    res_path(qualifier, "ic_launcher.png"),
                    FileEntry::binary(&encode_png(&scaled)?),
                );
                count += 1;
            }

            // Adaptive icon (API 26+). Foreground defaults to the
            // shared source (Expo's default), background to white.
            let foreground = match &cfg.android_foreground {
                Some(p) => load_adaptive_layer(&crate_root, p, "android_foreground")?,
                None => img.clone(),
            };
            let background = cfg
                .android_background
                .as_ref()
                .map(|p| load_adaptive_layer(&crate_root, p, "android_background"))
                .transpose()?;
            let monochrome = cfg
                .android_monochrome
                .as_ref()
                .map(|p| load_adaptive_layer(&crate_root, p, "android_monochrome"))
                .transpose()?;

            for (qualifier, px) in ADAPTIVE_DENSITIES {
                let mut layers: Vec<(&str, &DynamicImage)> =
                    vec![("ic_launcher_foreground.png", &foreground)];
                if let Some(bg) = &background {
                    layers.push(("ic_launcher_background.png", bg));
                }
                if let Some(mono) = &monochrome {
                    layers.push(("ic_launcher_monochrome.png", mono));
                }
                for (name, src) in layers {
                    let scaled = src.resize_exact(*px, *px, FilterType::Lanczos3);
                    android.extra_files.insert(
                        res_path(qualifier, name),
                        FileEntry::binary(&encode_png(&scaled)?),
                    );
                    count += 1;
                }
            }

            let background_ref = if background.is_some() {
                "@mipmap/ic_launcher_background"
            } else {
                "@color/ic_launcher_background"
            };
            android.extra_files.insert(
                PathBuf::from("app/src/main/res/mipmap-anydpi-v26/ic_launcher.xml"),
                FileEntry::text(adaptive_icon_xml(background_ref, monochrome.is_some())),
            );
            count += 1;
            if background.is_none() {
                let color = cfg.android_background_color.as_deref().unwrap_or("#FFFFFF");
                android.extra_files.insert(
                    PathBuf::from("app/src/main/res/values/ic_launcher_background.xml"),
                    FileEntry::text(background_color_xml(color)),
                );
                count += 1;
            }

            ctx.journal.record(
                AppIconConfig::NAME,
                Target::Android,
                "extra_files",
                Operation::ArrayPush { count },
            );

            android
                .manifest
                .application_attributes
                .push(ApplicationAttribute {
                    name: "android:icon".into(),
                    value: "@mipmap/ic_launcher".into(),
                });
            ctx.journal.record(
                AppIconConfig::NAME,
                Target::Android,
                "manifest.application_attributes",
                Operation::ArrayPush { count: 1 },
            );
        }

        Ok(())
    }
}

/// Recursively read an Icon Composer bundle into `(rel, bytes)`
/// pairs, sorted so the downstream inputs fingerprint stays stable.
fn collect_icon_bundle(
    crate_root: &Path,
    bundle: &Path,
) -> anyhow::Result<Vec<(PathBuf, Vec<u8>)>> {
    let abs = crate_root.join(bundle);
    if !abs.is_dir() {
        anyhow::bail!(
            "whisker-app-icon: ios_icon `{}` is not a directory (resolved to `{}`, \
             relative to the app crate root) — expected an Icon Composer bundle, \
             which is a `Something.icon` folder.",
            bundle.display(),
            abs.display(),
        );
    }
    if !abs.join("icon.json").is_file() {
        anyhow::bail!(
            "whisker-app-icon: ios_icon `{}` has no `icon.json` — this doesn't look \
             like an Icon Composer export.",
            bundle.display(),
        );
    }
    let mut out = Vec::new();
    collect_bundle_dir(&abs, &abs, &mut out)?;
    Ok(out)
}

fn collect_bundle_dir(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(PathBuf, Vec<u8>)>,
) -> anyhow::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("whisker-app-icon: read dir `{}`: {e}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(|e| {
            anyhow::anyhow!(
                "whisker-app-icon: read dir entry under `{}`: {e}",
                dir.display()
            )
        })?;
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_bundle_dir(root, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .expect("path is under root by construction")
                .to_path_buf();
            let bytes = std::fs::read(&path)
                .map_err(|e| anyhow::anyhow!("whisker-app-icon: read `{}`: {e}", path.display()))?;
            out.push((rel, bytes));
        }
    }
    Ok(())
}

/// `app/src/main/res/mipmap-<qualifier>/<name>`.
fn res_path(qualifier: &str, name: &str) -> PathBuf {
    Path::new("app/src/main/res")
        .join(format!("mipmap-{qualifier}"))
        .join(name)
}

/// Load + validate one adaptive-icon layer image. Same shape rules
/// as the main source except the minimum edge is the largest
/// rendered density (432 px at xxxhdpi) instead of the App Store's
/// 1024 — these layers never leave the APK.
fn load_adaptive_layer(
    crate_root: &Path,
    source: &Path,
    field: &str,
) -> anyhow::Result<DynamicImage> {
    let abs = crate_root.join(source);
    let bytes = std::fs::read(&abs).map_err(|e| {
        anyhow::anyhow!(
            "whisker-app-icon: {field} `{}` could not be read (resolved to `{}`, \
             relative to the app crate root): {e}",
            source.display(),
            abs.display(),
        )
    })?;
    let img = image::load_from_memory(&bytes).map_err(|e| {
        anyhow::anyhow!(
            "whisker-app-icon: {field} `{}` is not a decodable PNG: {e}",
            source.display(),
        )
    })?;
    let (w, h) = (img.width(), img.height());
    if w != h {
        anyhow::bail!(
            "whisker-app-icon: {field} `{}` is {w}×{h} — adaptive-icon layers must be \
             square (they render on a 108dp×108dp canvas).",
            source.display(),
        );
    }
    if w < MIN_ADAPTIVE_LAYER_PX {
        anyhow::bail!(
            "whisker-app-icon: {field} `{}` is {w}×{h} — at least \
             {MIN_ADAPTIVE_LAYER_PX}×{MIN_ADAPTIVE_LAYER_PX} is required (the xxxhdpi \
             layer renders at {MIN_ADAPTIVE_LAYER_PX} px; 1024×1024 recommended).",
            source.display(),
        );
    }
    Ok(img)
}

/// The `mipmap-anydpi-v26/ic_launcher.xml` adaptive-icon definition.
fn adaptive_icon_xml(background_ref: &str, monochrome: bool) -> String {
    let monochrome_line = if monochrome {
        "\n    <monochrome android:drawable=\"@mipmap/ic_launcher_monochrome\"/>"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="{background_ref}"/>
    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>{monochrome_line}
</adaptive-icon>
"#
    )
}

/// The `values/ic_launcher_background.xml` flat-color resource used
/// when no background image is configured.
fn background_color_xml(color: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<resources>
    <color name="ic_launcher_background">{color}</color>
</resources>
"#
    )
}

/// Resize to `px` and composite over opaque white, dropping alpha.
/// App Store validation rejects the 1024 marketing icon when it has
/// an alpha channel ("can't be transparent"), so the iOS copy is
/// always emitted as 8-bit RGB.
fn flatten_onto_white(img: &DynamicImage, px: u32) -> DynamicImage {
    let rgba = img.resize_exact(px, px, FilterType::Lanczos3).to_rgba8();
    let mut out = image::RgbImage::new(px, px);
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        out.put_pixel(x, y, image::Rgb([blend(p[0]), blend(p[1]), blend(p[2])]));
    }
    DynamicImage::ImageRgb8(out)
}

fn encode_png(img: &DynamicImage) -> anyhow::Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| anyhow::anyhow!("whisker-app-icon: PNG encode failed: {e}"))?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
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
        let contents = ios.extra_files
            [Path::new("Assets.xcassets/AppIcon.appiconset/Contents.json")]
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
        // Fully transparent source — every flattened pixel must be white.
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
        // Foreground layers at the 108dp densities.
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

        // Adaptive definition points at the color background; no
        // monochrome line without an explicit monochrome layer.
        let xml = String::from_utf8(
            android.extra_files[Path::new("app/src/main/res/mipmap-anydpi-v26/ic_launcher.xml")]
                .to_bytes()
                .unwrap(),
        )
        .unwrap();
        assert!(xml.contains(r#"@color/ic_launcher_background"#), "{xml}");
        assert!(xml.contains(r#"@mipmap/ic_launcher_foreground"#), "{xml}");
        assert!(!xml.contains("monochrome"), "{xml}");

        // Default background color is white.
        let colors = String::from_utf8(
            android.extra_files[Path::new("app/src/main/res/values/ic_launcher_background.xml")]
                .to_bytes()
                .unwrap(),
        )
        .unwrap();
        assert!(colors.contains("#FFFFFF"), "{colors}");

        // No background/monochrome bitmaps were emitted.
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

        // Monochrome bitmaps landed at every density; foreground
        // bitmaps keep their alpha channel.
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
        // Both background kinds at once.
        let mut cfg = cfg_with("assets/icon.png");
        cfg.android_background("assets/bg.png")
            .android_background_color("#FFFFFF");
        let err = AppIcon.validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("pick one"), "{err}");

        // Bad color literal.
        let mut cfg = cfg_with("assets/icon.png");
        cfg.android_background_color("blue");
        let err = AppIcon.validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("hex color"), "{err}");

        // Adaptive options without a source.
        let mut cfg = AppIconConfig::default();
        cfg.android_foreground("assets/fg.png");
        let err = AppIcon.validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("`source` is not"), "{err}");

        // Traversal in an adaptive path.
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
        // Bundle staged under the fixed AppIcon.icon name, tree intact.
        assert!(
            ios.extra_files
                .contains_key(Path::new("AppIcon.icon/icon.json"))
        );
        assert!(
            ios.extra_files
                .contains_key(Path::new("AppIcon.icon/Assets/glyph.png"))
        );
        // No PNG-derived catalog alongside it.
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

        // Missing directory.
        let mut cfg = cfg_with("assets/icon.png");
        cfg.ios_icon("assets/Nope.icon");
        let mut ctx = ctx_both(&root);
        let err = AppIcon.apply(&mut ctx, &cfg).unwrap_err();
        assert!(err.to_string().contains("not a directory"), "{err}");

        // Directory without icon.json.
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
        // 5 legacy + 5 foreground + adaptive XML + color resource.
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
}
