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
//! `whisker-config`, the only crate the config probe can name types
//! from; this module is the engine-side implementation registered
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

/// Minimum source edge — the App Store's marketing-icon slot takes
/// 1024 verbatim, and anything smaller would upscale blurrily.
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
/// Distinct from the `whisker_config::AppIcon` declaration marker;
/// both resolve to the same `AppIconConfig::NAME`, which is how a
/// user's declaration reaches this plugin.
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

        if let Some(ios) = ctx.ios.as_mut() {
            let (resource, count) = if let Some(icon_bundle) = &cfg.ios_icon {
                // Stage the bundle under the fixed name
                // `AppIcon.icon` so the template's hardcoded
                // `ASSETCATALOG_COMPILER_APPICON_NAME = AppIcon`
                // resolves regardless of the user's export name.
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

        if let Some(android) = ctx.android.as_mut() {
            let mut count = 0usize;

            // The only icons API ≤ 25 reads, and the fallback for
            // launchers that ignore the adaptive definition.
            for (qualifier, px) in ANDROID_DENSITIES {
                let scaled = img.resize_exact(*px, *px, FilterType::Lanczos3);
                android.extra_files.insert(
                    res_path(qualifier, "ic_launcher.png"),
                    FileEntry::binary(&encode_png(&scaled)?),
                );
                count += 1;
            }

            // Adaptive icon (API 26+); foreground defaults to the
            // shared source, background to white.
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
mod tests;
