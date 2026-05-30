//! Hacker News reader — real-world Whisker example.
//!
//! Demonstrates the "fetch on a worker thread, update signal on the
//! main thread" pattern using `run_on_main_thread` for the
//! marshaling step. Mirrors React's `useState + useEffect` and
//! Leptos's `signal + Effect::new` shape: one signal holds the load
//! state, `on_mount` kicks off the fetch, and the closure that
//! completes the fetch hops back to the main thread before touching
//! the signal.
//!
//! ### Stack
//!
//! - HTTP: `ureq` (blocking) with built-in rustls — works on
//!   Android / iOS without needing the system cert store.
//! - JSON: `serde` + `serde_json`.
//! - API: Algolia Hacker News search
//!   <https://hn.algolia.com/api/v1/search?tags=front_page> —
//!   one GET, returns 30 front-page stories with title/url/author/
//!   points/num_comments in a single response. No API key needed.
//!
//! ### State machine
//!
//! ```text
//!                ┌────── Loading ──────┐
//!                │                     │
//!         fetch ok                 fetch err
//!                │                     │
//!                ▼                     ▼
//!            Loaded(stories)      Error(msg)
//! ```

use serde::Deserialize;
use whisker::prelude::*;
use whisker::runtime::view::Element;

// ---- Data model -------------------------------------------------------------

/// One Hacker News story as returned by the Algolia API. `serde`
/// is permissive — fields that may be missing for ask/poll posts
/// (no URL, no title) are `Option`s.
#[derive(Debug, Clone, Deserialize)]
pub struct Story {
    #[serde(rename = "objectID")]
    pub object_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub author: Option<String>,
    pub points: Option<u32>,
    pub num_comments: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct HnResponse {
    hits: Vec<Story>,
}

// ---- Palette (HN orange + cream, like the real site) -----------------------

const BG: &str = "#f6f6ef";
const HEADER_BG: &str = "#ff6600";
const HEADER_FG: &str = "#ffffff";
const TEXT_PRIMARY: &str = "#000000";
const TEXT_SECONDARY: &str = "#828282";

// ---- Fetch ------------------------------------------------------------------

/// Blocking HTTPS GET + JSON parse. Runs synchronously — must be
/// called from inside a `run_blocking(...)` so it lands on a worker
/// thread instead of stalling the main TASM thread.
fn fetch_blocking() -> Result<Vec<Story>, String> {
    let url = "https://hn.algolia.com/api/v1/search?tags=front_page&hitsPerPage=30";
    let body = ureq::get(url)
        .call()
        .map_err(|e| format!("network: {e}"))?
        .into_string()
        .map_err(|e| format!("read: {e}"))?;
    let parsed: HnResponse = serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))?;
    Ok(parsed.hits)
}

/// Async wrapper around `fetch_blocking`. `resource()` polls this on
/// the main thread; `run_blocking` hops the blocking HTTP call to a
/// worker thread and resumes here once the bytes are back.
async fn fetch_stories() -> Result<Vec<Story>, String> {
    whisker::runtime::tasks::run_blocking(fetch_blocking).await
}

// ---- Components -------------------------------------------------------------

