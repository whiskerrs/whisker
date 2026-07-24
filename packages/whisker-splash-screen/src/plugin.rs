//! Whisker plugin + config for the splash screen.
//!
//! [`WhiskerSplashScreenConfig`] is the `whisker.rs` builder (image /
//! background / resize / dark variant). [`WhiskerSplashScreen`]'s
//! [`Plugin::apply`] turns it into the native launch screen.
//!
//! **Android** (Android 12 `SplashScreen` API): sets the app theme to a
//! generated splash theme, injects `installSplashScreen()` into
//! `MainActivity`, adds the `androidx.core:core-splashscreen` backport,
//! and emits the theme + icon drawable.
//!
//! **iOS** (`UILaunchScreen`, iOS 14+): sets the `UILaunchScreen`
//! Info.plist dict (background color + centered image) and emits the
//! matching `.colorset` / `.imageset` under `Assets.xcassets`. The image
//! is emitted `@3x`, so `image_width` isn't honored exactly yet (needs
//! image resizing — a follow-up).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use whisker_plugin::{
    FileEntry, GenerateContext, Operation, PlistValue, Plugin, PluginConfig, Target,
};

/// How the splash image is scaled, matching `expo-splash-screen`'s
/// `resizeMode`. (Android 12's animated-icon slot always centers +
/// fits; this is carried for the iOS launch screen + future parity.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResizeMode {
    /// Fit the whole image, preserving aspect ratio. The default.
    #[default]
    Contain,
    /// Fill the screen, cropping overflow.
    Cover,
    /// Native size (iOS falls back to `Contain`, matching Expo).
    Native,
}

/// Dark-mode overrides for the splash. Unset fields fall back to light.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SplashDarkConfig {
    #[serde(default)]
    pub image: Option<PathBuf>,
    #[serde(default)]
    pub background_color: Option<String>,
}

impl SplashDarkConfig {
    /// Dark-mode splash image (path relative to the app crate root).
    pub fn image(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.image = Some(path.into());
        self
    }

    /// Dark-mode background color (e.g. `"#000000"`).
    pub fn background_color(&mut self, color: impl Into<String>) -> &mut Self {
        self.background_color = Some(color.into());
        self
    }
}

/// The `whisker.rs` splash-screen config. Mirrors `expo-splash-screen`'s
/// plugin props.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WhiskerSplashScreenConfig {
    /// Centered splash image (path relative to the app crate root).
    #[serde(default)]
    pub image: Option<PathBuf>,
    /// Displayed image width in dp. `None` → the platform default.
    #[serde(default)]
    pub image_width: Option<u32>,
    /// How the image is scaled.
    #[serde(default)]
    pub resize_mode: ResizeMode,
    /// Background color behind the image (e.g. `"#ffffff"`). `None` →
    /// white.
    #[serde(default)]
    pub background_color: Option<String>,
    /// Optional dark-mode overrides.
    #[serde(default)]
    pub dark: Option<SplashDarkConfig>,
}

impl WhiskerSplashScreenConfig {
    /// Centered splash image (path relative to the app crate root).
    pub fn image(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.image = Some(path.into());
        self
    }

    /// Displayed image width in dp.
    pub fn image_width(&mut self, width: u32) -> &mut Self {
        self.image_width = Some(width);
        self
    }

    /// How the image is scaled (see [`ResizeMode`]).
    pub fn resize_mode(&mut self, mode: ResizeMode) -> &mut Self {
        self.resize_mode = mode;
        self
    }

    /// Background color behind the image (e.g. `"#ffffff"`).
    pub fn background_color(&mut self, color: impl Into<String>) -> &mut Self {
        self.background_color = Some(color.into());
        self
    }

    /// Configure dark-mode overrides.
    pub fn dark(&mut self, f: impl FnOnce(&mut SplashDarkConfig)) -> &mut Self {
        let mut dark = self.dark.take().unwrap_or_default();
        f(&mut dark);
        self.dark = Some(dark);
        self
    }

    /// Background color, defaulting to white.
    fn bg(&self) -> &str {
        self.background_color.as_deref().unwrap_or("#ffffff")
    }
}

impl PluginConfig for WhiskerSplashScreenConfig {
    const NAME: &'static str = "whisker-splash-screen";
}

/// The plugin the Whisker engine drives, and the config-plugin marker.
pub struct WhiskerSplashScreen;

// -- Android resource names (kept in one place) -----------------------

