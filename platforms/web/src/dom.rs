use std::error::Error;
use std::fmt;

use wasm_bindgen::JsCast;

/// Failure while creating or driving the browser Host.
#[derive(Clone, Debug)]
pub struct WebError(pub(crate) String);

impl fmt::Display for WebError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for WebError {}

pub(crate) fn set_style(
    element: &web_sys::Element,
    property: &str,
    value: &str,
) -> Result<(), WebError> {
    let style = if let Some(html) = element.dyn_ref::<web_sys::HtmlElement>() {
        html.style()
    } else if let Some(svg) = element.dyn_ref::<web_sys::SvgElement>() {
        svg.style()
    } else {
        return Err(WebError(
            "Whisker DOM node exposes no CSS style declaration".into(),
        ));
    };
    style
        .set_property(property, value)
        .map_err(|error| js_error(&format!("set CSS property {property}"), error))
}

pub(crate) fn px(value: f32) -> String {
    format!("{value}px")
}

pub(crate) fn js_error(context: &str, value: wasm_bindgen::JsValue) -> WebError {
    WebError(format!(
        "{context}: {}",
        value.as_string().unwrap_or_else(|| format!("{value:?}"))
    ))
}
