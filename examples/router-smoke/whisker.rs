pub fn configure(app: &mut whisker_config::Config) {
    app.name("RouterSmoke")
        .bundle_id("rs.whisker.routersmoke")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.routersmoke")
            .application_id("rs.whisker.routersmoke")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.routersmoke")
            .scheme("RouterSmoke")
            .deployment_target("13.0");
    });
}
