//! App-level CNG configuration. Read by `tuft prebuild` to generate
//! `ios/` and `android/` projects.

use tuft::app_config::AppConfig;

pub fn configure(app: &mut AppConfig) {
    app.name("HelloWorld")
        .bundle_id("rs.tuft.examples.helloworld")
        .version("0.1.0")
        .build_number(1);

    app.ios(|ios| {
        ios.deployment_target("13.0");
    });

    app.android(|android| {
        android
            .package("rs.tuft.examples.helloworld")
            .min_sdk(24)
            .target_sdk(34);
    });
}
