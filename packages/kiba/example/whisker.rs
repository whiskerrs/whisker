fn main() {
    whisker_cng::run(|app| {
        app.name("KibaExample")
            .bundle_id("rs.whisker.kibaexample")
            .version("0.1.0")
            .build_number(1)
            .project_plugin::<whisker_asset::WhiskerAsset>(|config| {
                config.dir("assets");
            });
    });
}