const ANDROID_THEME: &str = "@style/Theme.Whisker.Splash";
const ANDROID_BG_COLOR: &str = "whisker_splash_background";
const ANDROID_ICON: &str = "whisker_splash_icon";
/// The app's normal theme, restored after the splash (`postSplashScreenTheme`).
/// Matches the manifest template's historical default.
const ANDROID_POST_THEME: &str = "@style/Theme.AppCompat.NoActionBar";
const ANDROID_SPLASHSCREEN_DEP: &str = "implementation(\"androidx.core:core-splashscreen:1.0.1\")";

impl Plugin for WhiskerSplashScreen {
    type Config = WhiskerSplashScreenConfig;
    fn apply(
        &self,
        ctx: &mut GenerateContext,
        cfg: &WhiskerSplashScreenConfig,
    ) -> anyhow::Result<()> {
        // Read the splash image up-front (needs the app crate dir; the
        // config path is spelled relative to it). Done before mutating
        // the per-target IR so the borrows stay simple.
        let image_bytes =
            match &cfg.image {
                Some(rel) => {
                    let root = ctx.app_crate_dir.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "whisker-splash-screen: app_crate_dir is required to read the \
                         splash image `{}`",
                            rel.display()
                        )
                    })?;
                    let path = root.join(rel);
                    Some(std::fs::read(&path).map_err(|e| {
                        anyhow::anyhow!("read splash image {}: {e}", path.display())
                    })?)
                }
                None => None,
            };

        if ctx.android.is_some() {
            apply_android(ctx, cfg, image_bytes.as_deref());
        }
        if ctx.ios.is_some() {
            apply_ios(ctx, cfg, image_bytes.as_deref());
        }
        Ok(())
    }
}

/// Generate the Android 12 SplashScreen: point the app theme at a
/// generated splash theme, inject `installSplashScreen()`, add the
/// androidx backport, and emit the theme + icon drawable.
fn apply_android(ctx: &mut GenerateContext, cfg: &WhiskerSplashScreenConfig, image: Option<&[u8]>) {
    let android = ctx.android.as_mut().expect("android checked by caller");

    // 1. Point `<application android:theme>` at the splash theme.
    android.manifest.application_theme = Some(ANDROID_THEME.to_string());

    // 2. Inject `installSplashScreen()` into MainActivity.onCreate
    //    (before super.onCreate — required by the SplashScreen API).
    android
        .manifest
        .main_activity_imports
        .push("androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen".to_string());
    android
        .manifest
        .main_activity_pre_super
        .push("installSplashScreen()".to_string());

    // 3. androidx SplashScreen backport (also enables it on API < 31).
    android
        .gradle
        .dependencies
        .push(ANDROID_SPLASHSCREEN_DEP.to_string());

    // 4. Emit the splash theme + background color (one values file).
    let icon_item = if image.is_some() {
        format!(
            "\n        <item name=\"windowSplashScreenAnimatedIcon\">@drawable/{ANDROID_ICON}</item>"
        )
    } else {
        String::new()
    };
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <resources>\n\
         \x20   <color name=\"{ANDROID_BG_COLOR}\">{bg}</color>\n\
         \x20   <style name=\"Theme.Whisker.Splash\" parent=\"Theme.SplashScreen\">\n\
         \x20       <item name=\"windowSplashScreenBackground\">@color/{ANDROID_BG_COLOR}</item>{icon_item}\n\
         \x20       <item name=\"postSplashScreenTheme\">{ANDROID_POST_THEME}</item>\n\
         \x20   </style>\n\
         </resources>\n",
        bg = cfg.bg(),
    );
    android.extra_files.insert(
        PathBuf::from("app/src/main/res/values/whisker_splash.xml"),
        FileEntry::text(styles),
    );

    // 5. Emit the icon drawable (density-independent so the system just
    //    scales it into the splash icon slot).
    if let Some(bytes) = image {
        android.extra_files.insert(
            PathBuf::from(format!(
                "app/src/main/res/drawable-nodpi/{ANDROID_ICON}.png"
            )),
            FileEntry::binary(bytes),
        );
    }

    let n = WhiskerSplashScreenConfig::NAME;
    ctx.journal.record(
        n,
        Target::Android,
        "manifest.application_theme",
        Operation::Override,
    );
    ctx.journal.record(
        n,
        Target::Android,
        "manifest.main_activity_pre_super",
        Operation::ArrayPush { count: 1 },
    );
    ctx.journal.record(
        n,
        Target::Android,
        "gradle.dependencies",
        Operation::ArrayPush { count: 1 },
    );
    ctx.journal.record(
        n,
        Target::Android,
        "extra_files",
        Operation::ArrayPush { count: 1 },
    );
}

