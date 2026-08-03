// `whisker.rs` for the `whisker-image` example.
//
// Tells `whisker run` how to install / launch / hot-patch this app.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerImageExample")
        .bundle_id("rs.whisker.imageexample")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.imageexample")
            .application_id("rs.whisker.imageexample")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.imageexample")
            .scheme("WhiskerImageExample")
            .deployment_target("13.0");
    });
}
