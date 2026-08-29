//! Featured hero card — used in the "New" row.
//!
//! Three stacked elements: category label in small caps above, a
//! two-line podcast title, and a large square artwork below. Width
//! is fixed via [`podcast_theme::FEATURED_CARD_WIDTH`] so the
//! parent horizontal row knows the card's intrinsic size without
//! probing the children.

use podcast_domain::Podcast;
use podcast_theme as theme;
use whisker::css::{Display, FlexDirection, FontWeight, TextOverflow};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_image::{Image, ImageMode};

#[component]
pub fn featured_card(podcast: Podcast) -> Element {
    let category_label = podcast
        .primary_genre_name
        .clone()
        .unwrap_or_else(|| "Featured".to_string())
        .to_uppercase();
    let title_text = podcast.collection_name.clone();
    let artwork_src = podcast.artwork_url_600.clone();

    render! {
        view(style: css!(
            width: theme::FEATURED_CARD_WIDTH,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            text(
                style: css!(
                    font_size: theme::T_CATEGORY,
                    color: theme::TEXT_SECONDARY,
                    font_weight: FontWeight::Numeric(600),
                    letter_spacing: px(0.5),
                ),
                value: category_label,
            )
            text(
                style: css!(
                    font_size: theme::T_FEATURED_TITLE,
                    color: theme::TEXT_PRIMARY,
                    font_weight: FontWeight::Numeric(600),
                    margin_top: px(6),
                    text_overflow: TextOverflow::Ellipsis,
                ),
                value: title_text,
            )
            Image(
                style: css!(
                    width: theme::FEATURED_CARD_WIDTH,
                    height: theme::FEATURED_CARD_WIDTH,
                    border_radius: theme::ARTWORK_RADIUS,
                    margin_top: px(12),
                    background_color: theme::SURFACE,
                ),
                src: artwork_src,
                mode: ImageMode::AspectFill,
            )
        }
    }
}
