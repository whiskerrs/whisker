//! Browser Host implementation for `whisker-input`.

use std::rc::Rc;

use whisker_protocol::{
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextAlignment, PaintColor,
};
use whisker_web::wasm_bindgen::JsCast;
use whisker_web::wasm_bindgen::closure::Closure;
use whisker_web::{
    ModuleDefinition, WebEventEmitter, WebNativeEvent, WebViewDefinition, WhiskerModule,
    WhiskerTextStyle, WhiskerValue, wasm_bindgen, web_sys,
};

const MODULE_NAME: &str = "whisker-input:Input";

struct Listener {
    target: web_sys::EventTarget,
    name: &'static str,
    callback: Closure<dyn FnMut(web_sys::Event)>,
}

impl Listener {
    fn new(
        target: &web_sys::EventTarget,
        name: &'static str,
        callback: impl FnMut(web_sys::Event) + 'static,
    ) -> Result<Self, wasm_bindgen::JsValue> {
        let callback = Closure::new(callback);
        target.add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())?;
        Ok(Self {
            target: target.clone(),
            name,
            callback,
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.name, self.callback.as_ref().unchecked_ref());
    }
}

struct InputWebView {
    root: web_sys::HtmlElement,
    input: web_sys::HtmlInputElement,
    textarea: web_sys::HtmlTextAreaElement,
    emitter: WebEventEmitter,
    _listeners: Vec<Listener>,
    multiline: bool,
    secure: bool,
    editable: bool,
    value: String,
}

impl InputWebView {
    fn active_element(&self) -> web_sys::HtmlElement {
        if self.multiline && !self.secure {
            self.textarea.clone().unchecked_into()
        } else {
            self.input.clone().unchecked_into()
        }
    }

    fn set_value(&mut self, value: &str) {
        self.value.clear();
        self.value.push_str(value);
        if self.input.value() != value {
            self.input.set_value(value);
        }
        if self.textarea.value() != value {
            self.textarea.set_value(value);
        }
    }

    fn update_mode(&self) -> Result<(), wasm_bindgen::JsValue> {
        let use_textarea = self.multiline && !self.secure;
        self.input
            .style()
            .set_property("display", if use_textarea { "none" } else { "block" })?;
        self.textarea
            .style()
            .set_property("display", if use_textarea { "block" } else { "none" })?;
        self.input
            .set_type(if self.secure { "password" } else { "text" });
        Ok(())
    }

    fn focus(&self) -> Result<(), wasm_bindgen::JsValue> {
        self.active_element().focus()
    }

    fn blur(&self) -> Result<(), wasm_bindgen::JsValue> {
        self.active_element().blur()
    }

    fn apply_text_style(&self, style: &WhiskerTextStyle) -> Result<(), wasm_bindgen::JsValue> {
        for element in [
            &self.input.clone().unchecked_into::<web_sys::HtmlElement>(),
            &self
                .textarea
                .clone()
                .unchecked_into::<web_sys::HtmlElement>(),
        ] {
            let css = element.style();
            let families = style
                .style
                .font_families
                .iter()
                .map(|family| match family {
                    MeasureFontFamily::System => "system-ui".to_owned(),
                    MeasureFontFamily::Named(name) => format!("{name:?}"),
                })
                .collect::<Vec<_>>()
                .join(", ");
            css.set_property("font-family", &families)?;
            css.set_property("font-size", &format!("{}px", style.style.font_size))?;
            css.set_property("font-weight", &style.style.font_weight.to_string())?;
            css.set_property(
                "font-style",
                match style.style.font_style {
                    MeasureFontStyle::Normal => "normal",
                    MeasureFontStyle::Italic => "italic",
                    MeasureFontStyle::Oblique => "oblique",
                },
            )?;
            css.set_property(
                "line-height",
                &match style.style.line_height {
                    MeasureLineHeight::Normal => "normal".to_owned(),
                    MeasureLineHeight::LogicalPixels(value) => format!("{value}px"),
                },
            )?;
            css.set_property(
                "letter-spacing",
                &format!("{}px", style.style.letter_spacing),
            )?;
            css.set_property(
                "text-align",
                match style.alignment {
                    MeasureTextAlignment::Start => "start",
                    MeasureTextAlignment::End => "end",
                    MeasureTextAlignment::Left => "left",
                    MeasureTextAlignment::Right => "right",
                    MeasureTextAlignment::Center => "center",
                },
            )?;
            css.set_property("color", &css_color(&style.paint.foreground))?;
        }
        Ok(())
    }
}

