//! Bottom mini-player bar.
//!
//! Floats above the scrolling content. Three elements: a leading
//! placeholder artwork square (sized for the future "currently
//! playing show art"), a play glyph, and a "skip ahead 30 s" glyph
//! on the trailing edge. No audio wiring yet — the buttons are
//! visual-only until the `whisker-audio` module lands. Both glyphs
//! are now `whisker-icons` Lucide constants — the previous hand-
//! faked `▶` text + circled "30" view layout is gone.

use podcast_theme as theme;
use whisker::css::{AlignItems, Color, Display, FlexDirection, JustifyContent, PositionKind};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{lucide, Icon, IconProps};

#[component]
pub fn mini_player() -> Element {
    render! {
        // Floats above content via absolute positioning. The parent
        // page must use `position: relative` (default) so the floats
        // anchor correctly. Side gutter matches the page gutter so
        // the bar visually inset-aligns with the section content.
        view(style: css!(
            position: PositionKind::Absolute,
            left: theme::GUTTER,
            right: theme::GUTTER,
            bottom: theme::MINI_PLAYER_BOTTOM,
            height: theme::MINI_PLAYER_HEIGHT,
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding_left: px(12),
            padding_right: px(16),
            border_radius: px(12),
            background_color: theme::MINI_PLAYER_BG,
        )) {
            // Leading placeholder: 36×36 square where the now-
            // playing show art would go.
            view(style: css!(
                width: px(36),
                height: px(36),
                border_radius: px(6),
                background_color: Color::rgba(255, 255, 255, 0.15),
            ))
            // Spacer between leading art and trailing controls.
            view(style: css!(flex_grow: 1.0, flex_shrink: 1.0))
            view(style: css!(
                width: px(32),
                height: px(32),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )) {
                Icon(svg: lucide::Play, color: "#ffffff", size: "22")
            }
            view(style: css!(
                width: px(36),
                height: px(36),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                margin_left: px(16),
            )) {
                Icon(svg: lucide::RotateCw, color: "#ffffff", size: "26")
            }
        }
    }
}
