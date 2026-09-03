//! Minimal application whose dependency graph contains every Rust Host module.

use whisker::prelude::*;

#[whisker::main]
pub fn app() -> Element {
    render! {
        View {
            Text(value: "Rust Host link test")
        }
    }
}
