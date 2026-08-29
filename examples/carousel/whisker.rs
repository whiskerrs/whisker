pub fn configure(app: &mut whisker_config::Config) {
    app.name("Whisker Carousel")
        .bundle_id("rs.whisker.carousel")
        .version("0.1.0")
        .build_number(1);

    app.android(|android| {
        android
            .package("rs.whisker.carousel")
            .application_id("rs.whisker.carousel")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|ios| {
        ios.bundle_id("rs.whisker.carousel")
            .scheme("WhiskerCarousel")
            .deployment_target("13.0");
    });
}
