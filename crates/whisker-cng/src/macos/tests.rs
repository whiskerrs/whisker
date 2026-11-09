use super::*;

#[test]
fn extracts_required_fields_from_config() {
    let mut config = Config::default();
    config
        .name("Desktop")
        .bundle_id("rs.whisker.desktop")
        .version("2.0.0")
        .build_number(9);
    let inputs = inputs_from(
        &config,
        "demo".into(),
        PathBuf::from("/app"),
        PathBuf::from("/workspace"),
    )
    .unwrap();
    assert_eq!(inputs.app_name, "Desktop");
    assert_eq!(inputs.build_number, 9);
    assert_eq!(inputs.minimum_system_version, "14.0");
}

#[test]
fn app_titles_are_escaped_for_swift_string_literals() {
    assert_eq!(swift_string_body("A \"B\" \\ C"), "A \\\"B\\\" \\\\ C");
    assert_eq!(swift_string_body("tab\t"), "tab\\u{9}");
}