/// Single story row. `story` is cloned in because `For` hands each
/// item to its `children` closure by value.
///
/// `#[component]` so that editing the row layout / styling reflects
/// via per-component remount on hot reload (issue #17 made
/// `#[component]` layout-transparent).
#[component]
fn story_row(story: Story) -> Element {
    let title = story
        .title
        .clone()
        .unwrap_or_else(|| "(no title)".to_string());
    let domain = story
        .url
        .as_deref()
        .and_then(|u| u.split("//").nth(1))
        .and_then(|s| s.split('/').next())
        .unwrap_or("")
        .to_string();
    let author = story.author.clone().unwrap_or_else(|| "anon".to_string());
    let points = story.points.unwrap_or(0);
    let comments = story.num_comments.unwrap_or(0);
    let meta_text = if domain.is_empty() {
        format!("{points} points by {author} · {comments} comments")
    } else {
        format!("{domain}  ·  {points} points by {author} · {comments} comments")
    };

    let row_style = "width: 100%; display: flex; flex-direction: row; \
                     padding: 12px 16px; \
                     border-bottom-width: 1px; border-bottom-style: solid; \
                     border-bottom-color: rgba(0,0,0,0.06);"
        .to_string();
    let body_inner_style =
        "flex-grow: 1; flex-shrink: 1; display: flex; flex-direction: column;".to_string();
    let title_style = format!("font-size: 15px; color: {TEXT_PRIMARY}; font-weight: 500;");
    let sub_style = format!("font-size: 12px; color: {TEXT_SECONDARY}; margin-top: 4px;");

    render! {
        view(style: row_style) {
            view(style: body_inner_style) {
                text(style: title_style, value: title.clone())
                text(style: sub_style, value: meta_text.clone())
            }
        }
    }
}

/// HN-style orange header bar.
#[component]
fn header() -> Element {
    let bar_style = format!(
        "width: 100%; padding: 16px; \
         display: flex; flex-direction: row; \
         background-color: {HEADER_BG};"
    );
    let title_style = format!("font-size: 18px; font-weight: 700; color: {HEADER_FG};");
    render! {
        view(style: bar_style) {
            text(style: title_style, value: "Hacker News")
        }
    }
}

/// Status banner shown while the resource is loading or has
/// errored. The Resource's own state drives the message.
#[component]
fn status_banner(message: &'static str) -> Element {
    let style = format!(
        "width: 100%; padding: 16px; \
         display: flex; flex-direction: row; \
         font-size: 13px; color: {TEXT_SECONDARY};"
    );

    render! {
        view(style: style) {
            text(value: message)
        }
    }
}

/// Root of the app. Kicks off the fetch via `resource()` and
/// switches between the loading/error banner and the loaded list
/// with `Show` + a manual state-match on the resource.
#[component]
pub fn hn_reader() -> Element {
    // `resource(...)` spawns a worker thread, marshals the result
    // back to the main thread, and exposes Loading / Ready(Vec<Story>) /
    // Error(String) through a Copy handle. The old hand-rolled
    // `signal + thread::spawn + run_on_main_thread + LoadState`
    // boilerplate collapses into this one call.
    let stories = resource(fetch_stories);

    let list_style: &'static str =
        "flex-grow: 1; flex-shrink: 1; width: 100%; display: flex; flex-direction: column;";

    // The body view is the only direct child of `page`. We match
    // hello-world's pattern: explicit `width: 100%` + flex-grow +
    // `display: flex` + `flex-direction: column`. `flex: 1`
    // shorthand seems unreliable in Lynx; the long-form
    // `flex-grow: 1; flex-shrink: 1` is what `examples/hello-world`
    // uses and works there.
    let body_style = "width: 100%; flex-grow: 1; flex-shrink: 1; \
                      display: flex; flex-direction: column;"
        .to_string();

    render! {
        view(style: body_style) {
            header()
            Show(
                when: move || stories.get().is_some(),
                fallback: move || render! {
                    status_banner(message: if stories.error().is_some() {
                        "Failed to load — check your connection"
                    } else {
                        "Loading top stories…"
                    })
                },
            ) {
                list(
                    each: move || stories.get().unwrap_or_default(),
                    key: |s: &Story| s.object_id.clone(),
                    children: |s: Story| render! {
                        list_item { story_row(story: s) }
                    },
                    style: list_style,
                )
            }
        }
    }
}

// ---- Main app ---------------------------------------------------------------

#[whisker::main]
fn app() -> Element {
    // The `page` element needs explicit dimensions + background +
    // flex direction. Without `width: 100vw; height: 100vh;` it
    // collapses to 0 px on iOS and the whole screen renders as the
    // host's clear color (black).
    let page_style = format!(
        "width: 100vw; height: 100vh; background-color: {BG}; \
         display: flex; flex-direction: column;"
    );
    render! {
        page(style: page_style) {
            hn_reader()
        }
    }
}