struct InputModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for InputModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .view(input_definition())
    }
}

fn input_definition() -> WebViewDefinition<InputWebView> {
    WebViewDefinition::new(
        MODULE_NAME,
        |document, emitter| {
            let root = document
                .create_element("div")?
                .dyn_into::<web_sys::HtmlElement>()?;
            let input = document
                .create_element("input")?
                .dyn_into::<web_sys::HtmlInputElement>()?;
            let textarea = document
                .create_element("textarea")?
                .dyn_into::<web_sys::HtmlTextAreaElement>()?;
            input.set_class_name("whisker-input-control");
            textarea.set_class_name("whisker-input-control");
            configure_control(&input.clone().unchecked_into())?;
            configure_control(&textarea.clone().unchecked_into())?;
            textarea.style().set_property("resize", "none")?;
            textarea.style().set_property("display", "none")?;
            let style = document.create_element("style")?;
            style.set_text_content(Some(
                ".whisker-input-control::placeholder{color:var(--whisker-placeholder-color)}\
                 .whisker-input-control::selection{background:var(--whisker-selection-color)}",
            ));
            root.append_child(&style)?;
            root.append_child(&input)?;
            root.append_child(&textarea)?;

            let mut listeners = Vec::new();
            let input_value = {
                let input = input.clone();
                Rc::new(move || input.value()) as Rc<dyn Fn() -> String>
            };
            let textarea_value = {
                let textarea = textarea.clone();
                Rc::new(move || textarea.value()) as Rc<dyn Fn() -> String>
            };
            install_control_listeners(
                &mut listeners,
                input.as_ref(),
                &emitter,
                input_value,
                false,
            )?;
            install_control_listeners(
                &mut listeners,
                textarea.as_ref(),
                &emitter,
                textarea_value,
                true,
            )?;
            Ok(InputWebView {
                root,
                input,
                textarea,
                emitter,
                _listeners: listeners,
                multiline: false,
                secure: false,
                editable: true,
                value: String::new(),
            })
        },
        |view| view.root.clone().unchecked_into(),
    )
    .prop("value", string_prop(InputWebView::set_value), |view| {
        view.set_value("");
        Ok(())
    })
    .prop(
        "placeholder",
        |view, value| {
            let value = expect_string(value, "placeholder")?;
            view.input.set_placeholder(value);
            view.textarea.set_placeholder(value);
            Ok(())
        },
        |view| {
            view.input.set_placeholder("");
            view.textarea.set_placeholder("");
            Ok(())
        },
    )
    .prop(
        "placeholder-color",
        color_prop("--whisker-placeholder-color"),
        clear_css_var("--whisker-placeholder-color"),
    )
    .prop(
        "caret-color",
        color_style_prop("caret-color"),
        clear_control_style("caret-color"),
    )
    .prop(
        "selection-color",
        color_prop("--whisker-selection-color"),
        clear_css_var("--whisker-selection-color"),
    )
    .prop(
        "multiline",
        |view, value| {
            view.multiline = expect_bool(value, "multiline")?;
            view.update_mode()
        },
        |view| {
            view.multiline = false;
            view.update_mode()
        },
    )
    .prop(
        "lines",
        |view, value| {
            let rows = match expect_int(value, "lines")? {
                value if value <= 0 => 2,
                value => value.min(u32::MAX as i64) as u32,
            };
            view.textarea.set_rows(rows);
            Ok(())
        },
        |view| {
            view.textarea.set_rows(2);
            Ok(())
        },
    )
    .prop(
        "secure",
        |view, value| {
            view.secure = expect_bool(value, "secure")?;
            view.update_mode()
        },
        |view| {
            view.secure = false;
            view.update_mode()
        },
    )
    .prop(
        "editable",
        |view, value| set_editable(view, expect_bool(value, "editable")?),
        |view| set_editable(view, true),
    )
    .prop(
        "auto-focus",
        |view, value| {
            if expect_bool(value, "auto-focus")? {
                view.focus()?;
            }
            Ok(())
        },
        |_| Ok(()),
    )
    .prop(
        "max-length",
        |view, value| {
            let value = match expect_int(value, "max-length")? {
                value if value <= 0 => -1,
                value => value.min(i32::MAX as i64) as i32,
            };
            view.input.set_max_length(value);
            view.textarea.set_max_length(value);
            Ok(())
        },
        |view| {
            view.input.set_max_length(-1);
            view.textarea.set_max_length(-1);
            Ok(())
        },
    )
    .prop(
        "keyboard-type",
        input_mode_prop,
        clear_attribute("inputmode"),
    )
    .prop(
        "return-key",
        enter_key_hint_prop,
        clear_attribute("enterkeyhint"),
    )
    .prop(
        "auto-capitalize",
        attribute_prop("autocapitalize"),
        clear_attribute("autocapitalize"),
    )
    .prop(
        "autocorrect",
        autocorrect_prop,
        clear_attribute("autocorrect"),
    )
    .prop(
        "spell-check",
        bool_attribute_prop("spellcheck"),
        clear_attribute("spellcheck"),
    )
    .event("input")
    .event("change")
    .event("focus")
    .event("blur")
    .event("submit")
    .command("focus", |view, _| view.focus())
    .command("blur", |view, _| view.blur())
    .command("clear", |view, _| {
        view.set_value("");
        emit_value(&view.emitter, "input", "");
        Ok(())
    })
    .command("setValue", |view, arguments| {
        let WhiskerValue::Map(arguments) = arguments else {
            return Err(js_error("setValue arguments must be a map"));
        };
        let Some(WhiskerValue::String(value)) = arguments.get("value") else {
            return Err(js_error("setValue.value must be a string"));
        };
        view.set_value(value);
        Ok(())
    })
    .text_style(|view, style| view.apply_text_style(style))
}

