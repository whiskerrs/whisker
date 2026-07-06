// `whisker.rs` for the Bluesky example.
//
// The native modules this app uses (WebView, Input, LocalStore,
// Image) auto-wire from their `[package.metadata.whisker]` markers —
// no plugin registration needed for those. Networking + crypto run in
// pure Rust on whisker's tokio feature. The only declared plugin is
// the built-in app-icon generator at the bottom.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerBluesky")
        .bundle_id("rs.whisker.bluesky")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.bluesky")
            .application_id("rs.whisker.bluesky")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.bluesky")
            .scheme("WhiskerBluesky")
            .deployment_target("13.0");
    });

    // Launcher / home-screen icon (built-in whisker-app-icon plugin).
    // `source` feeds the iOS catalog + Android legacy mipmaps; the
    // android_* options define the adaptive icon (API 26+): butterfly
    // foreground inside the 66% safe zone, flat blue background, and
    // the same silhouette as the Android 13+ themed (monochrome) icon.
    app.plugin::<whisker_config::AppIcon>(|c| {
        c.source("assets/icon.png")
            .android_foreground("assets/icon-fg.png")
            .android_background_color("#0087DC")
            .android_monochrome("assets/icon-fg.png");
    });
}
