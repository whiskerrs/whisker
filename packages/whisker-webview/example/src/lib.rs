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

use whisker::css::{FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_webview::{WebView, WebViewRef};

const BG: u32 = 0x101012;
const CARD_BG: u32 = 0x1c1c1f;
const FG: u32 = 0xf0f0f3;
const MUTED: u32 = 0x9a9aa2;
const ACCENT: u32 = 0xff5577;

const INLINE_HTML: &str = "<!doctype html><html><head><meta name='viewport' content='width=device-width, initial-scale=1'></head><body style='font-family: -apple-system, sans-serif; padding: 16px;'><h2>Inline HTML page</h2><button onclick=\"window.whisker.postMessage('hi from page')\">Post message to Rust</button></body></html>";

#[whisker::main]
pub fn app() -> Element {
    let page_style = Css::new()
        .background_color(Color::hex(BG))
        .flex_grow(1.0)
        .flex_shrink(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .padding_top(px(56))
        .padding_left(px(20))
        .padding_right(px(20));
    let header_style = Css::new()
        .color(Color::hex(FG))
        .font_size(px(22))
        .font_weight(FontWeight::Numeric(700))
        .margin_bottom(px(20));

    render! {
        View(style: page_style) {
            Text(style: header_style, value: "whisker-webview demo")

            UrlDemo()
            InlineHtmlDemo()
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

    let msg_style = Css::new()
        .color(Color::hex(MUTED))
        .font_size(px(14))
        .margin_top(px(6));

    render! {
        View(style: section_style()) {
            Text(style: label_style(), value: "URL load + JS bridge")

            WebView(
                url: url,
                webview_ref: webview.clone(),
                on_message: {
                    move |msg: String| last_message.set(msg)
                },
                on_load: move |u: String| log_load(&u),
                style: webview_style(),
            )

            View(style: Css::new().display_flex().flex_direction(FlexDirection::Row).margin_top(px(10))) {
                Text(style: button_style(), value: "Reload", on_tap: {
                    let w = webview.clone();
                    move |_| w.reload()
                })
                Text(style: button_style(), value: "Back", on_tap: {
                    let w = webview.clone();
                    move |_| w.go_back()
                })
                Text(style: button_style(), value: "Forward", on_tap: {
                    let w = webview.clone();
                    move |_| w.go_forward()
                })
                Text(style: button_style(), value: "Ping JS", on_tap: {
                    let w = webview.clone();
                    move |_| {
                        w.post_message("ping from rust");
                        w.evaluate_javascript(
                            "window.whisker.postMessage('pong: ' + document.title)",
                        );
                    }
                })
            }

            Text(
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
    let msg_style = Css::new()
        .color(Color::hex(MUTED))
        .font_size(px(14))
        .margin_top(px(6));

    render! {
        View(style: section_style()) {
            Text(style: label_style(), value: "Inline HTML")
            WebView(
                html: INLINE_HTML.to_string(),
                on_message: move |msg: String| last_message.set(msg),
                style: webview_style(),
            )
            Text(
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

fn section_style() -> Css {
    Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .margin_bottom(px(24))
}

fn label_style() -> Css {
    Css::new()
        .color(Color::hex(FG))
        .font_size(px(13))
        .font_weight(FontWeight::Numeric(600))
        .margin_bottom(px(8))
}

fn webview_style() -> Css {
    Css::new()
        .background_color(Color::hex(CARD_BG))
        .height(px(280))
        .border_radius(px(10))
}

fn button_style() -> Css {
    Css::new()
        .background_color(Color::hex(ACCENT))
        .color(Color::hex(FG))
        .font_size(px(14))
        .font_weight(FontWeight::Numeric(600))
        .padding_top(px(8))
        .padding_bottom(px(8))
        .padding_left(px(14))
        .padding_right(px(14))
        .margin_right(px(8))
        .border_radius(px(8))
}
