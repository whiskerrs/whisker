#[test]
fn required_component_and_module_element_props_are_checked_by_the_builder_type() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/missing_required_component_prop.rs");
    tests.compile_fail("tests/ui/missing_required_module_event.rs");
}
