// `whisker.rs` for the router-demo example.
//
// Same shape as `examples/hello-world/whisker.rs`: `whisker run`
// compiles this file into a probe binary, runs it to get JSON, and
// feeds that into the dev-server config + platform sync (Android
// gradle / iOS xcodeproj generation).

pub fn configure(app: &mut whisker_app_config::AppConfig) {
    app.name("RouterDemo")
        .bundle_id("rs.whisker.examples.routerDemo")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.examples.routerdemo")
            .application_id("rs.whisker.examples.routerdemo")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.examples.routerDemo")
            .scheme("RouterDemo")
            .deployment_target("13.0");
    });
}
