//! Podcast detail screen — `/podcast/:id`.
//!
//! Layout:
//!
//! ```text
//!   ┌───────────────────────────┐
//!   │  ← back   title           │   top bar (safe-area inset)
//!   ├───────────────────────────┤
//!   │       ╔═══════════╗       │
//!   │       ║           ║       │   large artwork
//!   │       ║  artwork  ║       │
//!   │       ║           ║       │
//!   │       ╚═══════════╝       │
//!   │      Show title           │
//!   │      Artist name          │
//!   │      Genre · 42 ep        │
//!   │     [ + Follow ]          │   placeholder pill
//!   ├───────────────────────────┤
//!   │  Episodes                 │
//!   │  ─────────                │
//!   │  Episode list placeholder │
//!   └───────────────────────────┘
//! ```
//!
//! The screen reads the shared [`PodcastIndex`] context (set up by
//! the top-level `podcast` crate) to look up the podcast by `id`
//! at render time — no re-fetch, no prop-drilling. If the id is
//! missing (e.g. a deep-link to a podcast Browse hasn't cached
//! yet), the screen renders a "not found" pane. A future PR can
//! plumb the data layer's `fetch_podcast_by_id` through `resource`
//! to handle the deep-link case.
//!
//! Playback is **not** wired up — the play / follow buttons are
//! visual-only. Adding that will be a `whisker-audio` module +
//! plumbing on top of this screen.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use podcast_domain::Podcast;
use podcast_routing::Navigator;
use podcast_theme as theme;
use whisker::css::{
    AlignItems, Display, FlexDirection, FontWeight, JustifyContent, TextAlign, TextOverflow,
};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{lucide, Icon, IconProps};
use whisker_image::{Image, ImageProps};
use whisker_safe_area::safe_area_insets;

/// `Rc<RefCell<HashMap<u64, Podcast>>>` — same alias the top-level
/// `podcast` crate defines. Duplicated here so this crate stays
/// router-agnostic (no dep on the top-level shell) but can still
/// match the context type the host provides.
pub type PodcastIndex = Rc<RefCell<HashMap<u64, Podcast>>>;

/// Show detail screen.
///
/// `id` is the iTunes `collectionId` of the podcast to display.
/// The back chevron pulls [`Navigator::go_back`] from context — the
/// shell wires it to `router::back()`.
#[component]
pub fn detail_screen(id: u64) -> Element {
    let podcast: Option<Podcast> =
        use_context::<PodcastIndex>().and_then(|index| index.borrow().get(&id).cloned());

    match podcast {
        Some(podcast) => detail_body(podcast),
        None => not_found(),
    }
}

/// The populated detail body. Split out so the top-level component
/// stays a thin "lookup + branch" wrapper.
fn detail_body(podcast: Podcast) -> Element {
    let artwork_src = podcast.artwork_url_600.clone();
    let title = podcast.collection_name.clone();
    let artist = podcast.artist_name.clone();
    let genre = podcast.primary_genre_name.clone();
    let track_count = podcast.track_count;
    let is_explicit = podcast.is_explicit();

    // Meta line: "Genre · N episodes". Built once here so the
    // `render!` body doesn't fan out into three nested `text`s.
    let meta_line = build_meta_line(genre, track_count, is_explicit);

    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_shrink: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
        )) {
            top_bar(title: "Show".to_string())
            scroll_view(
                style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    width: percent(100),
                ),
                scroll_orientation: "vertical",
                scroll_bar_enable: false,
                bounces: true,
            ) {
                // Hero block — artwork + title metadata. Padded
                // horizontally by the page gutter so it visually
                // aligns with the section content below.
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding_left: theme::GUTTER,
                    padding_right: theme::GUTTER,
                    padding_top: px(16),
                    padding_bottom: px(24),
                )) {
                    Image(
                        style: css!(
                            width: px(220),
                            height: px(220),
                            border_radius: theme::ARTWORK_RADIUS,
                            background_color: theme::SURFACE,
                        ).raw("aspect-ratio", "1 / 1").to_css_string(),
                        src: artwork_src,
                        mode: "aspectFill",
                    )
                    text(
                        style: css!(
                            font_size: px(22),
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Bold,
                            margin_top: px(16),
                            text_align: TextAlign::Center,
                            text_overflow: TextOverflow::Ellipsis,
                        ).raw("text-maxline", "3"),
                        value: title,
                    )
                    text(
                        style: css!(
                            font_size: px(15),
                            color: theme::ACCENT,
                            margin_top: px(6),
                            text_align: TextAlign::Center,
                        ),
                        value: artist,
                    )
                    text(
                        style: css!(
                            font_size: px(13),
                            color: theme::TEXT_SECONDARY,
                            margin_top: px(8),
                            text_align: TextAlign::Center,
                        ),
                        value: meta_line,
                    )
                    follow_pill()
                }
                // Episodes section header + placeholder list. No
                // RSS-feed wiring yet — that's the playback PR's
                // job. The placeholder uses the same `episode_row`
                // shape an actual episode would render with so a
                // future wiring doesn't move the layout around.
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding_left: theme::GUTTER,
                    padding_right: theme::GUTTER,
                    padding_top: px(8),
                    padding_bottom: px(40),
                )) {
                    text(
                        style: css!(
                            font_size: theme::T_SECTION,
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Bold,
                            margin_bottom: px(8),
                        ),
                        value: "Episodes".to_string(),
                    )
                    episode_placeholder()
                }
            }
        }
    }
}

