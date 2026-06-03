//! Horizontally-scrolling row of arbitrary children.
//!
//! Wraps a Lynx `scroll-view` with `scroll_orientation: horizontal`
//! and applies the page gutter as left padding so the first card
//! visually aligns with the section header. Cards inside provide
//! their own intrinsic widths.

use podcast_theme as theme;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker::Children;

#[component]
pub fn horizontal_row(children: Children) -> Element {
    // scroll-view with horizontal orientation. `bounces: true` so
    // iOS rubber-banding kicks in at the ends; `scroll_bar_enable:
    // false` because Apple-style podcast browsers don't show one.
    let scroll_style = "width: 100%; display: flex;".to_string();
    // Inner content row — cards laid out left-to-right with
    // `CARD_GAP` between them and `GUTTER` of breathing room at
    // either side of the row.
    let inner_style = format!(
        "display: flex; flex-direction: row; align-items: flex-start; \
         padding-left: {gutter}; padding-right: {gutter};",
        gutter = theme::GUTTER,
    );
    // Each child gets a right margin via wrapping `view`s — done by
    // having the children include a margin in their own style, but
    // to keep the kit's components style-agnostic we instead leave
    // the gap to the caller's section layout. The caller (browse
    // screen) inserts manual spacer views when needed.

    render! {
        scroll_view(
            style: scroll_style,
            scroll_orientation: "horizontal",
            scroll_bar_enable: false,
            bounces: true,
        ) {
            view(style: inner_style) {
                children()
            }
        }
    }
}
