//! `whisker-web-browser` — mirrors Expo's `expo-web-browser`:
//! `open_auth_session_async` (`ASWebAuthenticationSession` /
//! Chrome Custom Tabs) for in-app OAuth, plus a plain
//! `open_browser_async` (`SFSafariViewController` / Custom Tabs).
//!
//! ## OAuth redirect
//!
//! `open_auth_session_async`'s `redirect_url` must use a scheme the
//! app owns. iOS needs no registration — `ASWebAuthenticationSession`
//! intercepts the redirect before the OS routes it. Android needs
//! `Config::url_scheme` set in `whisker.rs` (`app.url_scheme("giga")`
//! → register `giga://...` redirect URIs), which wires a matching
//! `MainActivity` intent-filter — see `whisker-config`'s docs.
//!
//! ## Usage
//!
//! ```ignore
//! use whisker_web_browser::{open_auth_session_async, AuthSessionResult};
//!
//! let result = open_auth_session_async(&authorize_url, "giga://oauth2redirect", false).await;
//! match result {
//!     AuthSessionResult::Success { url } => { /* extract `code` from `url` */ }
//!     AuthSessionResult::Cancel => { /* user backed out */ }
//!     AuthSessionResult::Error(msg) => { /* show msg */ }
//! }
//! ```
//!
//! ## Native source
//!
//! - iOS: `packages/whisker-web-browser/ios/Sources/WhiskerWebBrowser/WebBrowserModule.swift`
//! - Android: `packages/whisker-web-browser/android/src/main/kotlin/rs/whisker/modules/webbrowser/WebBrowserModule.kt`

use std::sync::{Arc, Mutex};

use whisker::WhiskerValue;
use whisker::module;

/// Outcome of [`open_auth_session_async`].
#[derive(Debug, Clone, PartialEq)]
pub enum AuthSessionResult {
    /// The OAuth redirect fired; `url` is the full redirect URL
    /// (query string included — parse `code`/`error` out of it).
    Success { url: String },
    /// The user backed out of the auth flow without completing it.
    Cancel,
    /// [`dismiss_auth_session`] was called mid-flow.
    Dismiss,
    /// The native module is unavailable, or a platform-side failure.
    Error(String),
}

/// Outcome of [`open_browser_async`].
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserResult {
    Dismiss,
    Cancel,
    Error(String),
}

/// Open `url` in an in-app auth browser (`ASWebAuthenticationSession`
/// / Chrome Custom Tabs) and wait for it to complete. `redirect_url`
/// is the OAuth redirect URI — see the module docs for scheme setup.
/// `prefers_ephemeral` (iOS only) requests a private-browsing-style
/// session that shares no cookies/state with the user's regular
/// browsing (ignored on Android).
pub async fn open_auth_session_async(
    url: &str,
    redirect_url: &str,
    prefers_ephemeral: bool,
) -> AuthSessionResult {
    let module = module!("WebBrowser");
    let (tx, rx) = futures_channel::oneshot::channel::<AuthSessionResult>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    // Held as a plain local for the whole function body (across the
    // `.await` below) so it only drops once this fn returns — NOT
    // inside the callback that fires it. Dropping a `ModuleSubscription`
    // frees the very `Box<EventCallback>` the bridge is currently
    // executing; doing that synchronously from within its own callback
    // is a use-after-free (crashed with SIGSEGV during on-device testing).
    let subscription = module.on_event("authSessionCompleted", move |payload| {
        // Resolve in its own statement, not `if let Some(x) =
        // mutex.lock().unwrap().take() { ... }` — that form holds the
        // `MutexGuard` alive through the whole body, still locked
        // during `send()` below. `send()` can reentrantly drop
        // `subscription` (freeing `tx`), and the guard then unlocks
        // freed memory — confirmed SIGSEGV on device.
        let sender = tx.lock().unwrap().take();
        if let Some(sender) = sender {
            let _ = sender.send(decode_auth_result(payload));
        }
    });
    if let Some(err) = subscription.error() {
        return AuthSessionResult::Error(err.to_string());
    }

    let _ = module.invoke(
        "openAuthSession",
        vec![
            WhiskerValue::String(url.to_string()),
            WhiskerValue::String(redirect_url.to_string()),
            WhiskerValue::Bool(prefers_ephemeral),
        ],
    );

    let result = rx
        .await
        .unwrap_or_else(|_| AuthSessionResult::Error("native module unavailable".to_string()));
    drop(subscription);
    result
}

