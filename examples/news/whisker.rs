fn main() {
    whisker_cng::run(|app| {
        app.name("Fieldnotes")
            .bundle_id("rs.whisker.news")
            .version("0.1.0")
            .build_number(1);
        app.project_plugin::<whisker_asset::WhiskerAsset>(|assets| {
            assets.dir("assets");
        });
        app.ios(|ios| {
            ios.scheme("Fieldnotes").deployment_target("15.0");
        });
        app.android(|android| {
            android
                .package("rs.whisker.news")
                .application_id("rs.whisker.news");
        });
    });
}
