fn main() {
    whisker_cng::run(|app| {
        app.name("Commit City")
            .bundle_id("rs.whisker.commitcity")
            .version("0.1.0")
            .build_number(1)
            .background("#CDD3CF");
        app.web(|web| {
            web.base_path("/examples/commitcity/");
        });
        app.android(|android| {
            android
                .package("rs.whisker.commitcity")
                .application_id("rs.whisker.commitcity")
                .launcher_activity(".MainActivity")
                .min_sdk(24)
                .target_sdk(36);
        });
        app.ios(|ios| {
            ios.bundle_id("rs.whisker.commitcity")
                .scheme("CommitCity")
                .deployment_target("15.0");
        });
    });
}
