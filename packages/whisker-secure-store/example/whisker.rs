// `whisker.rs` for the `whisker-secure-store` example.
//
// Tells `whisker run` how to install / launch / hot-patch this app.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerSecureStoreExample")
        .bundle_id("rs.whisker.securestoreexample")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.securestoreexample")
            .application_id("rs.whisker.securestoreexample")
            .launcher_activity(".MainActivity")
            // whisker-secure-store floors at API 23 (Keystore master key).
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.securestoreexample")
            .scheme("WhiskerSecureStoreExample")
            .deployment_target("13.0");
    });
}
