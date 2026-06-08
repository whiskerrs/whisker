//! Whisker CNG plugin for the audio module.
//!
//! Mirrors the surface of `expo-audio`'s config plugin
//! (<https://docs.expo.dev/versions/latest/sdk/audio/>) — apps that
//! play or record audio declare a small typed config block in
//! `whisker.rs`, and this plugin contributes the matching
//! `Info.plist` / `AndroidManifest.xml` entries.
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! use whisker_audio::cng::WhiskerAudioConfig;
//!
//! app.plugin::<WhiskerAudioConfig>(|c| c
//!     .microphone_permission("Record audio clips for podcasts.")
//!     .record_audio_android(true)
//!     .enable_background_playback(true));
//! ```
//!
//! ## What each field does
//!
//! | Field                          | Effect |
//! |--------------------------------|---|
//! | [`microphone_permission`](WhiskerAudioConfig::microphone_permission) | Sets `Info.plist.NSMicrophoneUsageDescription` to the supplied string. Required on iOS before `AVAudioSession` can record. |
//! | [`record_audio_android`](WhiskerAudioConfig::record_audio_android) | Appends `android.permission.RECORD_AUDIO` to the Android manifest. Required for runtime audio capture. |
//! | [`enable_background_recording`](WhiskerAudioConfig::enable_background_recording) | Adds `"audio"` to `Info.plist.UIBackgroundModes` so capture survives backgrounding. Bundled with the playback variant — both flags share the same plist entry. |
//! | [`enable_background_playback`](WhiskerAudioConfig::enable_background_playback) | Same `UIBackgroundModes` entry as above, but expresses playback intent for App Store review. |
//!
//! When neither background flag is set, `UIBackgroundModes` stays
//! untouched. When at least one is set, the renderer emits the
//! entry exactly once (the plugin dedups internally).

use serde::{Deserialize, Serialize};
use whisker_plugin::{GenerateContext, Operation, PlistValue, Plugin, PluginConfig, Target};

/// Typed config the user spells in `whisker.rs` via
/// `app.plugin::<WhiskerAudioConfig>(|c| …)`.
#[derive(Default, Serialize, Deserialize)]
pub struct WhiskerAudioConfig {
    /// `NSMicrophoneUsageDescription` text. `None` → don't add
    /// the key; iOS will deny `AVAudioSession` recording attempts
    /// at runtime if the user app actually tries to record.
    #[serde(default)]
    pub microphone_permission: Option<String>,
    /// When true, append `android.permission.RECORD_AUDIO` to the
    /// generated `AndroidManifest.xml`. Required for any Android
    /// audio capture API path. Default: `false`.
    #[serde(default)]
    pub record_audio_android: bool,
    /// When true, advertise audio recording in
    /// `Info.plist.UIBackgroundModes`. Default: `false`.
    #[serde(default)]
    pub enable_background_recording: bool,
    /// When true, advertise audio playback in
    /// `Info.plist.UIBackgroundModes`. Default: `false`.
    #[serde(default)]
    pub enable_background_playback: bool,
}

impl WhiskerAudioConfig {
    /// Fluent setter for [`Self::microphone_permission`].
    pub fn microphone_permission(&mut self, description: impl Into<String>) -> &mut Self {
        self.microphone_permission = Some(description.into());
        self
    }
    /// Fluent setter for [`Self::record_audio_android`].
    pub fn record_audio_android(&mut self, enabled: bool) -> &mut Self {
        self.record_audio_android = enabled;
        self
    }
    /// Fluent setter for [`Self::enable_background_recording`].
    pub fn enable_background_recording(&mut self, enabled: bool) -> &mut Self {
        self.enable_background_recording = enabled;
        self
    }
    /// Fluent setter for [`Self::enable_background_playback`].
    pub fn enable_background_playback(&mut self, enabled: bool) -> &mut Self {
        self.enable_background_playback = enabled;
        self
    }
}

impl PluginConfig for WhiskerAudioConfig {
    const NAME: &'static str = "whisker-audio";
}

/// The plugin the CNG engine drives in-process (1st-party) or
/// spawns as a subprocess (3rd-party path via the
/// `whisker-audio-cng` binary).
pub struct WhiskerAudioPlugin;

