pub fn configure(app: &mut whisker_config::Config) {
    app.name("Whisker Chat")
        .bundle_id("rs.whisker.chat")
        .version("0.1.0")
        .build_number(1);
    app.android(|android| {
        android
            .package("rs.whisker.chat")
            .application_id("rs.whisker.chat")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(36);
    });
    app.ios(|ios| {
        ios.bundle_id("rs.whisker.chat")
            .scheme("WhiskerChat")
            .deployment_target("15.0");
    });
}
