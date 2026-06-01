// `whisker.rs` for the anim-demo example. Mirrors the router-demo
// shape — minimal config, no platform modules.

pub fn configure(app: &mut whisker_app_config::AppConfig) {
    app.name("AnimDemo")
        .bundle_id("rs.whisker.examples.animDemo")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.examples.animdemo")
            .application_id("rs.whisker.examples.animdemo")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.examples.animDemo")
            .scheme("AnimDemo")
            .deployment_target("13.0");
    });
}
