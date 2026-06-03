//! Section header row — "Title >" pattern.
//!
//! The chevron-suffix variant is the trailing form ("Top Shows >",
//! "New Shows >"). The hero variant ("New") is the same component
//! with `show_chevron: false` — a leading style cue that the
//! section is a hero block, not a tappable list. The host screen
//! decides which variant by passing the flag.

use podcast_theme as theme;
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[component]
pub fn section_header(title: String, #[prop(default = false)] show_chevron: bool) -> Element {
    let row_style = format!(
        "width: 100%; \
         padding-left: {gutter}; padding-right: {gutter}; \
         display: flex; flex-direction: row; align-items: center;",
        gutter = theme::GUTTER,
    );
    let title_style = format!(
        "font-size: {size}; font-weight: 700; color: {fg};",
        size = theme::T_HERO,
        fg = theme::TEXT_PRIMARY,
    );

    // Chevron style is built fresh inside the Show child closure (not
    // captured from the outer body) — Show's `children:` closure
    // captures by move and re-runs on `when` changes, so any moved
    // outer String would fail the second invocation. Constructing the
    // string inside the closure body sidesteps that.
    render! {
        view(style: row_style) {
            text(style: title_style, value: title.clone())
            Show(when: move || show_chevron, fallback: || render! { fragment() }) {
                text(
                    style: format!(
                        "font-size: {size}; font-weight: 700; \
                         color: {fg}; margin-left: 8px;",
                        size = theme::T_HERO,
                        fg = theme::TEXT_PRIMARY,
                    ),
                    value: ">",
                )
            }
        }
    }
}
