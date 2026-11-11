//! Desktop system-browser Host for `whisker-web-browser`.

use whisker_desktop::{ModuleDefinition, WhiskerModule, WhiskerValue};

const MODULE_NAME: &str = "whisker-web-browser:WebBrowser";

struct WebBrowserModule;

#[whisker_desktop::WhiskerModule]
impl WhiskerModule for WebBrowserModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .function("openAuthSession", |_args, emitter| {
                emitter.emit(
                    "authSessionCompleted",
                    WhiskerValue::map([
                        ("type", WhiskerValue::String("error".into())),
                        (
                            "message",
                            WhiskerValue::String(
                                "Desktop auth redirect interception is not supported".into(),
                            ),
                        ),
                    ]),
                );
                WhiskerValue::Null
            })
            .function("dismissAuthSession", |_args, _| WhiskerValue::Null)
            .function("openBrowser", |args, emitter| {
                let [WhiskerValue::String(url)] = args else {
                    return WhiskerValue::Error(
                        "WebBrowser.openBrowser requires one URL string".into(),
                    );
                };
                if !is_browser_url(url) {
                    return WhiskerValue::Error("browser URL must use HTTP or HTTPS".into());
                }
                if let Err(error) = open::that_detached(url) {
                    return WhiskerValue::Error(error.to_string());
                }
                // The system browser is a separate process, so its close state is
                // not observable. Resolve once the handoff succeeds.
                emitter.emit(
                    "browserClosed",
                    WhiskerValue::map([("type", WhiskerValue::String("dismiss".into()))]),
                );
                WhiskerValue::Null
            })
            .function("dismissBrowser", |_args, _| WhiskerValue::Null)
            .event("authSessionCompleted")
            .event("browserClosed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_is_service_only() {
        assert!(__whisker_module_definition().factories().is_empty());
    }
}

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
