//! Ranked grid card — used in "Top Shows" / "New Shows".
//!
//! Square artwork on top, then a metadata block: a numeric rank on
//! the leading edge, with title + subtitle stacked to the right.
//! Width is fixed via [`podcast_theme::RANKED_CARD_SIDE`] so the
//! parent horizontal row knows the intrinsic size.

use podcast_domain::Podcast;
use podcast_theme as theme;
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight, TextOverflow};
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
        View(style: css!(
            width: theme::RANKED_CARD_SIDE,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            Image(
                style: css!(
                    width: theme::RANKED_CARD_SIDE,
                    height: theme::RANKED_CARD_SIDE,
                    border_radius: theme::ARTWORK_RADIUS,
                    background_color: theme::SURFACE,
                ),
                src: artwork_src,
                mode: ImageMode::AspectFill,
            )
            View(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                margin_top: px(10),
                align_items: AlignItems::FlexStart,
            )) {
                Text(
                    style: css!(
                        font_size: px(18),
                        color: theme::TEXT_PRIMARY,
                        font_weight: FontWeight::Bold,
                        margin_right: px(8),
                        min_width: px(18),
                    ),
                    value: rank_text,
                )
                View(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                )) {
                    Text(
                        style: css!(
                            font_size: theme::T_CARD_TITLE,
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Numeric(500),
                            text_overflow: TextOverflow::Ellipsis,
                        ),
                        value: title_text,
                    )
                    Text(
                        style: css!(
                            font_size: theme::T_CARD_SUBTITLE,
                            color: theme::TEXT_SECONDARY,
                            margin_top: px(2),
                            text_overflow: TextOverflow::Ellipsis,
                        ),
                        value: subtitle_text,
                    )
                }
            }
        }
    }
}
