//! `whisker-webview` — native web-view component.
//!
//! **API shape — 2 (Component + ref-bound handle).** A native UI
//! element ([`WebView`]) backed by `WKWebView` on iOS and
//! `android.webkit.WebView` on Android, with a reactive `url` / inline
//! `html` content prop, a single JavaScript bridge channel, declarative
//! origin-whitelist navigation control, and a typed imperative handle
//! ([`WebViewRef`]) bound on mount via `ref:` for `reload` / `goBack` /
//! `goForward` / `stopLoading` / `postMessage` / `evaluateJavaScript`.
//!
//! The Lynx tag is `whisker-webview:WebView` (the crate name is
//! auto-prepended by `#[whisker::module_component]`).
//!
//! ## Usage
//!
//! ### Load a URL (controlled)
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_webview::WebView;
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     let url = RwSignal::new("https://example.com".to_string());
//!     render! {
//!         view(style: "flex-direction: column; flex-grow: 1;") {
//!             WebView(url: url, style: "flex-grow: 1;")
//!             // `url.set("https://other.com")` navigates the view.
//!         }
//!     }
//! }
//! ```
//!
//! `url` is a **controlled load prop**: writing `url.set(...)` tells the
//! native view to load that address. It is one-way DOWN — internal
//! navigations (link clicks, redirects, form posts) are *not* written
//! back into the signal. Observe those via [`on_navigation`](WebViewProps).
//!
//! ### Inline HTML
//!
//! ```ignore
//! let html = "<h1>Hi</h1>".to_string();
//! render! { WebView(html: html, style: "flex-grow: 1;") }
//! ```
//!
//! ### JS bridge round-trip
//!
//! ```ignore
//! let webview = WebViewRef::new();
//! render! {
//!     WebView(
//!         url: url,
//!         webview_ref: webview.clone(),
//!         // JS → Rust: the page calls window.whisker.postMessage("…").
//!         on_message: move |msg: String| log::info!("from page: {msg}"),
//!     )
//! }
//! // Rust → JS: deliver a message the page listens for, or run script.
//! webview.post_message("hello from rust");
//! webview.evaluate_javascript("document.title = 'set by rust'");
//! ```
//!
//! ### Imperative handle
//!
//! ```ignore
//! let webview = WebViewRef::new();
//! render! {
//!     view(style: "flex-direction: column;") {
//!         WebView(url: url, webview_ref: webview.clone(), style: "flex-grow: 1;")
//!         view(style: "flex-direction: row;") {
//!             text(value: "Reload", on_tap: {
//!                 let w = webview.clone();
//!                 move |_| w.reload()
//!             })
//!         }
//!     }
//! }
//! ```
//!
//! ## Props
//!
//! | Prop                | Type                       | Default                       | Description |
//! |---------------------|----------------------------|-------------------------------|-------------|
//! | `url`               | `RwSignal<String>`         | —                             | Controlled load address. `set()` navigates. One-way down. |
//! | `html`              | `Signal<String>`           | —                             | Inline HTML to render (ignored when `url` is set). |
//! | `user_agent`        | `Signal<String>`           | `""`                          | Custom User-Agent string. |
//! | `javascript_enabled`| `bool`                     | `true`                        | Allow JavaScript execution. |
//! | `scroll_enabled`    | `bool`                     | `true`                        | Allow the web content to scroll. |
//! | `origin_whitelist`  | `Vec<String>`              | `["https://*", "http://*"]`   | Navigation origins the native side permits (glob). |
//! | `on_message`        | `Fn(String)`               | —                             | JS → Rust message (`window.whisker.postMessage`). |
//! | `on_load_start`     | `Fn(String)`               | —                             | A navigation started; carries the URL. |
//! | `on_load`           | `Fn(String)`               | —                             | A navigation finished; carries the URL. |
//! | `on_navigation`     | `Fn(String)`               | —                             | An internal navigation was requested; carries the URL. |
//! | `on_progress`       | `Fn(f32)`                  | —                             | Load progress `0.0..=1.0`. |
//! | `on_error`          | `Fn(WebViewError)`         | —                             | Navigation failed (url / code / description). |
//! | `style`             | `Signal<String>`           | `""`                          | Standard Whisker CSS style string. |
//! | `webview_ref`       | [`WebViewRef`]             | —                             | Imperative handle (see [Methods](#methods)). |
//!
//! ## JS bridge
//!
//! A single message channel connects the page and Rust. The native side
//! injects a global **`window.whisker`** object at document start:
//!
//! ```js
//! // Injected by the native view:
//! window.whisker = {
//!     // Page → Rust. Delivered to the `on_message` callback as a String.
//!     postMessage: function (data) { /* native bridge */ },
//! };
//! ```
//!
//! - **Page → Rust:** the page calls `window.whisker.postMessage("…")`;
//!   the string surfaces in [`on_message`](WebViewProps).
//! - **Rust → Page:** [`WebViewRef::post_message`] delivers a string the
//!   page can consume (the native side dispatches a
//!   `window.whisker.onmessage`-style event / invokes the page's
//!   handler), and [`WebViewRef::evaluate_javascript`] runs arbitrary
//!   script in the page.
//!
//! ## Methods
//!
//! Hold a [`WebViewRef`], pass `webview_ref:` to the component, then:
//!
//! - [`WebViewRef::reload`] — reload the current page.
//! - [`WebViewRef::go_back`] / [`WebViewRef::go_forward`] — history nav.
//! - [`WebViewRef::stop_loading`] — abort the in-flight load.
//! - [`WebViewRef::post_message`] — push a string to the page.
//! - [`WebViewRef::evaluate_javascript`] — run script (fire-and-forget).
//!   To read a value back, `postMessage` it from the script and handle
//!   it in `on_message`.
//! - [`WebViewRef::can_go_back`] / [`WebViewRef::can_go_forward`] —
//!   async history-availability checks.
//!
//! ## Permissions
//!
//! The app must declare any network access its content needs. The
//! Whisker-generated app already ships the Android `INTERNET` permission
//! and Android cleartext (`usesCleartextTraffic`) plus an iOS ATS
//! exception for HTTP — sufficient for the default
//! `["https://*", "http://*"]` whitelist. For anything beyond that
//! (camera / microphone / geolocation prompts, custom ATS domain rules,
//! file access) the app must add the matching Android `<uses-permission>`
//! / iOS `Info.plist` entries itself.
//!
//! ## Native source
//!
//! Contributors: the matching platform module lives at
//!
//! - iOS: `packages/whisker-webview/ios/Sources/WhiskerWebView/WebViewModule.swift`
//!   (view: `WebViewView.swift`)
//! - Android: `packages/whisker-webview/android/src/main/kotlin/rs/whisker/elements/webview/WebViewModule.kt`
//!   (view: `WhiskerWebViewView.kt`)

