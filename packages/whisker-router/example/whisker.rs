pub fn configure(app: &mut whisker_app_config::AppConfig) {
    app.name("WhiskerRouterExample")
        .bundle_id("rs.whisker.routerexample")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.routerexample")
            .application_id("rs.whisker.routerexample")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.routerexample")
            .scheme("WhiskerRouterExample")
            .deployment_target("13.0");
    });
}
