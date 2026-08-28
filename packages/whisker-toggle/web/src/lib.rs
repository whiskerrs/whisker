//! Web Host implementation for `whisker-toggle`.

use whisker_web::wasm_bindgen::JsCast;
use whisker_web::wasm_bindgen::closure::Closure;
use whisker_web::{
    ModuleDefinition, WebNativeEvent, WebViewDefinition, WhiskerModule, WhiskerValue, wasm_bindgen,
    web_sys,
};

struct ToggleWebView {
    input: web_sys::HtmlInputElement,
    change: Closure<dyn FnMut(web_sys::Event)>,
}

struct ToggleModule;

impl Drop for ToggleWebView {
    fn drop(&mut self) {
        let _ = self
            .input
            .remove_event_listener_with_callback("change", self.change.as_ref().unchecked_ref());
    }
}

/// Declares the Web implementation independently from the Rust schema.
#[WhiskerModule]
impl WhiskerModule for ToggleModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name("whisker-toggle:WhiskerToggle")
            .view(
                WebViewDefinition::new(
                    "whisker.toggle/Toggle",
                    |document, emitter| {
                        let input = document
                            .create_element("input")?
                            .dyn_into::<web_sys::HtmlInputElement>()?;
                        input.set_type("checkbox");
                        let event_input = input.clone();
                        let event_emitter = emitter.clone();
                        let change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                            event_emitter.emit(WebNativeEvent {
                                event: "change".into(),
                                detail: WhiskerValue::map([(
                                    "checked",
                                    WhiskerValue::Bool(event_input.checked()),
                                )]),
                            });
                        });
                        input.add_event_listener_with_callback(
                            "change",
                            change.as_ref().unchecked_ref(),
                        )?;
                        Ok(ToggleWebView { input, change })
                    },
                    |view| view.input.clone().unchecked_into(),
                )
                .prop(
                    "checked",
                    |view, value| {
                        let WhiskerValue::Bool(value) = value else {
                            return Err(wasm_bindgen::JsValue::from_str(
                                "Toggle checked property must be boolean",
                            ));
                        };
                        view.input.set_checked(*value);
                        Ok(())
                    },
                    |view| {
                        view.input.set_checked(false);
                        Ok(())
                    },
                )
                .prop(
                    "disabled",
                    |view, value| {
                        let WhiskerValue::Bool(value) = value else {
                            return Err(wasm_bindgen::JsValue::from_str(
                                "Toggle disabled property must be boolean",
                            ));
                        };
                        view.input.set_disabled(*value);
                        Ok(())
                    },
                    |view| {
                        view.input.set_disabled(false);
                        Ok(())
                    },
                )
                .event("change")
                .command("setChecked", |view, parameters| {
                    let WhiskerValue::Bool(checked) = parameters else {
                        return Err(wasm_bindgen::JsValue::from_str(
                            "Toggle setChecked parameters must be boolean",
                        ));
                    };
                    view.input.set_checked(*checked);
                    Ok(())
                }),
            )
    }
}