fn configure_control(element: &web_sys::HtmlElement) -> Result<(), wasm_bindgen::JsValue> {
    let style = element.style();
    for (name, value) in [
        ("position", "absolute"),
        ("inset", "0"),
        ("width", "100%"),
        ("height", "100%"),
        ("box-sizing", "border-box"),
        ("border", "0"),
        ("outline", "0"),
        ("margin", "0"),
        ("padding", "0"),
        ("background", "transparent"),
    ] {
        style.set_property(name, value)?;
    }
    Ok(())
}

fn install_control_listeners(
    listeners: &mut Vec<Listener>,
    target: &web_sys::EventTarget,
    emitter: &WebEventEmitter,
    value: Rc<dyn Fn() -> String>,
    multiline: bool,
) -> Result<(), wasm_bindgen::JsValue> {
    for name in ["input", "change"] {
        let emitter = emitter.clone();
        let value = Rc::clone(&value);
        listeners.push(Listener::new(target, name, move |_| {
            emit_value(&emitter, name, &value());
        })?);
    }
    for name in ["focus", "blur"] {
        let emitter = emitter.clone();
        listeners.push(Listener::new(target, name, move |_| {
            emitter.emit(WebNativeEvent {
                event: name.into(),
                detail: WhiskerValue::Null,
            });
        })?);
    }
    if !multiline {
        let emitter = emitter.clone();
        let value = Rc::clone(&value);
        listeners.push(Listener::new(target, "keydown", move |event| {
            let Some(event) = event.dyn_ref::<web_sys::KeyboardEvent>() else {
                return;
            };
            if event.key() == "Enter" && !event.is_composing() {
                emit_value(&emitter, "submit", &value());
            }
        })?);
    }
    Ok(())
}

fn emit_value(emitter: &WebEventEmitter, event: &str, value: &str) {
    emitter.emit(WebNativeEvent {
        event: event.into(),
        detail: WhiskerValue::map([("value", WhiskerValue::String(value.to_owned()))]),
    });
}

fn string_prop(
    setter: fn(&mut InputWebView, &str),
) -> impl Fn(&mut InputWebView, &WhiskerValue) -> Result<(), wasm_bindgen::JsValue> {
    move |view, value| {
        setter(view, expect_string(value, "string property")?);
        Ok(())
    }
}

fn set_editable(view: &mut InputWebView, editable: bool) -> Result<(), wasm_bindgen::JsValue> {
    view.editable = editable;
    view.input.set_disabled(!editable);
    view.textarea.set_disabled(!editable);
    Ok(())
}

fn attribute_prop(
    name: &'static str,
) -> impl Fn(&mut InputWebView, &WhiskerValue) -> Result<(), wasm_bindgen::JsValue> {
    move |view, value| {
        let value = expect_string(value, name)?;
        view.input.set_attribute(name, value)?;
        view.textarea.set_attribute(name, value)
    }
}