// -- iOS asset names ---------------------------------------------------

const IOS_IMAGE_NAME: &str = "WhiskerSplashLogo";
const IOS_COLOR_NAME: &str = "WhiskerSplashBackground";

/// Generate the iOS launch screen: a `UILaunchScreen` Info.plist dict
/// referencing an asset-catalog color (+ image), and the matching
/// `.colorset` / `.imageset` under the existing `Assets.xcassets`.
///
/// `UILaunchScreen` shows the image at its natural point size; the
/// image is emitted at `@3x`, so `image_width` is not yet honored
/// exactly (that needs image resizing — a follow-up). iOS 14+.
fn apply_ios(ctx: &mut GenerateContext, cfg: &WhiskerSplashScreenConfig, image: Option<&[u8]>) {
    let ios = ctx.ios.as_mut().expect("ios checked by caller");

    // Background color asset.
    let (r, g, b) = parse_hex_rgb(cfg.bg());
    ios.extra_files.insert(
        PathBuf::from(format!(
            "Assets.xcassets/{IOS_COLOR_NAME}.colorset/Contents.json"
        )),
        FileEntry::text(colorset_json(r, g, b)),
    );

    let mut launch = BTreeMap::new();
    launch.insert(
        "UIColorName".to_string(),
        PlistValue::String(IOS_COLOR_NAME.to_string()),
    );
    launch.insert(
        "UIImageRespectsSafeAreaInsets".to_string(),
        PlistValue::Boolean(false),
    );

    // Logo image asset (optional).
    if let Some(bytes) = image {
        ios.extra_files.insert(
            PathBuf::from(format!(
                "Assets.xcassets/{IOS_IMAGE_NAME}.imageset/Contents.json"
            )),
            FileEntry::text(imageset_json()),
        );
        ios.extra_files.insert(
            PathBuf::from(format!(
                "Assets.xcassets/{IOS_IMAGE_NAME}.imageset/logo.png"
            )),
            FileEntry::binary(bytes),
        );
        launch.insert(
            "UIImageName".to_string(),
            PlistValue::String(IOS_IMAGE_NAME.to_string()),
        );
    }

    ios.info_plist
        .insert("UILaunchScreen".to_string(), PlistValue::Dict(launch));

    let n = WhiskerSplashScreenConfig::NAME;
    ctx.journal
        .record(n, Target::Ios, "info_plist.UILaunchScreen", Operation::Set);
    ctx.journal.record(
        n,
        Target::Ios,
        "extra_files",
        Operation::ArrayPush { count: 1 },
    );
}

/// `#RRGGBB` (or `RRGGBB`) → sRGB components in `0.0..=1.0`. Falls back
/// to white on a malformed value.
fn parse_hex_rgb(hex: &str) -> (f32, f32, f32) {
    let h = hex.trim().trim_start_matches('#');
    let comp = |i: usize| {
        h.get(i..i + 2)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .map(|v| v as f32 / 255.0)
    };
    match (comp(0), comp(2), comp(4)) {
        (Some(r), Some(g), Some(b)) if h.len() >= 6 => (r, g, b),
        _ => (1.0, 1.0, 1.0),
    }
}

/// Asset-catalog `Contents.json` for the single `@3x` splash image.
fn imageset_json() -> String {
    "{\n  \"images\" : [\n    {\n      \"idiom\" : \"universal\",\n      \
     \"filename\" : \"logo.png\",\n      \"scale\" : \"3x\"\n    }\n  ],\n  \
     \"info\" : {\n    \"author\" : \"whisker\",\n    \"version\" : 1\n  }\n}\n"
        .to_string()
}

