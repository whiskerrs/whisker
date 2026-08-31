//! Section header row — "Title >" pattern.
//!
//! The chevron-suffix variant is the trailing Form ("Top Shows >",
//! "New Shows >"). The hero variant ("New") is the same component
//! with `show_chevron: false` — a leading style cue that the
//! section is a hero block, not a tappable list. The host screen
//! decides which variant by passing the flag.

use podcast_theme as theme;
use whisker::css::{Display, FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{Icon, lucide};

#[component]
pub fn section_header(title: String, #[prop(default = false)] show_chevron: bool) -> Element {
    let body_title = title.clone();
    render! {
        View(style: css!(
            width: percent(100),
            padding_left: theme::GUTTER,
            padding_right: theme::GUTTER,
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: whisker::css::AlignItems::Center,
        )) {
            Text(
                style: css!(
                    font_size: theme::T_HERO,
                    font_weight: FontWeight::Bold,
                    color: theme::TEXT_PRIMARY,
                ),
                value: body_title.clone(),
            )
            // `Show`'s `children:` closure re-runs on `when` changes
            // and captures by move, so any outer-scope String would
            // fail the second invocation. The Icon's props are
            // built fresh inside the closure to sidestep that.
            Show(when: move || show_chevron, fallback: || render! { Fragment() }) {
                View(style: css!(
                    margin_left: px(8),
                    display: Display::Flex,
                    align_items: whisker::css::AlignItems::Center,
                )) {
                    Icon(
                        svg: lucide::ChevronRight,
                        color: "#ffffff",
                        size: "28",
                    )
                }
            }
        }
    }
}
