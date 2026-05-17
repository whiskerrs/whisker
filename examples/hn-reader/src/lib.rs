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
use whisker::runtime::view::ElementHandle;

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

/// Three-state machine for the fetch lifecycle.
#[derive(Debug, Clone)]
pub enum LoadState {
    Loading,
    Loaded(Vec<Story>),
    Error(String),
}

impl LoadState {
    fn stories(&self) -> Vec<Story> {
        match self {
            LoadState::Loaded(s) => s.clone(),
            _ => Vec::new(),
        }
    }
}

// ---- Palette (HN orange + cream, like the real site) -----------------------

const BG: &str = "#f6f6ef";
const HEADER_BG: &str = "#ff6600";
const HEADER_FG: &str = "#ffffff";
const TEXT_PRIMARY: &str = "#000000";
const TEXT_SECONDARY: &str = "#828282";

// ---- Fetch ------------------------------------------------------------------

/// Blocking HTTPS GET + JSON parse. Returns the stories on success
/// or a human-readable error string on failure. Runs on a worker
/// thread; never call from the main thread (it'd block the render
/// loop for the duration of the network round-trip).
fn fetch_blocking() -> Result<Vec<Story>, String> {
    let url = "https://hn.algolia.com/api/v1/search?tags=front_page&hitsPerPage=30";
    let body = ureq::get(url)
        .call()
        .map_err(|e| format!("network: {e}"))?
        .into_string()
        .map_err(|e| format!("read: {e}"))?;
    let parsed: HnResponse =
        serde_json::from_str(&body).map_err(|e| format!("parse: {e}"))?;
    Ok(parsed.hits)
}

// ---- Components -------------------------------------------------------------

/// Single story row. `story` is cloned in because `For` hands each
/// item to its `children` closure by value, and `#[component]`
/// requires the prop to be `Clone`.
///
/// All String interpolations are at the top level of the body —
/// keeping them out of nested `Show {}` blocks avoids the
/// move-into-Fn-closure issue (the macro's per-`{expr}` effect is
/// `move ||`, which would consume captures of a wrapping `Fn`
/// closure like `Show`'s `children`).
#[component]
fn story_row(story: Story) -> ElementHandle {
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
    let meta = format!("{points} points by {author} · {comments} comments");

    let row_style = format!(
        "flex-direction: row; padding: 12px 16px; background: {BG};"
    );
    let body_style = "flex-direction: column; flex: 1; gap: 4px;".to_string();
    let title_style = format!(
        "font-size: 15px; color: {TEXT_PRIMARY}; font-weight: 500;"
    );
    let sub_style = format!("font-size: 12px; color: {TEXT_SECONDARY};");
    let sub_style_2 = sub_style.clone();

    render! {
        view {
            style: row_style,
            view {
                style: body_style,
                text {
                    style: title_style,
                    {title.clone()}
                }
                text {
                    style: sub_style,
                    {domain.clone()}
                }
                text {
                    style: sub_style_2,
                    {meta.clone()}
                }
            }
        }
    }
}

/// HN-style orange header bar.
fn header() -> ElementHandle {
    let bar_style = format!(
        "background: {HEADER_BG}; padding: 16px; flex-direction: row;"
    );
    let title_style = format!(
        "font-size: 18px; font-weight: 700; color: {HEADER_FG};"
    );
    render! {
        view {
            style: bar_style,
            text {
                style: title_style,
                "Hacker News"
            }
        }
    }
}

/// Status banner — uses a closure that returns `&'static str` so
/// we can read the signal reactively without owned-String capture
/// issues. Empty string when there's nothing to say (loaded state).
#[component]
fn status_banner(state: RwSignal<LoadState>) -> ElementHandle {
    let status_text = move || match state.get() {
        LoadState::Loading => "Loading top stories…",
        LoadState::Loaded(_) => "",
        LoadState::Error(_) => "Failed to load — check your connection",
    };

    let style = format!(
        "padding: 16px; flex-direction: row; \
         font-size: 13px; color: {TEXT_SECONDARY};"
    );

    render! {
        view {
            style: style,
            text { {status_text()} }
        }
    }
}

/// Root component. Owns the load-state signal and kicks off the
/// background fetch on mount.
#[component]
pub fn hn_reader() -> ElementHandle {
    let state = RwSignal::new(LoadState::Loading);

    on_mount(move || {
        // Worker thread: do the blocking HTTPS call.
        std::thread::spawn(move || {
            let result = fetch_blocking();

            // Hop back to the main thread before touching the signal.
            // Inside this closure we're on the TASM thread, so signal
            // writes + dependent effect scheduling all behave the
            // same as if we were inside an event handler or a
            // `#[component]` body.
            run_on_main_thread(move || match result {
                Ok(stories) => state.set(LoadState::Loaded(stories)),
                Err(msg) => state.set(LoadState::Error(msg)),
            });
        });
    });

    let page_style = format!(
        "flex-direction: column; flex: 1; background: {BG};"
    );

    render! {
        view {
            style: page_style,
            {header()}
            {status_banner(state)}
            For {
                each: move || state.get().stories(),
                key: |s: &Story| s.object_id.clone(),
                children: |s: Story| story_row(s),
            }
        }
    }
}

// ---- Main app ---------------------------------------------------------------

#[whisker::main]
fn app() -> ElementHandle {
    render! {
        page {
            style: "flex-direction: column;",
            {hn_reader()}
        }
    }
}
