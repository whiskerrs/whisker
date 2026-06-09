// `whisker.rs` for the `whisker-svg` example.
//
// Tells `whisker run` how to install / launch / hot-patch this
// app. See `examples/podcast/whisker.rs` for the pattern.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerSvgExample")
        .bundle_id("rs.whisker.svgexample")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.svgexample")
            .application_id("rs.whisker.svgexample")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.svgexample")
            .scheme("WhiskerSvgExample")
            .deployment_target("13.0");
    });
}
