//! Search screen — real-device repro for resource-reactivity.
//!
//! ## Architecture
//!
//! ```text
//!   query: RwSignal<String>
//!       |
//!       +- Input (whisker-input) -> updates query on every keystroke
//!       |
//!       +- resource(move || async move { query.get(); ... })
//!               |
//!               +- Loading   -> "Searching..."
//!               +- Ready([]) -> "No results" / "Type to search"
//!               +- Ready(v)  -> vertical list of result cards
//!               +- Error(s)  -> error message
//! ```
//!
//! The crux: `query.get()` is called INSIDE the async block, so the
//! resource's reactive tracking catches the signal and re-runs the
//! fetch on every change. This is the pattern the `resource-reactivity`
//! fix targets; the screen exists to exercise it on a real device.
//!
//! ## Layout
//!
//! ```text
//!   +---------------------------+
//!   |  <- back    Search        |   top Bar (safe-area inset)
//!   +---------------------------+
//!   |  search query...          |   native text input
//!   +---------------------------+
//!   |  results list             |
//!   |  (or status pane)         |
//!   +---------------------------+
//! ```

use podcast_data::{FetchError, SearchQuery, search};
use podcast_domain::Podcast;
use podcast_routing::Navigator;
use podcast_theme as theme;
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight, JustifyContent, TextOverflow};
use whisker::prelude::*;
use whisker::runtime::tasks::run_blocking;
use whisker::runtime::view::Element;
use whisker_icons::{Icon, lucide};
use whisker_image::{Image, ImageMode};
use whisker_input::{Input, ReturnKey};
use whisker_safe_area::safe_area_insets;

// ---------------------------------------------------------------------------
// Public screen component
// ---------------------------------------------------------------------------

