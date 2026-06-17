//! `whisker-webview` example app.
//!
//! Exercises the headline usage modes end-to-end on a real device so a
//! `whisker run` round-trip verifies the native module wiring:
//!
//! * **URL load** — a [`WebView`] driven by a reactive `RwSignal<String>`,
//!   with a reload / back / forward button row over a [`WebViewRef`].
//! * **JS bridge** — `on_message` surfaces the page's
//!   `window.whisker.postMessage(...)` into a live `<text>`, and a button
//!   pushes back via `post_message` + `evaluate_javascript`.
//! * **Inline HTML** — a second [`WebView`] rendering `html:` with a
//!   `<button>` that posts a message back to Rust.

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_webview::{WebView, WebViewRef};

const BG: &str = "#101012";
const CARD_BG: &str = "#1c1c1f";
const FG: &str = "#f0f0f3";
const MUTED: &str = "#9a9aa2";
const ACCENT: &str = "#ff5577";

const INLINE_HTML: &str = "<!doctype html><html><head><meta name='viewport' content='width=device-width, initial-scale=1'></head><body style='font-family: -apple-system, sans-serif; padding: 16px;'><h2>Inline HTML page</h2><button onclick=\"window.whisker.postMessage('hi from page')\">Post message to Rust</button></body></html>";

#[whisker::main]
pub fn app() -> Element {
    let page_style = format!(
        "background-color: {BG}; flex-grow: 1; flex-shrink: 1; \
         display: flex; flex-direction: column; \
         padding-top: 56px; padding-left: 20px; padding-right: 20px;",
    );
    let header_style =
        format!("color: {FG}; font-size: 22px; font-weight: 700; margin-bottom: 20px;",);

    render! {
        view(style: page_style) {
            text(style: header_style, value: "whisker-webview demo")

            url_demo()
            inline_html_demo()
        }
    }
}

/// A URL-loading web view with a control row driving a `WebViewRef`,
/// plus a JS-bridge round-trip.
#[component]
fn url_demo() -> Element {
    let url = RwSignal::new("https://example.com".to_string());
    let last_message = RwSignal::new(String::from("(none)"));
    let webview = WebViewRef::new();

    let msg_style = format!("color: {MUTED}; font-size: 14px; margin-top: 6px;");

    render! {
        view(style: section_style()) {
            text(style: label_style(), value: "URL load + JS bridge")

            WebView(
                url: url,
                webview_ref: webview.clone(),
                on_message: {
                    move |msg: String| last_message.set(msg)
                },
                on_load: move |u: String| log_load(&u),
                style: webview_style(),
            )

            view(style: "display: flex; flex-direction: row; margin-top: 10px;") {
                text(style: button_style(), value: "Reload", on_tap: {
                    let w = webview.clone();
                    move |_| w.reload()
                })
                text(style: button_style(), value: "Back", on_tap: {
                    let w = webview.clone();
                    move |_| w.go_back()
                })
                text(style: button_style(), value: "Forward", on_tap: {
                    let w = webview.clone();
                    move |_| w.go_forward()
                })
                text(style: button_style(), value: "Ping JS", on_tap: {
                    let w = webview.clone();
                    move |_| {
                        w.post_message("ping from rust");
                        w.evaluate_javascript(
                            "window.whisker.postMessage('pong: ' + document.title)",
                        );
                    }
                })
            }

            text(
                style: msg_style,
                value: computed(move || format!("Last JS message: {}", last_message.get())),
            )
        }
    }
}

/// A second web view rendering inline HTML that posts back to Rust.
#[component]
fn inline_html_demo() -> Element {
    let last_message = RwSignal::new(String::from("(none)"));
    let msg_style = format!("color: {MUTED}; font-size: 14px; margin-top: 6px;");

    render! {
        view(style: section_style()) {
            text(style: label_style(), value: "Inline HTML")
            WebView(
                html: INLINE_HTML.to_string(),
                on_message: move |msg: String| last_message.set(msg),
                style: webview_style(),
            )
            text(
                style: msg_style,
                value: computed(move || format!("From inline page: {}", last_message.get())),
            )
        }
    }
}

fn log_load(url: &str) {
    let _ = url;
}

// ---- Shared styling --------------------------------------------------------

fn section_style() -> String {
    "display: flex; flex-direction: column; margin-bottom: 24px;".to_string()
}

fn label_style() -> String {
    format!("color: {FG}; font-size: 13px; font-weight: 600; margin-bottom: 8px;")
}

fn webview_style() -> String {
    format!(
        "background-color: {CARD_BG}; height: 280px; \
         border-radius: 10px;",
    )
}

fn button_style() -> String {
    format!(
        "background-color: {ACCENT}; color: {FG}; \
         font-size: 14px; font-weight: 600; \
         padding-top: 8px; padding-bottom: 8px; \
         padding-left: 14px; padding-right: 14px; \
         margin-right: 8px; border-radius: 8px;",
    )
}
