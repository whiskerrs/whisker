//! Web popup Host for `whisker-web-browser`.

use std::cell::RefCell;

use whisker::runtime::module::ModuleEventEmitter;
use whisker_web::wasm_bindgen::closure::Closure;
use whisker_web::{ModuleDefinition, WhiskerModule, WhiskerValue, web_sys};

const MODULE_NAME: &str = "whisker-web-browser:WebBrowser";

thread_local! {
    static SESSION: RefCell<Option<PopupSession>> = const { RefCell::new(None) };
}

enum SessionKind {
    Auth { redirect_url: String },
    Browser,
}

struct PopupSession {
    popup: web_sys::Window,
    interval: i32,
    _poll: Closure<dyn FnMut()>,
}

struct WebBrowserModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for WebBrowserModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .function("openAuthSession", |args, emitter| {
                let [
                    WhiskerValue::String(url),
                    WhiskerValue::String(redirect_url),
                    WhiskerValue::Bool(_),
                ] = args
                else {
                    return WhiskerValue::Error(
                        "WebBrowser.openAuthSession requires URL, redirect URL, and ephemeral flag"
                            .into(),
                    );
                };
                open_popup(
                    url,
                    SessionKind::Auth {
                        redirect_url: redirect_url.clone(),
                    },
                    emitter,
                )
            })
            .function("dismissAuthSession", |args, emitter| {
                if !args.is_empty() {
                    return WhiskerValue::Error(
                        "WebBrowser.dismissAuthSession does not accept arguments".into(),
                    );
                }
                finish("authSessionCompleted", "dismiss", None, emitter);
                WhiskerValue::Null
            })
            .function("openBrowser", |args, emitter| {
                let [WhiskerValue::String(url)] = args else {
                    return WhiskerValue::Error(
                        "WebBrowser.openBrowser requires one URL string".into(),
                    );
                };
                open_popup(url, SessionKind::Browser, emitter)
            })
            .function("dismissBrowser", |args, emitter| {
                if !args.is_empty() {
                    return WhiskerValue::Error(
                        "WebBrowser.dismissBrowser does not accept arguments".into(),
                    );
                }
                finish("browserClosed", "dismiss", None, emitter);
                WhiskerValue::Null
            })
            .event("authSessionCompleted")
            .event("browserClosed")
    }
}

fn open_popup(url: &str, kind: SessionKind, emitter: &ModuleEventEmitter) -> WhiskerValue {
    if !is_browser_url(url) {
        return WhiskerValue::Error("browser URL must use HTTP or HTTPS".into());
    }
    close_current();
    let Some(window) = web_sys::window() else {
        return WhiskerValue::Error("browser Window is unavailable".into());
    };
    let popup = match window.open_with_url_and_target(url, "_blank") {
        Ok(Some(popup)) => popup,
        Ok(None) => return WhiskerValue::Error("browser blocked the popup".into()),
        Err(error) => return WhiskerValue::Error(format!("open popup failed: {error:?}")),
    };
    let polled_popup = popup.clone();
    let event_emitter = emitter.clone();
    let poll = Closure::<dyn FnMut()>::new(move || {
        if polled_popup.closed().unwrap_or(true) {
            match &kind {
                SessionKind::Auth { .. } => {
                    emit_result(&event_emitter, "authSessionCompleted", "cancel", None)
                }
                SessionKind::Browser => {
                    emit_result(&event_emitter, "browserClosed", "dismiss", None)
                }
            }
            clear_later();
            return;
        }
        if let SessionKind::Auth { redirect_url } = &kind
            && let Ok(location) = polled_popup.location().href()
            && location.starts_with(redirect_url)
        {
            emit_result(
                &event_emitter,
                "authSessionCompleted",
                "success",
                Some(location),
            );
            let _ = polled_popup.close();
            clear_later();
        }
    });
    let interval = match window
        .set_interval_with_callback_and_timeout_and_arguments_0(poll.as_ref().unchecked_ref(), 100)
    {
        Ok(interval) => interval,
        Err(error) => return WhiskerValue::Error(format!("start popup observer: {error:?}")),
    };
    SESSION.with(|slot| {
        *slot.borrow_mut() = Some(PopupSession {
            popup,
            interval,
            _poll: poll,
        });
    });
    WhiskerValue::Null
}

fn finish(event: &str, kind: &str, url: Option<String>, emitter: &ModuleEventEmitter) {
    close_current();
    emit_result(emitter, event, kind, url);
}

fn close_current() {
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow_mut().take() {
            if let Some(window) = web_sys::window() {
                window.clear_interval_with_handle(session.interval);
            }
            let _ = session.popup.close();
        }
    });
}

fn clear_later() {
    let callback = Closure::once(close_current);
    if let Some(window) = web_sys::window() {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            0,
        );
        callback.forget();
    }
}

fn emit_result(emitter: &ModuleEventEmitter, event: &str, kind: &str, url: Option<String>) {
    let mut fields = vec![("type", WhiskerValue::String(kind.to_owned()))];
    if let Some(url) = url {
        fields.push(("url", WhiskerValue::String(url)));
    }
    emitter.emit(event, WhiskerValue::map(fields));
}

use whisker_web::wasm_bindgen::JsCast;

fn is_browser_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once(':') else {
        return false;
    };
    (scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
        && rest.starts_with("//")
        && rest[2..]
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|host| !host.is_empty())
        && !url.chars().any(|character| {
            character.is_control() || character.is_whitespace() || character == '\\'
        })
}

#[cfg(test)]
mod url_policy_tests {
    use super::is_browser_url;
    #[test]
    fn rejects_executable_and_local_schemes() {
        for value in [
            "javascript:alert(1)",
            "file:///tmp/key",
            "data:text/html,hello",
            "intent://example",
            "custom://example",
            "https:",
            "//example",
            " https://example",
            "https://example\n",
        ] {
            assert!(!is_browser_url(value), "{value}");
        }
        assert!(is_browser_url("https://example.com/path?q=1"));
        assert!(is_browser_url("HTTP://localhost:8080/"));
    }
}
