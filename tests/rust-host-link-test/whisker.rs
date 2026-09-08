pub fn configure(app: &mut whisker_config::Config) {
    app.name("Rust Host Link Test")
        .bundle_id("rs.whisker.rusthostlinktest")
        .version("0.0.0")
        .build_number(1);
}
