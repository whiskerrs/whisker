// `whisker.rs` for the `whisker-audio` example.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerAudioExample")
        .bundle_id("rs.whisker.audioexample")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.audioexample")
            .application_id("rs.whisker.audioexample")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.audioexample")
            .scheme("WhiskerAudioExample")
            .deployment_target("13.0");
    });
}
