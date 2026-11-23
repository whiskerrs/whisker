fn main() {
    whisker_cng::run(|app| {
        app.name("WhiskerSceneExample")
            .bundle_id("rs.whisker.sceneexample")
            .version("0.1.0")
            .build_number(1);

        app.android(|a| {
            a.package("rs.whisker.sceneexample")
                .application_id("rs.whisker.sceneexample")
                .launcher_activity(".MainActivity")
                .min_sdk(24)
                .target_sdk(34);
        });

        app.ios(|i| {
            i.bundle_id("rs.whisker.sceneexample")
                .scheme("WhiskerSceneExample")
                .deployment_target("15.0");
        });
    });
}
