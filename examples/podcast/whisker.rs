// `whisker.rs` for the podcast example.
//
// Tells `whisker run` how to install / launch / hot-patch this app:
// the bundle id the simulator should `simctl launch`, the Android
// applicationId `adb am start` expects, the launcher activity, etc.
//
// `whisker run` compiles this file as part of a small probe binary
// that serializes the resulting `AppConfig` to JSON. The host shell
// (`whisker-cli`) reads that JSON and projects the fields it needs
// into a flat `whisker_dev_server::Config`.

pub fn configure(app: &mut whisker_app_config::AppConfig) {
    app.name("Podcast")
        .bundle_id("rs.whisker.podcast")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.podcast")
            .application_id("rs.whisker.podcast")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.podcast")
            .scheme("Podcast")
            .deployment_target("13.0");
    });

    // Whisker-audio CNG plugin — mirrors expo-audio's plugin
    // surface. Exercise every option so the generated project
    // shows we're contributing the right entries:
    //   - microphone_permission → Info.plist.NSMicrophoneUsageDescription
    //   - record_audio_android → AndroidManifest <uses-permission RECORD_AUDIO>
    //   - enable_background_{recording,playback} → Info.plist.UIBackgroundModes
    //
    // Using the raw `app.plugins` map here rather than the typed
    // `app.plugin::<WhiskerAudioConfig>(|c| …)` builder because the
    // config probe (`crates/whisker-cli/src/probe.rs`) is
    // deliberately tiny and only depends on `whisker-app-config` +
    // `serde_json`. Pulling `whisker-audio` into the probe would
    // drag in the runtime + Lynx bridge, blowing up the
    // "single-digit seconds first time" probe budget. Splitting a
    // probe-friendly `whisker-audio-config` crate out is a future
    // improvement that would re-enable the typed builder.
    app.plugins.insert(
        "whisker-audio".to_string(),
        serde_json::json!({
            "microphone_permission": "Record clips for podcast episodes.",
            "record_audio_android": true,
            "enable_background_playback": true,
        }),
    );
}
