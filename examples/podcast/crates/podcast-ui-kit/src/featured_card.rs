//! Featured hero card — used in the "New" row.
//!
//! Three stacked elements: category label in small caps above, a
//! two-line podcast title, and a large square artwork below. Width
//! is fixed via [`podcast_theme::FEATURED_CARD_WIDTH`] so the
//! parent horizontal row knows the card's intrinsic size without
//! probing the children.

use podcast_domain::Podcast;
use podcast_theme as theme;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_image::{Image, ImageProps};

#[component]
pub fn featured_card(podcast: Podcast) -> Element {
    let card_style = format!(
        "width: {w}; \
         display: flex; flex-direction: column;",
        w = theme::FEATURED_CARD_WIDTH,
    );
    let category_label = podcast
        .primary_genre_name
        .clone()
        .unwrap_or_else(|| "Featured".to_string())
        .to_uppercase();
    let category_style = format!(
        "font-size: {size}; color: {fg}; \
         letter-spacing: 0.5px; font-weight: 600;",
        size = theme::T_CATEGORY,
        fg = theme::TEXT_SECONDARY,
    );
    let title_text = podcast.collection_name.clone();
    let title_style = format!(
        "font-size: {size}; color: {fg}; \
         font-weight: 600; margin-top: 6px; \
         text-maxline: 2; text-overflow: ellipsis;",
        size = theme::T_FEATURED_TITLE,
        fg = theme::TEXT_PRIMARY,
    );
    let art_style = format!(
        "width: {w}; height: {w}; \
         border-radius: {r}; margin-top: 12px; \
         background-color: {surface};",
        w = theme::FEATURED_CARD_WIDTH,
        r = theme::ARTWORK_RADIUS,
        surface = theme::SURFACE,
    );
    let artwork_src = podcast.artwork_url_600.clone();

    render! {
        view(style: card_style) {
            text(style: category_style, value: category_label)
            text(style: title_style, value: title_text)
            Image(
                style: art_style,
                src: artwork_src,
                mode: "aspectFill",
            )
        }
    }
}
