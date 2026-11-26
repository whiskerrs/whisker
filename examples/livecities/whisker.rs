fn main() {
    whisker_cng::run(|app| {
        app.name("Live Cities")
            .bundle_id("rs.whisker.livecities")
            .version("0.1.0")
            .build_number(1)
            .background("#F7F1E3");
        app.project_plugin::<whisker_asset::WhiskerAsset>(|assets| {
            assets.dir("assets/faces");
        });
        app.web(|web| {
            web.base_path("/examples/livecities/");
        });
        app.android(|android| {
            android
                .package("rs.whisker.livecities")
                .application_id("rs.whisker.livecities")
                .launcher_activity(".MainActivity")
                .min_sdk(24)
                .target_sdk(36);
        });
        app.ios(|ios| {
            ios.bundle_id("rs.whisker.livecities")
                .scheme("LiveCities")
                .deployment_target("15.0");
        });
    });
}
