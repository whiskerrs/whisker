pub fn configure(app: &mut whisker_config::Config) {
    app.name("Whisker List Benchmark")
        .bundle_id("rs.whisker.listbenchmark")
        .version("0.1.0")
        .build_number(1);
}
