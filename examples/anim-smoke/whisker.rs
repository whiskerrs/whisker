pub fn configure(app: &mut whisker_config::Config) {
    app.name("AnimSmoke")
        .bundle_id("rs.whisker.animsmoke")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.animsmoke")
            .application_id("rs.whisker.animsmoke")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.animsmoke")
            .scheme("AnimSmoke")
            .deployment_target("13.0");
    });
}
