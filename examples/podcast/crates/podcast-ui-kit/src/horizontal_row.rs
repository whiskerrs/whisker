//! Horizontally-scrolling row of arbitrary children.
//!
//! Wraps a `scroll_view` with `axis: ScrollAxis::Horizontal`
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
    let projected = children.clone();
    render! {
        // Host scrolling behavior stays at the standard ScrollView defaults.
        ScrollView(
            style: css!(width: percent(100), display: Display::Flex),
            axis: ScrollAxis::Horizontal,
        ) {
            // Inner content row — cards laid out left-to-right with
            // `GUTTER` of breathing room at either side of the row.
            // Card-to-card gap is the caller's concern (the browse
            // screen inserts manual spacer views) so this component
            // stays style-agnostic.
            View(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexStart,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
            )) {
                { projected() }
            }
        }
    }
}
