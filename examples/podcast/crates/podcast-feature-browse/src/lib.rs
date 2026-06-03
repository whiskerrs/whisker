//! Browse screen — the only screen so far.
//!
//! ## State machine
//!
//! ```text
//!   resource(fetch) ──► Loading ──► Ready(Vec<ChartSection>)
//!                            ╲
//!                             ╰──► Error(FetchError)
//! ```
//!
//! `resource(...)` spawns a worker, calls the data layer's
//! `fetch_browse_screen()`, and marshals the result back to the
//! main thread. The view branches between a `loading_state`, an
//! `error_state`, and the populated browse content.
//!
//! ## Layout
//!
//! ```text
//!   ┌───────────────────────────┐
//!   │   top_nav  (fixed top)    │
//!   ├───────────────────────────┤
//!   │   scroll_view (vertical)  │
//!   │   ┌─ section ─────────┐   │
//!   │   │  section_header   │   │
//!   │   │  horizontal_row   │   │
//!   │   └───────────────────┘   │
//!   │   ┌─ section ─────────┐   │
//!   │   │  …                │   │
//!   │   └───────────────────┘   │
//!   ├───────────────────────────┤
//!   │   mini_player (float)     │
//!   └───────────────────────────┘
//! ```
//!
//! The mini player floats above the scroll view via absolute
//! positioning, so the bottom-most card row needs trailing
//! padding equal to its height + bottom inset — otherwise the
//! last card hides behind the player on initial scroll-to-end.

use podcast_data::fetch_browse_screen;
use podcast_domain::{ChartSection, Podcast, SectionLayout};
use podcast_theme as theme;
use podcast_ui_kit::{
    FeaturedCard, FeaturedCardProps, HorizontalRow, HorizontalRowProps, MiniPlayer,
    MiniPlayerProps, RankedCard, RankedCardProps, SectionHeader, SectionHeaderProps, TopNav,
    TopNavProps,
};
use whisker::prelude::*;
use whisker::runtime::tasks::run_blocking;
use whisker::runtime::view::Element;

/// Browse screen root. Mount under the app `page`.
#[component]
pub fn browse_screen() -> Element {
    let sections = resource(fetch_sections);

    let root_style = "flex-grow: 1; flex-shrink: 1; \
                      display: flex; flex-direction: column; \
                      position: relative;"
        .to_string();

    render! {
        view(style: root_style) {
            top_nav(title: "Podcasts", action_label: "Sign In")
            Show(
                when: move || sections.get().is_some(),
                fallback: move || render! {
                    status_pane(
                        message: if sections.error().is_some() {
                            "Couldn't load podcasts.".to_string()
                        } else {
                            "Loading…".to_string()
                        }
                    )
                },
            ) {
                browse_body(
                    sections: sections.get().unwrap_or_default(),
                )
            }
            mini_player()
        }
    }
}

/// Wrap the blocking data-layer call in an async future so
/// `resource()` can drive it.
///
/// `resource()`'s contract is `Result<T, String>` — flatten the
/// data layer's typed `FetchError` to a printable message at this
/// boundary. The UI doesn't pattern-match on error type today; if
/// that changes (separate "offline" vs "rate-limited" UI), thread
/// the typed error through instead.
async fn fetch_sections() -> Result<Vec<ChartSection>, String> {
    run_blocking(fetch_browse_screen)
        .await
        .map_err(|e| e.to_string())
}

/// Centred status pane shown during loading / error states. Same
/// shape regardless of which state — only the message varies — so
/// the layout doesn't shift when state transitions.
#[component]
fn status_pane(message: String) -> Element {
    let pane_style = "flex-grow: 1; flex-shrink: 1; \
                      display: flex; flex-direction: row; \
                      align-items: center; justify-content: center;"
        .to_string();
    let text_style = format!("font-size: 14px; color: {fg};", fg = theme::TEXT_SECONDARY,);
    render! {
        view(style: pane_style) {
            text(style: text_style, value: message.clone())
        }
    }
}