use std::rc::Rc;

use whisker::platform_module::WhiskerValue;
use whisker::prelude::*;
use whisker::{ElementRef, RefError, Signal, Style};

// ---------------------------------------------------------------------------
// Event payloads
//
// Every native event delivers its body under `detail` (the Android event
// reporter wraps a custom event's params there, and the iOS bridge
// normalizes `LynxCustomEvent`'s `params` key to `detail`), so each struct
// reads one shape on both platforms. Every field is `#[serde(default)]` so
// a partial / mismatched body degrades to a default rather than dropping
// the handler call.
// ---------------------------------------------------------------------------

/// Payload of the JS-bridge message event (`message`).
///
/// The native view dispatches the page's `window.whisker.postMessage`
/// argument under `detail.data`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct MessageEvent {
    /// The event body's `detail` dict.
    #[serde(default)]
    pub detail: MessageDetail,
}

/// The `detail` of a [`MessageEvent`] — carries the page's message under
/// the `data` key.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct MessageDetail {
    /// The string the page passed to `window.whisker.postMessage`.
    #[serde(default)]
    pub data: String,
}

impl MessageEvent {
    /// The page's message string — shorthand for `self.detail.data`.
    pub fn data(&self) -> &str {
        &self.detail.data
    }

    /// Take ownership of the page's message string.
    pub fn into_data(self) -> String {
        self.detail.data
    }
}

/// Payload of a navigation event (`load-start` / `load` / `navigation`).
///
/// Carries the relevant URL under `detail.url`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct NavEvent {
    /// The event body's `detail` dict.
    #[serde(default)]
    pub detail: NavDetail,
}

/// The `detail` of a [`NavEvent`] — carries the URL under the `url` key.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct NavDetail {
    /// The URL involved in the navigation.
    #[serde(default)]
    pub url: String,
}

