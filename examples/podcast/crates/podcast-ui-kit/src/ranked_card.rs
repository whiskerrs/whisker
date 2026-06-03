//! Ranked grid card — used in "Top Shows" / "New Shows".
//!
//! Square artwork on top, then a metadata block: a numeric rank on
//! the leading edge, with title + subtitle stacked to the right.
//! Width is fixed via [`podcast_theme::RANKED_CARD_SIDE`] so the
//! parent horizontal row knows the intrinsic size.

use podcast_domain::Podcast;
use podcast_theme as theme;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_image::{Image, ImageProps};

#[component]
pub fn ranked_card(podcast: Podcast, rank: u32) -> Element {
    let card_style = format!(
        "width: {w}; \
         display: flex; flex-direction: column;",
        w = theme::RANKED_CARD_SIDE,
    );
    let art_style = format!(
        "width: {w}; height: {w}; \
         border-radius: {r}; \
         background-color: {surface};",
        w = theme::RANKED_CARD_SIDE,
        r = theme::ARTWORK_RADIUS,
        surface = theme::SURFACE,
    );
    let meta_row_style = "display: flex; flex-direction: row; \
                          margin-top: 10px; align-items: flex-start;"
        .to_string();
    let rank_style = format!(
        "font-size: 18px; color: {fg}; \
         font-weight: 700; margin-right: 8px; \
         min-width: 18px;",
        fg = theme::TEXT_PRIMARY,
    );
    let stack_style = "display: flex; flex-direction: column; \
                       flex-grow: 1; flex-shrink: 1;"
        .to_string();
    let title_style = format!(
        "font-size: {size}; color: {fg}; \
         font-weight: 500; \
         text-maxline: 1; text-overflow: ellipsis;",
        size = theme::T_CARD_TITLE,
        fg = theme::TEXT_PRIMARY,
    );
    let subtitle_style = format!(
        "font-size: {size}; color: {fg}; \
         margin-top: 2px; \
         text-maxline: 2; text-overflow: ellipsis;",
        size = theme::T_CARD_SUBTITLE,
        fg = theme::TEXT_SECONDARY,
    );

    let rank_text = format!("{rank}");
    let title_text = podcast.collection_name.clone();
    let subtitle_text = podcast.artist_name.clone();
    let artwork_src = podcast.artwork_url_600.clone();

    render! {
        view(style: card_style) {
            Image(
                style: art_style,
                src: artwork_src,
                mode: "aspectFill",
            )
            view(style: meta_row_style) {
                text(style: rank_style, value: rank_text)
                view(style: stack_style) {
                    text(style: title_style, value: title_text)
                    text(style: subtitle_style, value: subtitle_text)
                }
            }
        }
    }
}