/// Cancel an in-flight [`open_auth_session_async`] call — it resolves
/// with [`AuthSessionResult::Dismiss`].
pub fn dismiss_auth_session() {
    let _ = module!("WebBrowser").invoke("dismissAuthSession", vec![]);
}

/// Open `url` in a plain in-app browser (`SFSafariViewController` /
/// Chrome Custom Tabs — no cookie sharing with `open_auth_session_async`
/// on iOS) and wait for it to close.
pub async fn open_browser_async(url: &str) -> BrowserResult {
    let module = module!("WebBrowser");
    let (tx, rx) = futures_channel::oneshot::channel::<BrowserResult>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let subscription = module.on_event("browserClosed", move |payload| {
        let sender = tx.lock().unwrap().take();
        if let Some(sender) = sender {
            let _ = sender.send(decode_browser_result(payload));
        }
    });
    if let Some(err) = subscription.error() {
        return BrowserResult::Error(err.to_string());
    }

    let _ = module.invoke("openBrowser", vec![WhiskerValue::String(url.to_string())]);

    let result = rx
        .await
        .unwrap_or_else(|_| BrowserResult::Error("native module unavailable".to_string()));
    drop(subscription);
    result
}

/// Close a browser opened via [`open_browser_async`] — it resolves
/// with [`BrowserResult::Dismiss`].
pub fn dismiss_browser() {
    let _ = module!("WebBrowser").invoke("dismissBrowser", vec![]);
}

fn decode_auth_result(payload: WhiskerValue) -> AuthSessionResult {
    let WhiskerValue::Map(fields) = payload else {
        return AuthSessionResult::Error("malformed authSessionCompleted payload".to_string());
    };
    let kind = match fields.get("type") {
        Some(WhiskerValue::String(s)) => s.as_str(),
        _ => "",
    };
    match kind {
        "success" => {
            let url = match fields.get("url") {
                Some(WhiskerValue::String(s)) => s.clone(),
                _ => String::new(),
            };
            AuthSessionResult::Success { url }
        }
        "cancel" => AuthSessionResult::Cancel,
        "dismiss" => AuthSessionResult::Dismiss,
        _ => {
            let message = match fields.get("message") {
                Some(WhiskerValue::String(s)) => s.clone(),
                _ => "unknown error".to_string(),
            };
            AuthSessionResult::Error(message)
        }
    }
}

fn decode_browser_result(payload: WhiskerValue) -> BrowserResult {
    let WhiskerValue::Map(fields) = payload else {
        return BrowserResult::Dismiss;
    };
    match fields.get("type") {
        Some(WhiskerValue::String(s)) if s == "cancel" => BrowserResult::Cancel,
        _ => BrowserResult::Dismiss,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_auth_success_extracts_url() {
        let v = WhiskerValue::map([
            ("type", WhiskerValue::String("success".into())),
            (
                "url",
                WhiskerValue::String("giga://oauth2redirect?code=abc".into()),
            ),
        ]);
        assert_eq!(
            decode_auth_result(v),
            AuthSessionResult::Success {
                url: "giga://oauth2redirect?code=abc".to_string()
            }
        );
    }

    #[test]
    fn decode_auth_cancel() {
        let v = WhiskerValue::map([("type", WhiskerValue::String("cancel".into()))]);
        assert_eq!(decode_auth_result(v), AuthSessionResult::Cancel);
    }

    #[test]
    fn decode_auth_error_carries_message() {
        let v = WhiskerValue::map([
            ("type", WhiskerValue::String("error".into())),
            ("message", WhiskerValue::String("boom".into())),
        ]);
        assert_eq!(
            decode_auth_result(v),
            AuthSessionResult::Error("boom".to_string())
        );
    }

    #[test]
    fn decode_browser_cancel_vs_dismiss() {
        let cancel = WhiskerValue::map([("type", WhiskerValue::String("cancel".into()))]);
        assert_eq!(decode_browser_result(cancel), BrowserResult::Cancel);
        let dismiss = WhiskerValue::map([("type", WhiskerValue::String("dismiss".into()))]);
        assert_eq!(decode_browser_result(dismiss), BrowserResult::Dismiss);
    }
}
