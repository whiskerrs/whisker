pub fn configure(app: &mut whisker_config::Config) {
    app.name("Mobile Link Test")
        .bundle_id("rs.whisker.mobilelinktest")
        .version("0.0.0")
        .build_number(1)
        .android(|android| {
            android
                .application_id("rs.whisker.mobilelinktest")
                .min_sdk(24)
                .target_sdk(34);
        })
        .ios(|ios| {
            ios.bundle_id("rs.whisker.mobilelinktest")
                .scheme("MobileLinkTest")
                .deployment_target("13.0");
        });
}
