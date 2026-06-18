//! Horizontally-scrolling row of arbitrary children.
//!
//! Wraps a Lynx `scroll-view` with `scroll_orientation: horizontal`
//! and applies the page gutter as left padding so the first card
//! visually aligns with the section header. Cards inside provide
//! their own intrinsic widths.

use podcast_theme as theme;
use whisker::Children;
use whisker::css::{AlignItems, Display, FlexDirection};
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[component]
pub fn horizontal_row(children: Children) -> Element {
    render! {
        // `bounces: true` → iOS rubber-banding at the ends;
        // `scroll_bar_enable: false` → Apple-style podcast browsers
        // don't show a scroll bar.
        scroll_view(
            style: css!(width: percent(100), display: Display::Flex),
            scroll_orientation: ScrollOrientation::Horizontal,
            scroll_bar_enable: false,
            bounces: true,
        ) {
            // Inner content row — cards laid out left-to-right with
            // `GUTTER` of breathing room at either side of the row.
            // Card-to-card gap is the caller's concern (the browse
            // screen inserts manual spacer views) so this component
            // stays style-agnostic.
            view(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexStart,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
            )) {
                children()
            }
        }
    }
}
