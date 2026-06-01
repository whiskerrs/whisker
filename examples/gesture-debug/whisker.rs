// `whisker.rs` for the gesture-debug example. Same shape as the
// other demos — declares the iOS / Android app metadata that
// `whisker run` reads via a probe binary.

pub fn configure(app: &mut whisker_app_config::AppConfig) {
    app.name("GestureDebug")
        .bundle_id("rs.whisker.examples.gestureDebug")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.examples.gesturedebug")
            .application_id("rs.whisker.examples.gesturedebug")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.examples.gestureDebug")
            .scheme("GestureDebug")
            .deployment_target("13.0");
    });
}
