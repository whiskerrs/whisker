// `whisker.rs` for the Bluesky example.
//
// Metadata only. The native modules this app uses (WebView, Input,
// LocalStore, Image) auto-wire from their `[package.metadata.whisker]`
// markers — no plugin registration needed. Networking + crypto run in
// pure Rust on whisker's tokio feature, so there's no plugin to configure.

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
}
