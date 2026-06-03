// `whisker.rs` for the `whisker-icons` example.

pub fn configure(app: &mut whisker_app_config::AppConfig) {
    app.name("WhiskerIconsExample")
        .bundle_id("rs.whisker.iconsexample")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.iconsexample")
            .application_id("rs.whisker.iconsexample")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.iconsexample")
            .scheme("WhiskerIconsExample")
            .deployment_target("13.0");
    });
}
