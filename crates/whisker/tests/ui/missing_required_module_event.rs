#[whisker::module_element("test-button")]
fn test_button(on_press: ()) {}

fn main() {
    let _ = TestButton::builder().build();
}