/// Search screen root. Mount under the app `page` via the `Search`
/// route.
///
/// `query` is a local `RwSignal` -- it owns the current search text.
/// The resource reads `query.get()` INSIDE its async block so the
/// signal subscription is established and re-fetches fire on every
/// keystroke.
#[component]
pub fn search_screen() -> Element {
    let query = RwSignal::new(String::new());

    // THE REACTIVE RESOURCE -- this is the repro for resource-reactivity:
    //
    // `query.get()` is called INSIDE the async block (not outside it),
    // which is the pattern that correctly registers the reactive
    // dependency. When `query` changes (on every keystroke via
    // `Input(text: query, ...)`), the resource re-runs the async block and
    // re-fetches from iTunes.
    //
    // `Resource<Vec<Podcast>>`: `resource()` wraps the fetcher's
    // `Result<T, String>` output internally; `.get()` returns
    // `Option<Vec<Podcast>>` and `.error()` returns `Option<String>`.
    let results: Resource<Vec<Podcast>> = resource(move || async move {
        let q = query.get();
        if q.trim().is_empty() {
            // Return an empty vec without hitting the network when
            // the field is empty -- the status pane shows "Type to search".
            return Ok(Vec::new());
        }
        run_blocking(move || {
            search(SearchQuery {
                term: q.trim(),
                limit: 20,
            })
        })
        .await
        .map_err(|e: FetchError| e.to_string())
    });

    render! {
        View(style: css!(
            flex_grow: 1.0,
            flex_shrink: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
        )) {
            SearchTopBar()
            // Search input field -- two-way bound to `query`.
            // Every keystroke fires the bound `text` signal update,
            // which reactively re-triggers the resource above.
            SearchInputBar(query: query)
            // Results area -- branches on the resource's current state.
            SearchResults(
                results: results,
                query: query,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Top bar
// ---------------------------------------------------------------------------

/// Top bar with back chevron and "Search" title. Mirrors the detail
/// screen's `top_bar` -- pads the status-bar inset and reads
/// `Navigator::go_back` from context.
#[component]
fn search_top_bar() -> Element {
    let insets = safe_area_insets();
    let on_back = use_context::<Navigator>()
        .map(|n| n.go_back)
        .unwrap_or_else(|| std::rc::Rc::new(|| {}));

    let wrapper_style = computed(move || {
        css!(
            width: percent(100),
            padding_top: px(insets.get().top as f32),
            flex_shrink: 0.0,
            background_color: theme::BG,
        )
    });

    render! {
        View(style: wrapper_style) {
            View(style: css!(
                width: percent(100),
                min_height: theme::NAV_HEIGHT,
                flex_shrink: 0.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
            )) {
                View(
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
                    Icon(svg: lucide::ChevronLeft, color: "#a78bfa", size: "26")
                }
                View(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: percent(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                )) {
                    Text(
                        style: css!(
                            font_size: theme::T_NAV_TITLE,
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Numeric(600),
                        ),
                        value: "Search".to_string(),
                    )
                }
                // Spacer keeps the title genuinely centred.
                View(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: percent(0),
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Input bar
// ---------------------------------------------------------------------------

/// Horizontal input row: a search icon on the leading edge + the
/// native `Input` field two-way bound to `query`.
#[component]
fn search_input_bar(query: RwSignal<String>) -> Element {
    render! {
        View(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            margin_left: theme::GUTTER,
            margin_right: theme::GUTTER,
            margin_top: px(8),
            margin_bottom: px(8),
            padding_left: px(12),
            padding_right: px(12),
            padding_top: px(10),
            padding_bottom: px(10),
            border_radius: px(12),
            background_color: theme::SURFACE,
        )) {
            Icon(svg: lucide::Search, color: "#a78bfa", size: "18")
            // `Input(text: query, ...)` -- two-way binding headline API.
            // `query.set(new_text)` is called by the Input component on
            // every keystroke (inside its `on_input` wiring), which
            // triggers the reactive resource above to re-fetch.
            Input(
                text: query,
                placeholder: "Podcasts, shows, episodes...",
                return_key: ReturnKey::Search,
                style: Css::new()
                    .flex_grow(1.0)
                    .flex_shrink(1.0)
                    .margin_left(px(8))
                    .font_size(px(15))
                    .color(Color::hex(0xffffff))
                    .min_height(px(24)),
                placeholder_color: "#8E8E93",
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Results area
// ---------------------------------------------------------------------------

/// Results area: reads the resource state and renders Loading /
/// empty-state / result rows / error accordingly.
///
/// `Resource<Vec<Podcast>>` -- `resource()` internally wraps the fetcher's
/// `Result<Vec<Podcast>, String>` output; `.get()` returns
/// `Option<Vec<Podcast>>` directly. `.error()` exposes the error string.
#[component]
fn search_results(results: Resource<Vec<Podcast>>, query: RwSignal<String>) -> Element {
    render! {
        ScrollView(
            style: css!(
                flex_grow: 1.0,
                flex_shrink: 1.0,
                width: percent(100),
            ),
            axis: ScrollAxis::Vertical,
        ) {
            View(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                padding_bottom: px(96),
            )) {
                Show(
                    // Loading state: resource is in-flight (no value yet,
                    // no error yet, and query is non-empty).
                    when: move || {
                        results.get().is_none()
                            && results.error().is_none()
                            && !query.get().trim().is_empty()
                    },
                    fallback: || render! { Fragment() },
                ) {
                    StatusPane(message: "Searching...".to_string())
                }
                Show(
                    // Empty query: show a gentle prompt.
                    when: move || query.get().trim().is_empty(),
                    fallback: || render! { Fragment() },
                ) {
                    StatusPane(message: "Type to search podcasts".to_string())
                }
                Show(
                    // Error state.
                    when: move || results.error().is_some(),
                    fallback: || render! { Fragment() },
                ) {
                    StatusPane(
                        message: results.error()
                            .unwrap_or_else(|| "Unknown error".to_string()),
                    )
                }
                Show(
                    // Ready with an empty result vec (and non-empty query).
                    when: move || {
                        results.get().map(|v| v.is_empty()).unwrap_or(false)
                            && !query.get().trim().is_empty()
                    },
                    fallback: || render! { Fragment() },
                ) {
                    StatusPane(message: "No results".to_string())
                }
                Show(
                    // Ready with results.
                    when: move || results.get().map(|v| !v.is_empty()).unwrap_or(false),
                    fallback: || render! { Fragment() },
                ) {
                    ResultList(podcasts: results.get().unwrap_or_default())
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Result list
// ---------------------------------------------------------------------------

/// Vertical list of search-result cards.
#[component]
fn result_list(podcasts: Vec<Podcast>) -> Element {
    let podcasts = StoredValue::new(podcasts.clone());
    render! {
        View(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding_left: theme::GUTTER,
            padding_right: theme::GUTTER,
        )) {
            ForEach(
                each: {
                    let items = podcasts.get();
                    move || items.clone()
                },
                key: |p: &Podcast| p.id,
                children: |p: Podcast| render! { ResultRow(podcast: p) },
            )
        }
    }
}

/// One search-result row: small square artwork + title / artist.
/// Tapping navigates to the detail screen via the shared Navigator.
#[component]
fn result_row(podcast: Podcast) -> Element {
    let title = podcast.collection_name.clone();
    let artist = podcast.artist_name.clone();
    let artwork_src = podcast.artwork_url_600.clone();
    let podcast_id = podcast.id;

    let on_show: std::rc::Rc<dyn Fn(u64)> = use_context::<Navigator>()
        .map(|n| n.show_detail)
        .unwrap_or_else(|| std::rc::Rc::new(|_| {}));

    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_top: px(10),
                padding_bottom: px(10),
            ),
            on_tap: move |_| (on_show)(podcast_id),
        ) {
            Image(
                style: css!(
                    width: px(56),
                    height: px(56),
                    border_radius: theme::ARTWORK_RADIUS,
                    flex_shrink: 0.0,
                    background_color: theme::SURFACE,
                ),
                src: artwork_src,
                mode: ImageMode::AspectFill,
            )
            View(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                margin_left: px(12),
            )) {
                Text(
                    style: css!(
                        font_size: px(15),
                        color: theme::TEXT_PRIMARY,
                        font_weight: FontWeight::Numeric(600),
                        text_overflow: TextOverflow::Ellipsis,
                    ),
                    value: title,
                )
                Text(
                    style: css!(
                        font_size: px(13),
                        color: theme::TEXT_SECONDARY,
                        margin_top: px(2),
                        text_overflow: TextOverflow::Ellipsis,
                    ),
                    value: artist,
                )
            }
            Icon(svg: lucide::ChevronRight, color: "#48484A", size: "18")
        }
    }
}

// ---------------------------------------------------------------------------
// Shared status pane
// ---------------------------------------------------------------------------

/// Centred status pane -- loading / empty / error states all share
/// this layout; only the message string varies.
#[component]
fn status_pane(message: String) -> Element {
    let body_message = message.clone();
    render! {
        View(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding_top: px(60),
            padding_bottom: px(60),
        )) {
            Text(
                style: css!(
                    font_size: px(14),
                    color: theme::TEXT_SECONDARY,
                ),
                value: body_message.clone(),
            )
        }
    }
}
