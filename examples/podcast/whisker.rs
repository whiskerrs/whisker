// `whisker.rs` for the podcast example.
//
// Tells `whisker run` how to install / launch / hot-patch this app:
// the bundle id the simulator should `simctl launch`, the Android
// applicationId `adb am start` expects, the launcher activity, etc.
//
// `whisker run` compiles this file as part of a small probe binary
// that serializes the resulting `Config` to JSON. The host shell
// (`whisker-cli`) reads that JSON and projects the fields it needs
// into a flat `whisker_dev_server::Config`.

pub fn configure(app: &mut whisker_config::Config) {
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

    // Whisker-audio plugin — mirrors expo-audio's plugin surface.
    // Exercise every option so the generated project shows we're
    // contributing the right entries:
    //   - microphone_permission → Info.plist.NSMicrophoneUsageDescription
    //   - record_audio_android → AndroidManifest <uses-permission RECORD_AUDIO>
    //   - enable_background_{recording,playback} → Info.plist.UIBackgroundModes
    //
    // The probe pulls `whisker-audio` with `default-features = false`
    // so only the plugin module is built (no Lynx bridge / runtime
    // overhead) — see `crates/whisker-cli/src/probe.rs`.
    app.plugin::<whisker_audio::WhiskerAudio>(|c| {
        c.microphone_permission("Record clips for podcast episodes.")
            .record_audio_android(true)
            .enable_background_playback(true);
    });
}