impl Plugin for WhiskerAudioPlugin {
    type Config = WhiskerAudioConfig;
    fn apply(&self, ctx: &mut GenerateContext, cfg: &WhiskerAudioConfig) -> anyhow::Result<()> {
        // ----- iOS Info.plist contributions ------------------------------
        if let Some(ios) = ctx.ios.as_mut() {
            if let Some(desc) = cfg.microphone_permission.as_ref() {
                ios.info_plist.insert(
                    "NSMicrophoneUsageDescription".into(),
                    PlistValue::String(desc.clone()),
                );
                ctx.journal.record(
                    WhiskerAudioConfig::NAME,
                    Target::Ios,
                    "info_plist.NSMicrophoneUsageDescription",
                    Operation::Set,
                );
            }

            // Both flags map to the SAME `UIBackgroundModes` entry
            // (just `"audio"`). Set it once if either is on; dedup
            // is internal to this plugin so the rendered plist
            // doesn't show duplicates.
            if cfg.enable_background_recording || cfg.enable_background_playback {
                ios.info_plist.insert(
                    "UIBackgroundModes".into(),
                    PlistValue::Array(vec![PlistValue::String("audio".into())]),
                );
                ctx.journal.record(
                    WhiskerAudioConfig::NAME,
                    Target::Ios,
                    "info_plist.UIBackgroundModes",
                    Operation::Set,
                );
            }
        }

        // ----- Android manifest contributions --------------------------
        if let Some(android) = ctx.android.as_mut() {
            if cfg.record_audio_android {
                android
                    .manifest
                    .permissions
                    .push("android.permission.RECORD_AUDIO".into());
                ctx.journal.record(
                    WhiskerAudioConfig::NAME,
                    Target::Android,
                    "manifest.permissions",
                    Operation::ArrayPush { count: 1 },
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_plugin::{AndroidProjectIr, IosProjectIr};

    fn ctx_both() -> GenerateContext {
        GenerateContext {
            ios: Some(IosProjectIr::default()),
            android: Some(AndroidProjectIr::default()),
            ..Default::default()
        }
    }

    #[test]
    fn default_config_contributes_nothing() {
        let mut ctx = ctx_both();
        WhiskerAudioPlugin
            .apply(&mut ctx, &WhiskerAudioConfig::default())
            .unwrap();
        assert!(ctx.ios.unwrap().info_plist.is_empty());
        assert!(ctx.android.unwrap().manifest.permissions.is_empty());
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn microphone_permission_writes_info_plist_entry() {
        let mut cfg = WhiskerAudioConfig::default();
        cfg.microphone_permission("Record audio clips.");
        let mut ctx = ctx_both();
        WhiskerAudioPlugin.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(
            ctx.ios.unwrap().info_plist["NSMicrophoneUsageDescription"],
            PlistValue::String("Record audio clips.".into()),
        );
    }

    #[test]
    fn record_audio_android_appends_permission() {
        let mut cfg = WhiskerAudioConfig::default();
        cfg.record_audio_android(true);
        let mut ctx = ctx_both();
        WhiskerAudioPlugin.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(
            ctx.android.unwrap().manifest.permissions,
            vec!["android.permission.RECORD_AUDIO".to_string()],
        );
    }

    #[test]
    fn record_audio_android_default_false_appends_nothing() {
        let cfg = WhiskerAudioConfig::default(); // record_audio_android = false
        let mut ctx = ctx_both();
        WhiskerAudioPlugin.apply(&mut ctx, &cfg).unwrap();
        assert!(ctx.android.unwrap().manifest.permissions.is_empty());
    }

    #[test]
    fn enable_background_recording_sets_audio_in_ui_background_modes() {
        let mut cfg = WhiskerAudioConfig::default();
        cfg.enable_background_recording(true);
        let mut ctx = ctx_both();
        WhiskerAudioPlugin.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(
            ctx.ios.unwrap().info_plist["UIBackgroundModes"],
            PlistValue::Array(vec![PlistValue::String("audio".into())]),
        );
    }

    #[test]
    fn enable_background_playback_sets_audio_in_ui_background_modes() {
        let mut cfg = WhiskerAudioConfig::default();
        cfg.enable_background_playback(true);
        let mut ctx = ctx_both();
        WhiskerAudioPlugin.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(
            ctx.ios.unwrap().info_plist["UIBackgroundModes"],
            PlistValue::Array(vec![PlistValue::String("audio".into())]),
        );
    }

    #[test]
    fn both_background_flags_set_audio_exactly_once() {
        let mut cfg = WhiskerAudioConfig::default();
        cfg.enable_background_recording(true)
            .enable_background_playback(true);
        let mut ctx = ctx_both();
        WhiskerAudioPlugin.apply(&mut ctx, &cfg).unwrap();
        let modes = ctx.ios.unwrap().info_plist["UIBackgroundModes"].clone();
        assert_eq!(
            modes,
            PlistValue::Array(vec![PlistValue::String("audio".into())]),
            "background flags should dedup to a single `audio` entry",
        );
    }

    #[test]
    fn full_config_writes_all_three_kinds_of_entries() {
        let mut cfg = WhiskerAudioConfig::default();
        cfg.microphone_permission("Record podcasts.")
            .record_audio_android(true)
            .enable_background_recording(true)
            .enable_background_playback(true);
        let mut ctx = ctx_both();
        WhiskerAudioPlugin.apply(&mut ctx, &cfg).unwrap();
        let ios = ctx.ios.as_ref().unwrap();
        assert!(ios.info_plist.contains_key("NSMicrophoneUsageDescription"));
        assert!(ios.info_plist.contains_key("UIBackgroundModes"));
        let android = ctx.android.as_ref().unwrap();
        assert_eq!(android.manifest.permissions.len(), 1);
    }

    #[test]
    fn no_ios_target_skips_ios_entries() {
        // Single-target compose run for Android only.
        let mut cfg = WhiskerAudioConfig::default();
        cfg.microphone_permission("…").record_audio_android(true);
        let mut ctx = GenerateContext {
            android: Some(AndroidProjectIr::default()),
            ..Default::default()
        };
        WhiskerAudioPlugin.apply(&mut ctx, &cfg).unwrap();
        // Android permission landed.
        assert_eq!(
            ctx.android.unwrap().manifest.permissions,
            vec!["android.permission.RECORD_AUDIO".to_string()],
        );
    }
}
