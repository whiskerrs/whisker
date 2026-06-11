//! Ranked grid card — used in "Top Shows" / "New Shows".
//!
//! Square artwork on top, then a metadata block: a numeric rank on
//! the leading edge, with title + subtitle stacked to the right.
//! Width is fixed via [`podcast_theme::RANKED_CARD_SIDE`] so the
//! parent horizontal row knows the intrinsic size.

use podcast_domain::Podcast;
use podcast_theme as theme;
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight, TextOverflow, ToCss};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_image::{Image, ImageMode};

#[component]
pub fn ranked_card(podcast: Podcast, rank: u32) -> Element {
    let rank_text = format!("{rank}");
    let title_text = podcast.collection_name.clone();
    let subtitle_text = podcast.artist_name.clone();
    let artwork_src = podcast.artwork_url_600.clone();

    render! {
        view(style: css!(
            width: theme::RANKED_CARD_SIDE,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            Image(
                // `Image` is a `module_component` — its `style` prop
                // is `Signal<String>` with no `From<Css>` impl, so
                // serialise here before handing it across.
                style: css!(
                    width: theme::RANKED_CARD_SIDE,
                    height: theme::RANKED_CARD_SIDE,
                    border_radius: theme::ARTWORK_RADIUS,
                    background_color: theme::SURFACE,
                ).to_css_string(),
                src: artwork_src,
                mode: ImageMode::AspectFill,
            )
            view(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                margin_top: px(10),
                align_items: AlignItems::FlexStart,
            )) {
                text(
                    style: css!(
                        font_size: px(18),
                        color: theme::TEXT_PRIMARY,
                        font_weight: FontWeight::Bold,
                        margin_right: px(8),
                        min_width: px(18),
                    ),
                    value: rank_text,
                )
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                )) {
                    // `text-maxline` is a Lynx-only extension not in
                    // the typed css! builder; `.raw(...)` appends it.
                    text(
                        style: css!(
                            font_size: theme::T_CARD_TITLE,
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Numeric(500),
                            text_overflow: TextOverflow::Ellipsis,
                        ).raw("text-maxline", "1"),
                        value: title_text,
                    )
                    text(
                        style: css!(
                            font_size: theme::T_CARD_SUBTITLE,
                            color: theme::TEXT_SECONDARY,
                            margin_top: px(2),
                            text_overflow: TextOverflow::Ellipsis,
                        ).raw("text-maxline", "2"),
                        value: subtitle_text,
                    )
                }
            }
        }
    }
}
