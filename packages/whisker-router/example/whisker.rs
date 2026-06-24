pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerRouterExample")
        .bundle_id("rs.whisker.router.example")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.router.example")
            .application_id("rs.whisker.router.example")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.router.example")
            .scheme("WhiskerRouterExample")
            .deployment_target("13.0");
    });
}