/// Asset-catalog `Contents.json` for the splash background color.
fn colorset_json(r: f32, g: f32, b: f32) -> String {
    format!(
        "{{\n  \"colors\" : [\n    {{\n      \"idiom\" : \"universal\",\n      \
         \"color\" : {{\n        \"color-space\" : \"srgb\",\n        \
         \"components\" : {{\n          \"red\" : \"{r:.3}\",\n          \
         \"green\" : \"{g:.3}\",\n          \"blue\" : \"{b:.3}\",\n          \
         \"alpha\" : \"1.000\"\n        }}\n      }}\n    }}\n  ],\n  \
         \"info\" : {{\n    \"author\" : \"whisker\",\n    \"version\" : 1\n  }}\n}}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_plugin::{AndroidProjectIr, IosProjectIr};

    fn ctx_with_android() -> GenerateContext {
        GenerateContext {
            android: Some(AndroidProjectIr::default()),
            ..Default::default()
        }
    }

    #[test]
    fn builder_captures_config() {
        let mut cfg = WhiskerSplashScreenConfig::default();
        cfg.image("assets/splash.png")
            .resize_mode(ResizeMode::Contain)
            .background_color("#ffffff");
        assert_eq!(cfg.image, Some(PathBuf::from("assets/splash.png")));
        assert_eq!(cfg.resize_mode, ResizeMode::Contain);
        assert_eq!(cfg.bg(), "#ffffff");
    }

    #[test]
    fn android_wires_theme_activity_gradle_and_styles() {
        // No image → styles still emitted (background only), no drawable.
        let mut ctx = ctx_with_android();
        let cfg = {
            let mut c = WhiskerSplashScreenConfig::default();
            c.background_color("#123456");
            c
        };
        WhiskerSplashScreen.apply(&mut ctx, &cfg).unwrap();
        let a = ctx.android.unwrap();

        assert_eq!(a.manifest.application_theme.as_deref(), Some(ANDROID_THEME));
        assert!(
            a.manifest
                .main_activity_pre_super
                .iter()
                .any(|s| s == "installSplashScreen()")
        );
        assert!(
            a.manifest
                .main_activity_imports
                .iter()
                .any(|s| s.contains("installSplashScreen"))
        );
        assert!(
            a.gradle
                .dependencies
                .iter()
                .any(|d| d.contains("core-splashscreen"))
        );

        let styles = a
            .extra_files
            .get(&PathBuf::from("app/src/main/res/values/whisker_splash.xml"))
            .expect("styles emitted");
        assert!(styles.contents.contains("#123456"));
        assert!(styles.contents.contains("parent=\"Theme.SplashScreen\""));
        assert!(styles.contents.contains("postSplashScreenTheme"));
        // No image → no animated-icon item, no drawable file.
        assert!(!styles.contents.contains("windowSplashScreenAnimatedIcon"));
        assert!(
            !a.extra_files
                .keys()
                .any(|p| p.to_string_lossy().contains("drawable"))
        );
    }

    #[test]
    fn missing_app_crate_dir_with_image_errors() {
        // An image path but no `app_crate_dir` → clear error, not a guess.
        let mut ctx = ctx_with_android();
        let mut cfg = WhiskerSplashScreenConfig::default();
        cfg.image("assets/splash.png");
        let err = WhiskerSplashScreen.apply(&mut ctx, &cfg).unwrap_err();
        assert!(err.to_string().contains("app_crate_dir"));
    }

    #[test]
    fn ios_sets_launch_screen_dict_and_colorset() {
        // No-image path (color only) to avoid file I/O.
        let mut ctx = GenerateContext {
            ios: Some(IosProjectIr::default()),
            ..Default::default()
        };
        let mut cfg = WhiskerSplashScreenConfig::default();
        cfg.background_color("#ff0000");
        WhiskerSplashScreen.apply(&mut ctx, &cfg).unwrap();
        let ios = ctx.ios.unwrap();

        match ios.info_plist.get("UILaunchScreen") {
            Some(PlistValue::Dict(d)) => {
                assert!(
                    matches!(d.get("UIColorName"), Some(PlistValue::String(s)) if s == IOS_COLOR_NAME)
                );
                // No image → no UIImageName.
                assert!(!d.contains_key("UIImageName"));
            }
            other => panic!("expected UILaunchScreen dict, got {other:?}"),
        }
        let cs = ios
            .extra_files
            .get(&PathBuf::from(format!(
                "Assets.xcassets/{IOS_COLOR_NAME}.colorset/Contents.json"
            )))
            .expect("colorset emitted");
        assert!(cs.contents.contains("\"red\" : \"1.000\""));
        assert!(cs.contents.contains("\"green\" : \"0.000\""));
    }

    #[test]
    fn parse_hex_rgb_handles_hex_and_junk() {
        assert_eq!(parse_hex_rgb("#ffffff"), (1.0, 1.0, 1.0));
        assert_eq!(parse_hex_rgb("000000"), (0.0, 0.0, 0.0));
        assert_eq!(parse_hex_rgb("#ff0000").0, 1.0);
        assert_eq!(parse_hex_rgb("nope"), (1.0, 1.0, 1.0)); // malformed → white
    }
}