impl NavEvent {
    /// The navigation URL — shorthand for `self.detail.url`.
    pub fn url(&self) -> &str {
        &self.detail.url
    }

    /// Take ownership of the navigation URL.
    pub fn into_url(self) -> String {
        self.detail.url
    }
}

/// Payload of the error event (`error`).
///
/// Carries the failed URL, a numeric error code, and a human-readable
/// description, all under `detail`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct ErrorEvent {
    /// The event body's `detail` dict.
    #[serde(default)]
    pub detail: ErrorDetail,
}

/// The `detail` of an [`ErrorEvent`].
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct ErrorDetail {
    /// The URL that failed to load.
    #[serde(default)]
    pub url: String,
    /// Platform error code (native-specific).
    #[serde(default)]
    pub code: i64,
    /// Human-readable error description.
    #[serde(default)]
    pub description: String,
}

/// Payload of the progress event (`progress`).
///
/// Carries the load fraction (`0.0..=1.0`) under `detail.progress`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct ProgressEvent {
    /// The event body's `detail` dict.
    #[serde(default)]
    pub detail: ProgressDetail,
}

/// The `detail` of a [`ProgressEvent`] — the load fraction under the
/// `progress` key.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct ProgressDetail {
    /// Load progress in `0.0..=1.0`.
    #[serde(default)]
    pub progress: f64,
}

// ---------------------------------------------------------------------------
// Public error type
// ---------------------------------------------------------------------------

/// A navigation / load failure surfaced to [`on_error`](WebViewProps).
///
/// The ergonomic, app-facing shape of an [`ErrorEvent`]'s `detail`.
#[derive(Debug, Clone, Default)]
pub struct WebViewError {
    /// The URL that failed to load.
    pub url: String,
    /// Platform error code (native-specific).
    pub code: i64,
    /// Human-readable error description.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Callback newtype
// ---------------------------------------------------------------------------

/// A cloneable user callback for a [`WebView`] event prop.
///
/// Wraps `Rc<dyn Fn(A)>` so it's `Clone` (required: `#[component]`
/// re-clones every prop for the hot-reload remount path) and so a bare
/// closure coerces into it via `Into` at the call site
/// (`on_message: move |s| …`). `A` is `String` for the URL / message
/// events, `f32` for `on_progress`, and [`WebViewError`] for `on_error`.
#[derive(Clone)]
pub struct Callback<A>(Rc<dyn Fn(A) + 'static>);

impl<A> Callback<A> {
    /// Invoke the wrapped callback.
    pub fn call(&self, arg: A) {
        (self.0)(arg)
    }
}

impl<A, F: Fn(A) + 'static> From<F> for Callback<A> {
    fn from(f: F) -> Self {
        Callback(Rc::new(f))
    }
}

// ---------------------------------------------------------------------------
// Origin whitelist default
// ---------------------------------------------------------------------------

/// The default navigation origin whitelist: `["https://*", "http://*"]`.
///
/// Permits any HTTPS or HTTP origin. Tighten it per-app by passing an
/// explicit `origin_whitelist:` of glob patterns.
pub fn default_origin_whitelist() -> Vec<String> {
    vec!["https://*".to_string(), "http://*".to_string()]
}

// ---------------------------------------------------------------------------
// Imperative handle
// ---------------------------------------------------------------------------

/// Typed imperative handle for a mounted [`WebView`].
///
/// Wraps the framework-internal `ElementRef` bound on mount when passed
/// as the component's `webview_ref:` prop. Methods dispatch the matching
/// platform UI method through `ElementRef::invoke` / `invoke_typed`. The
/// fire-and-forget methods swallow "not mounted" / platform errors —
/// these are UI controls; use the async result methods (which return a
/// [`RefError`]) when you need to inspect failures.
///
/// `Clone` produces a shared handle (same backing arena slot), so the
/// same handle can drive multiple event closures.
#[derive(Clone)]
pub struct WebViewRef {
    r: ElementRef,
}

impl WebViewRef {
    /// Allocate a fresh, unbound handle. Pass it to the component's
    /// `webview_ref:` prop in `render!` to bind it on mount.
    pub fn new() -> Self {
        Self {
            r: ElementRef::new(),
        }
    }

    /// The underlying `ElementRef`. Framework-internal — the [`web_view`]
    /// component reads it to wire the element's `ref:`. App code holds
    /// the `WebViewRef` and calls the methods below.
    #[doc(hidden)]
    pub fn r(&self) -> ElementRef {
        self.r
    }

