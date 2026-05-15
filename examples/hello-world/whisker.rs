// `whisker.rs` for the hello-world example.
//
// Tells `whisker run` how to install / launch / hot-patch this app:
// the bundle id the simulator should `simctl launch`, the Android
// applicationId `adb am start` expects, the launcher activity, etc.
//
// `whisker run` compiles this file as part of a small probe binary
// that serializes the resulting `AppConfig` to JSON. The host shell
// (`whisker-cli`) reads that JSON and projects the fields it needs
// into a flat `whisker_dev_server::Config`. The dev-server itself
// never sees `AppConfig` — keeping it manifest-agnostic so other
// host shells (editor plugins, …) can reuse it.
//
// Note: `whisker.rs` is `include!`-ed into a probe binary's main.rs,
// so inner doc comments (`//!`) at the top would fail to compile.
// Use plain `//` comments here.

pub fn configure(app: &mut whisker_app_config::AppConfig) {
    app.name("HelloWorld")
        .bundle_id("rs.whisker.examples.helloWorld")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.examples.helloworld")
            .application_id("rs.whisker.examples.helloworld")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.examples.helloWorld")
            .scheme("HelloWorld")
            .deployment_target("13.0");
    });
}
