use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use whisker_protocol::WhiskerValue;

use crate::module_api::{WebEventEmitter, WebNativeEvent, WebViewDefinition};

mod ranges;

pub(crate) struct TextElement {
    element: web_sys::Element,
    events: WebEventEmitter,
    selection_listener: Option<Closure<dyn FnMut(web_sys::Event)>>,
    copy_listener: Option<Closure<dyn FnMut(web_sys::Event)>>,
    action_listener: Option<Closure<dyn FnMut(web_sys::Event)>>,
}

pub(crate) fn definition() -> WebViewDefinition<TextElement> {
    WebViewDefinition::new(
        "whisker.ui/Text",
        |document, events| {
            let mut text = TextElement {
                element: document.create_element("div")?,
                events,
                selection_listener: None,
                copy_listener: None,
                action_listener: None,
            };
            text.install_actions()?;
            Ok(text)
        },
        |text| text.element.clone(),
    )
    .plain_text()
    .prop(
        "selectable",
        |text, value| text.set_selectable(matches!(value, WhiskerValue::Bool(true))),
        |text| text.set_selectable(false),
    )
    .event("selectionchange")
    .event("textqueryresult")
    .event("textactivate")
    .command("setSelection", |text, arguments| {
        text.set_selection(arguments, false)
    })
    .command("clearSelection", |text, arguments| {
        text.set_selection(arguments, true)
    })
    .command("textQuery", TextElement::query)
}

impl TextElement {
    fn install_actions(&mut self) -> Result<(), JsValue> {
        let element = self.element.clone();
        let events = self.events.clone();
        let listener = Closure::wrap(Box::new(move |event: web_sys::Event| {
            let activate = if let Some(key) = event.dyn_ref::<web_sys::KeyboardEvent>() {
                key.key() == "Enter" || key.key() == " "
            } else {
                event
                    .dyn_ref::<web_sys::MouseEvent>()
                    .is_some_and(|event| event.detail() == 0)
            };
            if !activate {
                return;
            }
            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
            let Some(action) = target.closest("[data-whisker-text-action]").ok().flatten() else {
                return;
            };
            if !element.contains(Some(&action)) {
                return;
            }
            let Some(content) = action
                .closest("[data-whisker-text-revision]")
                .ok()
                .flatten()
            else {
                return;
            };
            let (Some(span), Some(revision)) = (
                action
                    .get_attribute("data-whisker-text-action")
                    .and_then(|value| value.parse::<i64>().ok()),
                content
                    .get_attribute("data-whisker-text-revision")
                    .and_then(|value| value.parse::<i64>().ok()),
            ) else {
                return;
            };
            event.prevent_default();
            event.stop_propagation();
            events.emit(WebNativeEvent {
                event: "textactivate".into(),
                detail: WhiskerValue::map([
                    ("span", WhiskerValue::Int(span)),
                    ("revision", WhiskerValue::Int(revision)),
                ]),
            });
        }) as Box<dyn FnMut(web_sys::Event)>);
        for name in ["click", "keydown"] {
            self.element
                .add_event_listener_with_callback(name, listener.as_ref().unchecked_ref())?;
        }
        self.action_listener = Some(listener);
        Ok(())
    }

    fn content(&self) -> Result<web_sys::Element, JsValue> {
        self.element
            .query_selector("[data-whisker-text]")?
            .ok_or_else(|| JsValue::from_str("text is not presented"))
    }

    fn set_selectable(&mut self, selectable: bool) -> Result<(), JsValue> {
        crate::set_style(
            &self.element,
            "user-select",
            if selectable { "text" } else { "none" },
        )
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let document = self
            .element
            .owner_document()
            .ok_or_else(|| JsValue::from_str("missing text document"))?;
        if let Some(listener) = self.selection_listener.take() {
            document.remove_event_listener_with_callback(
                "selectionchange",
                listener.as_ref().unchecked_ref(),
            )?;
        }
        if let Some(listener) = self.copy_listener.take() {
            self.element
                .remove_event_listener_with_callback("copy", listener.as_ref().unchecked_ref())?;
        }
        if selectable {
            let element = self.element.clone();
            let copy = Closure::wrap(Box::new(move |event: web_sys::Event| {
                let Some(content) = element.query_selector("[data-whisker-text]").ok().flatten()
                else {
                    return;
                };
                let Ok(Some(_)) = ranges::selection(&content) else {
                    return;
                };
                let Some(clipboard) = event
                    .dyn_ref::<web_sys::ClipboardEvent>()
                    .and_then(|event| event.clipboard_data())
                else {
                    return;
                };
                if let Ok(text) = ranges::selected_text(&content) {
                    if clipboard.set_data("text/plain", &text).is_ok() {
                        event.prevent_default();
                    }
                }
            }) as Box<dyn FnMut(_)>);
            self.element
                .add_event_listener_with_callback("copy", copy.as_ref().unchecked_ref())?;
            self.copy_listener = Some(copy);
            let element = self.element.clone();
            let events = self.events.clone();
            let mut had_selection = false;
            let listener = Closure::wrap(Box::new(move |_: web_sys::Event| {
                let Some(content) = element.query_selector("[data-whisker-text]").ok().flatten()
                else {
                    return;
                };
                let Ok(selection) = ranges::selection(&content) else {
                    return;
                };
                let (start, end, direction) = if let Some((start, end, direction)) = selection {
                    had_selection = true;
                    (i64::from(start), i64::from(end), direction)
                } else if had_selection {
                    had_selection = false;
                    (-1, -1, "forward")
                } else {
                    return;
                };
                events.emit(WebNativeEvent {
                    event: "selectionchange".into(),
                    detail: WhiskerValue::map([
                        (
                            "revision",
                            WhiskerValue::Int(
                                content
                                    .get_attribute("data-whisker-text-revision")
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(0),
                            ),
                        ),
                        ("start", WhiskerValue::Int(start)),
                        ("end", WhiskerValue::Int(end)),
                        ("direction", WhiskerValue::String(direction.into())),
                    ]),
                });
            }) as Box<dyn FnMut(web_sys::Event)>);
            document.add_event_listener_with_callback(
                "selectionchange",
                listener.as_ref().unchecked_ref(),
            )?;
            self.selection_listener = Some(listener);
        }
        Ok(())
    }

