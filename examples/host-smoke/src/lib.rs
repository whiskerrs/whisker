//! Minimal application used to verify Whisker Host bootstrapping.

use whisker::prelude::*;
use whisker::runtime::view::Element;

#[whisker::main]
pub fn app() -> Element {
    render! {
        view(style: css!(
            flex_grow: 1.0,
            background_color: Color::hex(0x20242A),
            padding: px(24),
        )) {
            text(
                style: css!(
                    color: Color::hex(0xF5F7FA),
                    font_size: px(18),
                ),
                value: "Whisker Host is running",
            )
        }
    }
}