/// Populated browse content: a vertically-scrolling stack of
/// horizontal sections.
#[component]
fn browse_body(sections: Vec<ChartSection>) -> Element {
    let scroll_style = "flex-grow: 1; flex-shrink: 1; width: 100%;".to_string();
    // The vertical column inside the scroll-view. Bottom padding
    // = mini-player height + its bottom inset + a small breath so
    // the last section's card row doesn't hide behind the floating
    // player.
    let column_style = "display: flex; flex-direction: column; \
                        padding-top: 8px; padding-bottom: 96px;"
        .to_string();

    // `each:` value computes a fresh clone INSIDE its own scope so
    // the surrounding render! Fn closure doesn't lose ownership of
    // `sections` to the each-closure's `move ||`. Bare
    // `move || sections.clone()` would consume `sections` (the
    // restored prop) from the render closure on first invocation
    // and break re-render correctness.
    render! {
        scroll_view(
            style: scroll_style,
            scroll_orientation: "vertical",
            scroll_bar_enable: false,
            bounces: true,
        ) {
            view(style: column_style) {
                ForEach(
                    each: {
                        let items = sections.clone();
                        move || items.clone()
                    },
                    key: |s: &ChartSection| s.title.clone(),
                    children: |s: ChartSection| render! { section_block(section: s) },
                )
            }
        }
    }
}

/// One vertical section: header + horizontal row of cards.
/// Switches between the Featured and Ranked card variants based on
/// `section.layout`.
#[component]
fn section_block(section: ChartSection) -> Element {
    let block_style = format!(
        "display: flex; flex-direction: column; \
         margin-top: {gap}; \
         padding-top: 4px;",
        gap = theme::SECTION_GAP,
    );
    let header_gap_style = format!("height: {gap}; width: 100%;", gap = theme::HEADER_GAP,);

    let layout = section.layout;
    let title = section.title.clone();
    let items_for_each = section.items.clone();

    render! {
        view(style: block_style) {
            section_header(title: title, show_chevron: layout == SectionLayout::Ranked)
            view(style: header_gap_style)
            horizontal_row {
                ForEach(
                    each: {
                        let items = items_for_each.clone();
                        move || enumerate(items.clone())
                    },
                    key: |(_, p): &(u32, Podcast)| p.id,
                    children: move |(rank, podcast): (u32, Podcast)| render! {
                        card_with_gap(podcast: podcast, rank: rank, layout: layout)
                    },
                )
            }
        }
    }
}

/// One card + trailing margin. The margin is applied here (not on
/// the card itself) so the kit components stay layout-agnostic
/// — a different screen could space the cards differently without
/// touching the cards.
#[component]
fn card_with_gap(podcast: Podcast, rank: u32, layout: SectionLayout) -> Element {
    let wrap_style = format!(
        "margin-right: {gap}; \
         display: flex; flex-direction: column;",
        gap = theme::CARD_GAP,
    );
    // Each Show child closure captures the podcast by move (render!
    // wraps them as `move ||` for re-render correctness). The same
    // outer FnMut can't move `podcast` out twice — clone once per
    // Show into a local, move the local. Cheap: Podcast's fields
    // are mostly small strings.
    let podcast_for_featured = podcast.clone();
    let podcast_for_ranked = podcast.clone();
    render! {
        view(style: wrap_style) {
            Show(when: move || layout == SectionLayout::Featured, fallback: || render! { fragment() }) {
                featured_card(podcast: podcast_for_featured.clone())
            }
            Show(when: move || layout == SectionLayout::Ranked, fallback: || render! { fragment() }) {
                ranked_card(podcast: podcast_for_ranked.clone(), rank: rank)
            }
        }
    }
}

/// `(rank, item)` pairs — ranks are 1-based to match the visual
/// numbering in the design.
fn enumerate(items: Vec<Podcast>) -> Vec<(u32, Podcast)> {
    items
        .into_iter()
        .enumerate()
        .map(|(i, p)| (i as u32 + 1, p))
        .collect()
}
