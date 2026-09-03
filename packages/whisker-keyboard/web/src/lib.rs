//! Web Host implementation for `whisker-keyboard`.

use whisker_web::wasm_bindgen::JsCast;
use whisker_web::{ModuleDefinition, WhiskerModule, WhiskerValue};

const MODULE_NAME: &str = "whisker-keyboard:Keyboard";
const CHANGED_EVENT: &str = "keyboardChanged";

struct KeyboardModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for KeyboardModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .function("dismiss", |args, _| {
                if !args.is_empty() {
                    return WhiskerValue::Error(
                        "Keyboard.dismiss does not accept arguments".into(),
                    );
                }
                dismiss_active_element()
                    .map(|()| WhiskerValue::Null)
                    .unwrap_or_else(WhiskerValue::Error)
            })
            .event(CHANGED_EVENT)
            .on_start_observing(CHANGED_EVENT, |emitter| {
                emitter.emit(
                    CHANGED_EVENT,
                    WhiskerValue::map([("height", WhiskerValue::Float(0.0))]),
                );
            })
    }
}

fn dismiss_active_element() -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "browser Window is unavailable".to_owned())?;
    let document = window
        .document()
        .ok_or_else(|| "browser Document is unavailable".to_owned())?;
    let Some(element) = document.active_element() else {
        return Ok(());
    };
    let Ok(element) = element.dyn_into::<web_sys::HtmlElement>() else {
        return Ok(());
    };
    element
        .blur()
        .map_err(|error| format!("failed to blur the active element: {error:?}"))
}