    /// Reload the current page. No-op if the element isn't mounted.
    pub fn reload(&self) {
        let _ = self.r.invoke("reload", WhiskerValue::Null);
    }

    /// Navigate back one entry in history. No-op if unmounted or there's
    /// no back entry.
    pub fn go_back(&self) {
        let _ = self.r.invoke("goBack", WhiskerValue::Null);
    }

    /// Navigate forward one entry in history. No-op if unmounted or
    /// there's no forward entry.
    pub fn go_forward(&self) {
        let _ = self.r.invoke("goForward", WhiskerValue::Null);
    }

    /// Abort the in-flight load. No-op if unmounted.
    pub fn stop_loading(&self) {
        let _ = self.r.invoke("stopLoading", WhiskerValue::Null);
    }

    /// Push a string to the page (Rust → JS). No-op if unmounted.
    ///
    /// The native side delivers `data` to the page's `window.whisker`
    /// message handler.
    pub fn post_message(&self, data: &str) {
        let _ = self.r.invoke(
            "postMessage",
            WhiskerValue::args([WhiskerValue::String(data.to_string())]),
        );
    }

    /// Run JavaScript in the page, fire-and-forget. No-op if unmounted.
    ///
    /// To get a value BACK from the page, have the script post it to the
    /// host and read it via [`on_message`](web_view): e.g.
    /// `evaluate_javascript("window.whisker.postMessage(document.title)")`
    /// and handle it in `on_message`. (A direct result-returning
    /// `evaluate_javascript_result` awaits an async-native-result module
    /// feature; the native WebView's JS eval is inherently asynchronous
    /// and the current sync `Function` dispatch can't carry its result.)
    pub fn evaluate_javascript(&self, script: &str) {
        let _ = self.r.invoke(
            "evaluateJavaScript",
            WhiskerValue::args([WhiskerValue::String(script.to_string())]),
        );
    }

    /// Async: can the view navigate back in history right now?
    pub async fn can_go_back(&self) -> Result<bool, RefError> {
        self.r
            .invoke_typed::<bool>("canGoBack", WhiskerValue::Null)
            .await
    }

