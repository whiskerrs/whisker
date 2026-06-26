//! Smoke test for whisker's `tokio` feature.
//!
//! The app runs one [`resource()`] fetcher that combines the two things
//! the feature is meant to unlock:
//!
//! 1. **I/O on tokio's reactor** — `reqwest::get(...).await`. reqwest
//!    registers its socket with `Handle::current()`; without an entered
//!    runtime this panics. With the `tokio` feature, whisker-driver has
//!    entered a multi-thread runtime on the TASM thread at bootstrap, so
//!    it resolves and the request progresses (reactor on tokio's bg
//!    threads, future polled on the TASM thread → no `Send` needed).
//! 2. **CPU offload** — `tokio::task::spawn_blocking(...)`. Moves work to
//!    tokio's blocking pool so the UI thread never stalls, then `.await`s
//!    the result back.
//!
//! If the screen shows `OK · N bytes`, the whole chain worked: runtime
//! entered → reqwest serviced → spawn_blocking joined → result marshalled
//! back to the TASM thread and rendered. The `resource` state is read
//! reactively in a `move ||` text binding, so Loading → Ready/Error
//! transitions repaint on their own.

use whisker::css::{AlignItems, FlexDirection, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;

/// A public, auth-free AppView endpoint — no setup or credentials needed,
/// which keeps this smoke test runnable anywhere.
const ENDPOINT: &str =
    "https://public.api.bsky.app/xrpc/app.bsky.feed.getAuthorFeed?actor=bsky.app&limit=1";

#[whisker::main]
pub fn app() -> Element {
    let body_len = resource(fetch_body_len);

    // Derive the status line reactively from the resource state. `computed`
    // yields a `ReadSignal<String>` (which `text`'s `value:` accepts), and
    // re-runs on every Loading → Ready/Error transition so the text repaints
    // without any manual wiring.
    let status = computed(move || {
        if let Some(n) = body_len.get() {
            format!("OK · {n} bytes")
        } else if let Some(err) = body_len.error() {
            format!("error: {err}")
        } else {
            "loading…".to_string()
        }
    });

    render! {
        view(style: css!(
            flex_grow: 1.0,
            background_color: Color::hex(0x101012),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: px(24),
        )) {
            text(
                style: css!(
                    color: Color::hex(0xF5F5F7),
                    font_size: px(18),
                    font_weight: FontWeight::Bold,
                    margin_bottom: px(12),
                ),
                value: "tokio feature smoke",
            )
            text(
                style: css!(
                    color: Color::hex(0x9AA0AA),
                    font_size: px(14),
                    margin_bottom: px(20),
                ),
                value: "reqwest (I/O) + spawn_blocking (CPU offload)",
            )
            text(
                style: css!(
                    color: Color::hex(0x4ADE80),
                    font_size: px(16),
                    font_weight: FontWeight::Bold,
                ),
                value: status,
            )
        }
    }
}

/// `resource()` fetcher: fetch the endpoint over HTTPS, then count the
/// bytes off the TASM thread. Errors are stringified at this boundary to
/// match `resource`'s `Result<T, String>` contract.
async fn fetch_body_len() -> Result<usize, String> {
    // ① reqwest drives the request on tokio's reactor.
    let body = reqwest::get(ENDPOINT)
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    // ② Hand the (trivial here, but stand-in for real work) computation to
    //    tokio's blocking pool so the UI thread isn't the one doing it.
    tokio::task::spawn_blocking(move || body.len())
        .await
        .map_err(|e| e.to_string())
}
