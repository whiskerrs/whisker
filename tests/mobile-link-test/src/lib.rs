//! Minimal user application compiled by the native consumer link tests.

use whisker::prelude::*;

// Kept out of the initial tree intentionally: native link tests must still
// carry its schema into the pre-mount bootstrap registry.
#[whisker::module_component(
    name = "whisker.test/MobileDelayedElement",
    measurement = None,
)]
fn mobile_delayed_element(style: Style) {}

#[whisker::main]
pub fn app() -> Element {
    render! {
        view {
            text(value: "mobile link test")
        }
    }
}
