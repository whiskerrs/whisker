//! Minimal user application compiled by the native consumer link tests.

use whisker::prelude::*;

#[whisker::main]
pub fn app() -> Element {
    render! {
        view {
            text(value: "mobile link test")
        }
    }
}
