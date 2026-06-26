// `whisker.rs` for the `tokio-smoke` example.
//
// No build plugins — this app only exercises the `tokio` feature's
// runtime, which whisker-driver stands up at bootstrap. The config below
// is just enough metadata for `whisker run` to generate the native shells.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerTokioSmoke")
        .bundle_id("rs.whisker.tokiosmoke")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.tokiosmoke")
            .application_id("rs.whisker.tokiosmoke")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.tokiosmoke")
            .scheme("WhiskerTokioSmoke")
            .deployment_target("13.0");
    });
}