    /// Async: can the view navigate forward in history right now?
    pub async fn can_go_forward(&self) -> Result<bool, RefError> {
        self.r
            .invoke_typed::<bool>("canGoForward", WhiskerValue::Null)
            .await
    }
}

impl Default for WebViewRef {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Inner native binding — the thin element.
//
// `url` / `html` / `user_agent` / `style` are reactive `Signal<String>`
// attrs (kebab-cased: `user-agent`). The bool props are passed as
// pre-stringified `Signal<String>` attrs ("true" / "false"); the origin
// whitelist is a JSON-array string (e.g. `["https://*","http://*"]`) so
// the native side parses one stable form. `on_*` props are typed events
// wired via `bind_typed`. Crate-internal — only the outer `web_view`
// component uses it; not part of the public doc surface.
// ---------------------------------------------------------------------------

#[doc(hidden)]
#[whisker::module_component("WebView")]
pub fn native_webview(
    url: Signal<String>,
    html: Signal<String>,
    user_agent: Signal<String>,
    javascript_enabled: Signal<String>,
    scroll_enabled: Signal<String>,
    origin_whitelist: Signal<String>,
    style: Style,
    on_message: MessageEvent,
    on_load_start: NavEvent,
    on_load: NavEvent,
    on_error: ErrorEvent,
    on_progress: ProgressEvent,
    on_navigation: NavEvent,
) {
}

// ---------------------------------------------------------------------------
// Public ergonomic component.
// ---------------------------------------------------------------------------

/// `whisker-webview:WebView` — a native web view with reactive content,
/// a JS bridge, and an imperative handle.
///
/// See the [crate docs](crate) for usage, the full prop table, the JS
/// bridge contract, and permission notes.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn web_view(
    /// Controlled load address. Writing `url.set(...)` navigates the
    /// view. One-way down — internal navigations are not written back.
    url: Option<RwSignal<String>>,
    /// Inline HTML to render (ignored when `url` is set).
    html: Option<Signal<String>>,
    /// Custom User-Agent string.
    user_agent: Option<Signal<String>>,
    /// Allow JavaScript execution.
    #[prop(default = true)]
    javascript_enabled: bool,
    /// Allow the web content to scroll.
    #[prop(default = true)]
    scroll_enabled: bool,
    /// Navigation origins the native side permits (glob patterns).
    #[prop(default = default_origin_whitelist())]
    origin_whitelist: Vec<String>,
    /// JS → Rust message (`window.whisker.postMessage`).
    on_message: Option<Callback<String>>,
    /// A navigation started; carries the URL.
    on_load_start: Option<Callback<String>>,
    /// A navigation finished; carries the URL.
    on_load: Option<Callback<String>>,
    /// An internal navigation was requested; carries the URL.
    on_navigation: Option<Callback<String>>,
    /// Load progress `0.0..=1.0`.
    on_progress: Option<Callback<f32>>,
    /// Navigation failed (url / code / description).
    on_error: Option<Callback<WebViewError>>,
    /// Standard Whisker style. Accepts a `Css` builder, a raw string,
    /// or a reactive signal of either — same as a built-in element's
    /// `style:`.
    style: Option<Style>,
    /// Imperative handle ([`WebViewRef`]).
    webview_ref: Option<WebViewRef>,
) -> Element {
    // ----- Content (reactive) -----------------------------------------
    //
    // Feed `url` down as a `Signal::Dynamic` so an external
    // `url.set(...)` re-applies natively (a controlled load prop). When
    // `url` is unset, the attr stays empty and the native side falls
    // back to `html`. `url` is `Option<RwSignal<String>>` — `RwSignal`
    // is `Copy`, so the whole `Option` is `Copy`.
    let url_prop: Signal<String> = Signal::Dynamic(computed(move || match url {
        Some(u) => u.get(),
        None => String::new(),
    }));

    // ----- Event wiring -----------------------------------------------
    let on_message_cb = {
        let on_message = on_message.clone();
        move |ev: MessageEvent| {
            if let Some(cb) = &on_message {
                cb.call(ev.into_data());
            }
        }
    };
    let on_load_start_cb = {
        let on_load_start = on_load_start.clone();
        move |ev: NavEvent| {
            if let Some(cb) = &on_load_start {
                cb.call(ev.into_url());
            }
        }
    };
    let on_load_cb = {
        let on_load = on_load.clone();
        move |ev: NavEvent| {
            if let Some(cb) = &on_load {
                cb.call(ev.into_url());
            }
        }
    };
    let on_navigation_cb = {
        let on_navigation = on_navigation.clone();
        move |ev: NavEvent| {
            if let Some(cb) = &on_navigation {
                cb.call(ev.into_url());
            }
        }
    };
    let on_progress_cb = {
        let on_progress = on_progress.clone();
        move |ev: ProgressEvent| {
            if let Some(cb) = &on_progress {
                cb.call(ev.detail.progress as f32);
            }
        }
    };
    let on_error_cb = {
        let on_error = on_error.clone();
        move |ev: ErrorEvent| {
            if let Some(cb) = &on_error {
                cb.call(WebViewError {
                    url: ev.detail.url,
                    code: ev.detail.code,
                    description: ev.detail.description,
                });
            }
        }
    };

    // ----- Pass-through attrs (None → sensible default) ----------------
    let html_prop: Signal<String> = html.clone().unwrap_or_default();
    let user_agent_prop: Signal<String> = user_agent.clone().unwrap_or_default();
    let style_prop: Style = style.clone().unwrap_or_default();

    let javascript_enabled_attr = bool_attr(javascript_enabled);
    let scroll_enabled_attr = bool_attr(scroll_enabled);
    let origin_whitelist_attr = origin_whitelist_json(&origin_whitelist);

    // ----- Imperative handle: forward its ElementRef as `ref:` ---------
    let element_ref = webview_ref.as_ref().map(|h| h.r());

    let mut builder = NativeWebview::builder()
        .url(url_prop)
        .html(html_prop)
        .user_agent(user_agent_prop)
        .javascript_enabled(javascript_enabled_attr)
        .scroll_enabled(scroll_enabled_attr)
        .origin_whitelist(origin_whitelist_attr)
        .style(style_prop)
        .on_message(on_message_cb)
        .on_load_start(on_load_start_cb)
        .on_load(on_load_cb)
        .on_navigation(on_navigation_cb)
        .on_progress(on_progress_cb)
        .on_error(on_error_cb);

    if let Some(r) = element_ref {
        builder = builder.with_ref(r);
    }

    NativeWebview(builder.build())
}

/// `true` / `false` wire string for a bool attr.
fn bool_attr(b: bool) -> String {
    if b { "true" } else { "false" }.to_string()
}

/// Serialize an origin whitelist to a JSON array string the native side
/// parses, e.g. `["https://*","http://*"]`. Hand-rolled (no `serde_json`
/// dep): each entry is escaped for `"` and `\`.
fn origin_whitelist_json(origins: &[String]) -> String {
    let mut s = String::from("[");
    for (i, o) in origins.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push('"');
        for c in o.chars() {
            match c {
                '"' => s.push_str("\\\""),
                '\\' => s.push_str("\\\\"),
                _ => s.push(c),
            }
        }
        s.push('"');
    }
    s.push(']');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_attr_strings() {
        assert_eq!(bool_attr(true), "true");
        assert_eq!(bool_attr(false), "false");
    }

