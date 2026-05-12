//! App-level CNG configuration. Read by `flint prebuild` to generate
//! `ios/` and `android/` projects.

use flint::app_config::AppConfig;

pub fn configure(app: &mut AppConfig) {
    app.name("HelloWorld")
        .bundle_id("dev.flint.examples.helloworld")
        .version("0.1.0")
        .build_number(1);

    app.ios(|ios| {
        ios.deployment_target("13.0");
    });

    app.android(|android| {
        android
            .package("dev.flint.examples.helloworld")
            .min_sdk(24)
            .target_sdk(34);
    });
}
