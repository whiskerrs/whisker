// `whisker.rs` for the `whisker-webview` example.
//
// Tells `whisker run` how to install / launch / hot-patch this app.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("WhiskerWebViewExample")
        .bundle_id("rs.whisker.webviewexample")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.webviewexample")
            .application_id("rs.whisker.webviewexample")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.webviewexample")
            .scheme("WhiskerWebViewExample")
            .deployment_target("13.0");
    });
}