    #[test]
    fn default_origin_whitelist_values() {
        assert_eq!(default_origin_whitelist(), vec!["https://*", "http://*"]);
    }

    #[test]
    fn origin_whitelist_json_default() {
        let json = origin_whitelist_json(&default_origin_whitelist());
        assert_eq!(json, r#"["https://*","http://*"]"#);
    }

    #[test]
    fn origin_whitelist_json_empty() {
        let empty: Vec<String> = Vec::new();
        assert_eq!(origin_whitelist_json(&empty), "[]");
    }

    #[test]
    fn origin_whitelist_json_escapes_quotes_and_backslashes() {
        let origins = vec![r#"https://a"b"#.to_string(), r"c\d".to_string()];
        assert_eq!(
            origin_whitelist_json(&origins),
            r#"["https://a\"b","c\\d"]"#
        );
    }

    #[test]
    fn message_event_deserializes_detail_data() {
        // Mirrors the native event body: { detail: { data: "<msg>" } }.
        let v = WhiskerValue::map([(
            "detail",
            WhiskerValue::map([("data", WhiskerValue::String("hi from page".into()))]),
        )]);
        let ev: MessageEvent = v.deserialize_into().expect("deserialize MessageEvent");
        assert_eq!(ev.data(), "hi from page");
        assert_eq!(ev.into_data(), "hi from page");
    }

    #[test]
    fn nav_event_deserializes_detail_url() {
        let v = WhiskerValue::map([(
            "detail",
            WhiskerValue::map([("url", WhiskerValue::String("https://example.com".into()))]),
        )]);
        let ev: NavEvent = v.deserialize_into().expect("deserialize NavEvent");
        assert_eq!(ev.url(), "https://example.com");
        assert_eq!(ev.into_url(), "https://example.com");
    }

    #[test]
    fn error_event_deserializes_detail_fields() {
        let v = WhiskerValue::map([(
            "detail",
            WhiskerValue::map([
                ("url", WhiskerValue::String("https://bad.example".into())),
                ("code", WhiskerValue::Int(-1009)),
                ("description", WhiskerValue::String("offline".into())),
            ]),
        )]);
        let ev: ErrorEvent = v.deserialize_into().expect("deserialize ErrorEvent");
        assert_eq!(ev.detail.url, "https://bad.example");
        assert_eq!(ev.detail.code, -1009);
        assert_eq!(ev.detail.description, "offline");
    }

    #[test]
    fn progress_event_deserializes_detail_progress() {
        let v = WhiskerValue::map([(
            "detail",
            WhiskerValue::map([("progress", WhiskerValue::Float(0.5))]),
        )]);
        let ev: ProgressEvent = v.deserialize_into().expect("deserialize ProgressEvent");
        assert_eq!(ev.detail.progress, 0.5);
    }

    #[test]
    fn empty_body_defaults() {
        // Events with no body (or a `detail`-less body) degrade to
        // defaults via the `#[serde(default)]` on `detail`.
        let empty: [(&str, WhiskerValue); 0] = [];
        let ev: NavEvent = WhiskerValue::map(empty)
            .deserialize_into()
            .expect("empty map defaults");
        assert_eq!(ev.url(), "");
    }

    #[test]
    fn event_defaults_are_empty() {
        assert_eq!(MessageEvent::default().data(), "");
        assert_eq!(NavEvent::default().url(), "");
        assert_eq!(ErrorEvent::default().detail.code, 0);
        assert_eq!(ProgressEvent::default().detail.progress, 0.0);
    }
}
