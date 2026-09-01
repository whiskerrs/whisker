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