fn input_mode_prop(
    view: &mut InputWebView,
    value: &WhiskerValue,
) -> Result<(), wasm_bindgen::JsValue> {
    let value = match expect_string(value, "keyboard-type")? {
        "number" => "numeric",
        "phone" => "tel",
        "default" => "text",
        value => value,
    };
    view.input.set_attribute("inputmode", value)?;
    view.textarea.set_attribute("inputmode", value)
}

fn enter_key_hint_prop(
    view: &mut InputWebView,
    value: &WhiskerValue,
) -> Result<(), wasm_bindgen::JsValue> {
    let value = expect_string(value, "return-key")?;
    if value == "default" {
        view.input.remove_attribute("enterkeyhint")?;
        view.textarea.remove_attribute("enterkeyhint")
    } else {
        view.input.set_attribute("enterkeyhint", value)?;
        view.textarea.set_attribute("enterkeyhint", value)
    }
}

fn bool_attribute_prop(
    name: &'static str,
) -> impl Fn(&mut InputWebView, &WhiskerValue) -> Result<(), wasm_bindgen::JsValue> {
    move |view, value| {
        let value = expect_bool(value, name)?.to_string();
        view.input.set_attribute(name, &value)?;
        view.textarea.set_attribute(name, &value)
    }
}

fn autocorrect_prop(
    view: &mut InputWebView,
    value: &WhiskerValue,
) -> Result<(), wasm_bindgen::JsValue> {
    let value = if expect_bool(value, "autocorrect")? {
        "on"
    } else {
        "off"
    };
    view.input.set_attribute("autocorrect", value)?;
    view.textarea.set_attribute("autocorrect", value)
}

fn clear_attribute(
    name: &'static str,
) -> impl Fn(&mut InputWebView) -> Result<(), wasm_bindgen::JsValue> {
    move |view| {
        view.input.remove_attribute(name)?;
        view.textarea.remove_attribute(name)
    }
}

fn color_prop(
    name: &'static str,
) -> impl Fn(&mut InputWebView, &WhiskerValue) -> Result<(), wasm_bindgen::JsValue> {
    move |view, value| {
        view.root
            .style()
            .set_property(name, expect_string(value, name)?)
    }
}

fn clear_css_var(
    name: &'static str,
) -> impl Fn(&mut InputWebView) -> Result<(), wasm_bindgen::JsValue> {
    move |view| view.root.style().remove_property(name).map(|_| ())
}

fn color_style_prop(
    name: &'static str,
) -> impl Fn(&mut InputWebView, &WhiskerValue) -> Result<(), wasm_bindgen::JsValue> {
    move |view, value| {
        let value = expect_string(value, name)?;
        view.input.style().set_property(name, value)?;
        view.textarea.style().set_property(name, value)
    }
}

fn clear_control_style(
    name: &'static str,
) -> impl Fn(&mut InputWebView) -> Result<(), wasm_bindgen::JsValue> {
    move |view| {
        view.input.style().remove_property(name)?;
        view.textarea.style().remove_property(name).map(|_| ())
    }
}

fn expect_string<'a>(
    value: &'a WhiskerValue,
    name: &str,
) -> Result<&'a str, wasm_bindgen::JsValue> {
    let WhiskerValue::String(value) = value else {
        return Err(js_error(&format!("Input {name} property must be a string")));
    };
    Ok(value)
}

fn expect_bool(value: &WhiskerValue, name: &str) -> Result<bool, wasm_bindgen::JsValue> {
    let WhiskerValue::Bool(value) = value else {
        return Err(js_error(&format!(
            "Input {name} property must be a boolean"
        )));
    };
    Ok(*value)
}

fn expect_int(value: &WhiskerValue, name: &str) -> Result<i64, wasm_bindgen::JsValue> {
    let WhiskerValue::Int(value) = value else {
        return Err(js_error(&format!(
            "Input {name} property must be an integer"
        )));
    };
    Ok(*value)
}

fn css_color(value: &PaintColor) -> String {
    match value {
        PaintColor::Named(value) => value.clone(),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => {
            format!("rgba({red}, {green}, {blue}, {alpha})")
        }
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => {
            format!("hsla({hue_degrees}, {saturation}%, {lightness}%, {alpha})")
        }
    }
}

fn js_error(message: &str) -> wasm_bindgen::JsValue {
    wasm_bindgen::JsValue::from_str(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_exports_input_factory() {
        let definition = InputModule::definition();
        assert_eq!(definition.factories().len(), 1);
    }
}