/// Compose the "Genre · N episodes · E" line. Reused for both
/// the populated and not-found bodies (the latter just gets a
/// generic version).
fn build_meta_line(genre: Option<String>, track_count: u32, is_explicit: bool) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(3);
    if let Some(g) = genre {
        parts.push(g);
    }
    if track_count > 0 {
        // Singular vs plural — small touch, surfaces a single
        // "1 episode" entry the same way iTunes shows it.
        if track_count == 1 {
            parts.push("1 episode".to_string());
        } else {
            parts.push(format!("{} episodes", track_count));
        }
    }
    if is_explicit {
        parts.push("Explicit".to_string());
    }
    parts.join(" · ")
}

/// Top bar with a leading chevron + centered title. Pads the
/// status-bar inset via the shared `safe-area` context, same as
/// the browse screen's `top_nav` — keeps the bars visually flush
/// across the two screens. The back chevron pulls
/// [`Navigator::go_back`] from context (provided by the host).
#[component]
fn top_bar(title: String) -> Element {
    let insets = safe_area_insets();
    let on_back = use_context::<Navigator>()
        .expect("top_bar requires Navigator in context")
        .go_back;
    let wrapper_style = computed(move || {
        css!(
            width: percent(100),
            padding_top: px(insets.get().top as f32),
            flex_shrink: 0.0,
            background_color: theme::BG,
        )
        .to_css_string()
    });

    render! {
        view(style: wrapper_style) {
            view(style: css!(
                width: percent(100),
                min_height: theme::NAV_HEIGHT,
                flex_shrink: 0.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
            )) {
                view(
                    style: css!(
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        flex_basis: percent(0),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::FlexStart,
                    ),
                    on_tap: move |_| (on_back)(),
                ) {
                    Icon(
                        svg: lucide::ChevronLeft,
                        color: "#a78bfa",
                        size: "26",
                    )
                }
                view(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: percent(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                )) {
                    text(
                        style: css!(
                            font_size: theme::T_NAV_TITLE,
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Numeric(600),
                        ),
                        value: title.clone(),
                    )
                }
                // Right-side spacer keeps the title genuinely
                // centered — without it the title slides leftward
                // by the chevron's width.
                view(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: percent(0),
                ))
            }
        }
    }
}

/// Visual-only "+ Follow" pill. No state, no callbacks — the
/// playback / library PR adds the real subscribe flow.
#[component]
fn follow_pill() -> Element {
    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            margin_top: px(20),
            padding_left: px(20),
            padding_right: px(20),
            padding_top: px(10),
            padding_bottom: px(10),
            border_radius: px(20),
            background_color: theme::SURFACE,
        )) {
            Icon(svg: lucide::Plus, color: "#ffffff", size: "16")
            text(
                style: css!(
                    font_size: px(14),
                    color: theme::TEXT_PRIMARY,
                    font_weight: FontWeight::Numeric(600),
                    margin_left: px(6),
                ),
                value: "Follow".to_string(),
            )
        }
    }
}

/// Placeholder strip telling the user the episode list isn't wired
/// yet. Same vertical rhythm a real `episode_row` will take so the
/// future swap doesn't shift the surrounding layout.
#[component]
fn episode_placeholder() -> Element {
    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding_top: px(40),
            padding_bottom: px(40),
            border_radius: px(12),
            background_color: theme::SURFACE,
        )) {
            text(
                style: css!(
                    font_size: px(14),
                    color: theme::TEXT_SECONDARY,
                    text_align: TextAlign::Center,
                ),
                value: "Episode list is coming with the playback PR.".to_string(),
            )
        }
    }
}

/// Rendered when the registry doesn't have `id` (e.g. cold deep-
/// link before browse populated the cache, or the id is bogus).
fn not_found() -> Element {
    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_shrink: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
        )) {
            top_bar(title: "Show".to_string())
            view(style: css!(
                flex_grow: 1.0,
                flex_shrink: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
            )) {
                text(
                    style: css!(
                        font_size: px(16),
                        color: theme::TEXT_SECONDARY,
                        text_align: TextAlign::Center,
                    ),
                    value: "Couldn't find this podcast in the local cache. \
                            Open it from the browse screen first."
                        .to_string(),
                )
            }
        }
    }
}
