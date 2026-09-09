use web_sys::wasm_bindgen::JsCast;
use whisker_protocol::AvailableSpace;
use whisker_web::{WhiskerMeasureRequest, WhiskerMeasuredSize, WhiskerValue};

mod data {
    use serde::{Deserialize, Serialize};

    pub const VERSION: u16 = 1;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct InputMeasureData {
        pub scale_factor: f32,
        pub text: String,
        pub multiline: bool,
        pub font_family: Option<String>,
        pub font_size: f32,
        pub font_weight: u16,
        pub italic: bool,
        pub line_height: Option<f32>,
        pub letter_spacing: f32,
    }
}

pub(super) fn measure(request: &WhiskerMeasureRequest) -> Option<WhiskerMeasuredSize> {
    if request.payload_version != data::VERSION {
        return None;
    }
    let WhiskerValue::Bytes(bytes) = &request.payload else {
        return None;
    };
    let input: data::InputMeasureData = serde_json::from_slice(bytes).ok()?;
    let document = web_sys::window()?.document()?;
    let probe = document
        .create_element("div")
        .ok()?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()?;
    let result = measure_in(&document, &probe, request, &input);
    probe.remove();
    result.ok()
}

fn measure_in(
    document: &web_sys::Document,
    probe: &web_sys::HtmlElement,
    request: &WhiskerMeasureRequest,
    input: &data::InputMeasureData,
) -> Result<WhiskerMeasuredSize, web_sys::wasm_bindgen::JsValue> {
    let style = probe.style();
    for (name, value) in [
        ("position", "absolute"),
        ("visibility", "hidden"),
        ("left", "-100000px"),
        ("top", "0"),
        ("padding", "0"),
        ("border", "0"),
        (
            "white-space",
            if input.multiline { "pre-wrap" } else { "pre" },
        ),
        ("overflow-wrap", "anywhere"),
    ] {
        style.set_property(name, value)?;
    }
    style.set_property(
        "font-family",
        &input
            .font_family
            .as_ref()
            .map(|name| format!("{name:?}"))
            .unwrap_or_else(|| "system-ui".into()),
    )?;
    style.set_property("font-size", &format!("{}px", input.font_size))?;
    style.set_property("font-weight", &input.font_weight.to_string())?;
    style.set_property("font-style", if input.italic { "italic" } else { "normal" })?;
    style.set_property(
        "line-height",
        &input
            .line_height
            .map(|value| format!("{value}px"))
            .unwrap_or_else(|| "normal".into()),
    )?;
    style.set_property("letter-spacing", &format!("{}px", input.letter_spacing))?;
    let text = if input.text.is_empty() || input.text.ends_with('\n') {
        format!("{}\u{200b}", input.text)
    } else {
        input.text.clone()
    };
    probe.set_text_content(Some(&text));
    let width = match request.known_dimensions[0] {
        Some(width) => format!("{}px", width.max(0.0)),
        None => match request.available_space[0] {
            AvailableSpace::Definite(width) => {
                style.set_property("max-width", &format!("{}px", width.max(0.0)))?;
                "max-content".into()
            }
            AvailableSpace::MinContent => "min-content".into(),
            AvailableSpace::MaxContent => "max-content".into(),
        },
    };
    style.set_property("width", &width)?;
    document
        .body()
        .ok_or_else(|| web_sys::wasm_bindgen::JsValue::from_str("missing document body"))?
        .append_child(probe)?;
    let rect = probe.get_bounding_client_rect();
    let height = if input.multiline {
        let control = document
            .create_element("textarea")?
            .dyn_into::<web_sys::HtmlTextAreaElement>()?;
        control.set_rows(1);
        control.set_value(&input.text);
        let css = control.style();
        for (name, value) in [
            ("font", "inherit"),
            ("letter-spacing", "inherit"),
            ("line-height", "inherit"),
            ("padding", "0"),
            ("border", "0"),
            ("margin", "0"),
            ("height", "0"),
            ("overflow", "hidden"),
            ("resize", "none"),
            ("box-sizing", "content-box"),
        ] {
            css.set_property(name, value)?;
        }
        css.set_property("width", &format!("{}px", rect.width()))?;
        probe.set_text_content(None);
        probe.append_child(&control)?;
        control.scroll_height() as f32
    } else {
        rect.height() as f32
    };
    Ok(WhiskerMeasuredSize::new(
        request.known_dimensions[0].unwrap_or(rect.width() as f32),
        request.known_dimensions[1].unwrap_or(height),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    fn request(text: &str, width: f32) -> WhiskerMeasureRequest {
        WhiskerMeasureRequest {
            known_dimensions: [Some(width), None],
            available_space: [AvailableSpace::Definite(width), AvailableSpace::MaxContent],
            payload_version: data::VERSION,
            payload: WhiskerValue::Bytes(
                serde_json::to_vec(&data::InputMeasureData {
                    scale_factor: 1.0,
                    text: text.into(),
                    multiline: true,
                    font_family: None,
                    font_size: 16.0,
                    font_weight: 400,
                    italic: false,
                    line_height: Some(24.0),
                    letter_spacing: 0.0,
                })
                .unwrap(),
            ),
        }
    }

    #[wasm_bindgen_test]
    fn intrinsic_height_tracks_lines_wrapping_and_removes_probes() {
        let body = web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .body()
            .unwrap();
        let children = body.child_element_count();
        let empty = measure(&request("", 200.0)).unwrap();
        let one = measure(&request("Hello", 200.0)).unwrap();
        let two = measure(&request("Hello\n", 200.0)).unwrap();
        let text = "The quick brown fox jumps over the lazy dog";
        let wide = measure(&request(text, 400.0)).unwrap();
        let narrow = measure(&request(text, 80.0)).unwrap();
        assert_eq!(empty.height, 24.0);
        assert_eq!(one.height, empty.height);
        assert_eq!(two.height, 48.0);
        assert!(narrow.height > wide.height);
        let mut fixed = request(text, 80.0);
        fixed.known_dimensions[1] = Some(32.0);
        assert_eq!(measure(&fixed).unwrap().height, 32.0);
        fixed.payload_version = 2;
        assert!(measure(&fixed).is_none());
        assert_eq!(body.child_element_count(), children);
    }
}
