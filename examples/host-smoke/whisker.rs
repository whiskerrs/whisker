pub fn configure(app: &mut whisker_config::Config) {
    app.name("Whisker Host Smoke")
        .bundle_id("rs.whisker.hostsmoke")
        .version("0.1.0")
        .build_number(1);
}
