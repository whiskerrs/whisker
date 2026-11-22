//! Desktop system-browser Host for `whisker-web-browser`.

use whisker::WhiskerModule;
use whisker::platform_module::WhiskerValue;
use whisker::runtime::module::RustModuleDefinition as ModuleDefinition;

const MODULE_NAME: &str = "whisker-web-browser:WebBrowser";

struct WebBrowserModule;

#[whisker::WhiskerModule]
impl WhiskerModule for WebBrowserModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::default()
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
    fn definition_names_the_module() {
        assert_eq!(
            __whisker_module_definition().module_name(),
            Some(MODULE_NAME)
        );
    }
}
