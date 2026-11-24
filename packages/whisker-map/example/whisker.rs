fn main() {
    whisker_cng::run(|app| {
        app.name("WhiskerMapExample")
            .bundle_id("rs.whisker.mapexample")
            .version("0.1.0")
            .build_number(1);

        app.android(|a| {
            a.package("rs.whisker.mapexample")
                .application_id("rs.whisker.mapexample")
                .launcher_activity(".MainActivity")
                .min_sdk(24)
                .target_sdk(34);
        });

        app.ios(|i| {
            i.bundle_id("rs.whisker.mapexample")
                .scheme("WhiskerMapExample")
                .deployment_target("15.0");
        });
    });
}