    fn set_selection(&mut self, arguments: &WhiskerValue, clear: bool) -> Result<(), JsValue> {
        let content = self.content()?;
        if !revision_matches(&content, arguments) {
            return Ok(());
        }
        if clear {
            ranges::clear(&content)
        } else {
            let start = integer(arguments, "start")
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| JsValue::from_str("invalid selection start"))?;
            let end = integer(arguments, "end")
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| JsValue::from_str("invalid selection end"))?;
            ranges::select(&content, start, end)
        }
    }

    fn query(&mut self, arguments: &WhiskerValue) -> Result<(), JsValue> {
        let id = integer(arguments, "id").ok_or_else(|| JsValue::from_str("missing query ID"))?;
        let mut fields = std::collections::BTreeMap::from([("id".into(), WhiskerValue::Int(id))]);
        let result = (|| {
            let content = self.content().map_err(|_| "stale-layout")?;
            if !revision_matches(&content, arguments) {
                return Err("stale-layout");
            }
            match string(arguments, "kind") {
                Some("selectedText") => {
                    let text = ranges::selected_text(&content).map_err(|_| "invalid-range")?;
                    fields.insert("text".into(), WhiskerValue::String(text));
                }
                Some("boundingRects") => {
                    let start = integer(arguments, "start")
                        .and_then(|value| u32::try_from(value).ok())
                        .ok_or("invalid-range")?;
                    let end = integer(arguments, "end")
                        .and_then(|value| u32::try_from(value).ok())
                        .ok_or("invalid-range")?;
                    let rects = ranges::bounding_rects(&content, start, end)
                        .map_err(|_| "invalid-range")?;
                    fields.insert("rects".into(), WhiskerValue::Array(rects));
                }
                _ => return Err("unknown-query"),
            }
            Ok(())
        })();
        if let Err(error) = result {
            fields.insert("error".into(), WhiskerValue::String(error.into()));
        }
        self.events.emit(WebNativeEvent {
            event: "textqueryresult".into(),
            detail: WhiskerValue::Map(fields),
        });
        Ok(())
    }
}

impl Drop for TextElement {
    fn drop(&mut self) {
        if let Some(listener) = &self.action_listener {
            for name in ["click", "keydown"] {
                let _ = self
                    .element
                    .remove_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
            }
        }
        if let Some(listener) = &self.copy_listener {
            let _ = self
                .element
                .remove_event_listener_with_callback("copy", listener.as_ref().unchecked_ref());
        }
        if let Some(listener) = self.selection_listener.take() {
            if let Some(document) = self.element.owner_document() {
                let _ = document.remove_event_listener_with_callback(
                    "selectionchange",
                    listener.as_ref().unchecked_ref(),
                );
            }
        }
    }
}

fn revision_matches(content: &web_sys::Element, arguments: &WhiskerValue) -> bool {
    integer(arguments, "revision").is_some_and(|revision| {
        content
            .get_attribute("data-whisker-text-revision")
            .and_then(|value| value.parse::<i64>().ok())
            == Some(revision)
    })
}
fn integer(value: &WhiskerValue, key: &str) -> Option<i64> {
    let WhiskerValue::Map(fields) = value else {
        return None;
    };
    match fields.get(key)? {
        WhiskerValue::Int(value) => Some(*value),
        _ => None,
    }
}
fn string<'a>(value: &'a WhiskerValue, key: &str) -> Option<&'a str> {
    let WhiskerValue::Map(fields) = value else {
        return None;
    };
    match fields.get(key)? {
        WhiskerValue::String(value) => Some(value),
        _ => None,
    }
}

pub(crate) fn before_update(
    content: &web_sys::Element,
    text: &str,
) -> Result<Option<(u32, u32, &'static str)>, crate::WebError> {
    let selection = ranges::selection(content)
        .map_err(|error| crate::js_error("read paragraph selection", error))?;
    if selection.is_none() {
        return Ok(None);
    }
    if ranges::source(content) == text {
        return Ok(selection);
    }
    ranges::clear(content)
        .map_err(|error| crate::js_error("clear stale paragraph selection", error))?;
    Ok(None)
}

pub(crate) fn after_update(
    content: &web_sys::Element,
    selection: Option<(u32, u32, &str)>,
) -> Result<(), crate::WebError> {
    if let Some((start, end, direction)) = selection {
        ranges::select_direction(content, start, end, direction == "backward")
            .map_err(|error| crate::js_error("restore paragraph selection", error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
