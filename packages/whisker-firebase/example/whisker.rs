fn main() {
    whisker_cng::run(|app| {
        app.name("WhiskerFirebaseExample")
            .bundle_id("rs.whisker.firebaseexample")
            .version("0.1.0");
        app.android(|android| {
            android.min_sdk(24).target_sdk(35);
        });
        app.ios(|ios| {
            ios.deployment_target("15.0");
        });
        app.project_plugin::<whisker_firebase::WhiskerFirebase>(|_| {});
    });
}
