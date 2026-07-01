// `whisker.rs` for the `list-smoke` example. Metadata only.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerListSmoke")
        .bundle_id("rs.whisker.listsmoke")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.listsmoke")
            .application_id("rs.whisker.listsmoke")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.listsmoke")
            .scheme("WhiskerListSmoke")
            .deployment_target("13.0");
    });
}
