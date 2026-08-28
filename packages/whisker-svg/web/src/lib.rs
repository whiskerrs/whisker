//! Web Host implementation for `whisker-svg`.

use base64::Engine;
use whisker_svg::{Color, Transform, Visitor, replay};
use whisker_web::{
    ModuleDefinition, WebViewDefinition, WhiskerModule, WhiskerValue, wasm_bindgen, web_sys,
};

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

#[derive(Debug)]
struct SvgWebView {
    element: web_sys::Element,
}

impl SvgWebView {
    fn set_display_list(&mut self, encoded: &str) -> Result<(), wasm_bindgen::JsValue> {
        if encoded.is_empty() {
            self.element.set_inner_html("");
            self.element.remove_attribute("viewBox")?;
            return Ok(());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| {
                wasm_bindgen::JsValue::from_str(&format!(
                    "whisker-svg display-list is not valid base64: {error}"
                ))
            })?;
        let rendered = render_display_list(&bytes).map_err(|error| {
            wasm_bindgen::JsValue::from_str(&format!(
                "whisker-svg display-list replay failed: {error:?}"
            ))
        })?;
        if let Some(view_box) = rendered.view_box {
            self.element.set_attribute("viewBox", &view_box)?;
        } else {
            self.element.remove_attribute("viewBox")?;
        }
        self.element.set_inner_html(&rendered.body);
        Ok(())
    }

    fn set_color(&mut self, color: &str) -> Result<(), wasm_bindgen::JsValue> {
        if color.trim().is_empty() {
            self.element.remove_attribute("color")
        } else {
            self.element.set_attribute("color", color)
        }
    }
}

struct SvgModule;

#[WhiskerModule]
impl WhiskerModule for SvgModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new().view(
            WebViewDefinition::new(
                "whisker-svg:Svg",
                |document, _emitter| {
                    let element = document.create_element_ns(Some(SVG_NAMESPACE), "svg")?;
                    element.set_attribute("preserveAspectRatio", "xMidYMid meet")?;
                    element.set_attribute("aria-hidden", "true")?;
                    element.set_attribute("focusable", "false")?;
                    Ok(SvgWebView { element })
                },
                |view| view.element.clone(),
            )
            .prop(
                "display-list",
                |view, value| {
                    let WhiskerValue::String(value) = value else {
                        return Err(wasm_bindgen::JsValue::from_str(
                            "Svg display-list property must be a string",
                        ));
                    };
                    view.set_display_list(value)
                },
                |view| view.set_display_list(""),
            )
            .prop(
                "color",
                |view, value| {
                    let WhiskerValue::String(value) = value else {
                        return Err(wasm_bindgen::JsValue::from_str(
                            "Svg color property must be a string",
                        ));
                    };
                    view.set_color(value)
                },
                |view| view.set_color(""),
            ),
        )
    }
}

#[derive(Clone, Copy)]
enum Paint {
    Literal(Color),
    Tint,
}

#[derive(Clone, Copy)]
struct State {
    transform: Transform,
    fill: Option<Paint>,
    stroke: Option<Paint>,
    stroke_width: f32,
    opacity: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            transform: Transform::IDENTITY,
            fill: Some(Paint::Literal(Color::BLACK)),
            stroke: None,
            stroke_width: 1.0,
            opacity: 1.0,
        }
    }
}

#[derive(Default)]
struct SvgMarkupVisitor {
    state: State,
    stack: Vec<State>,
    path: String,
    body: String,
    view_box: Option<String>,
}

struct RenderedSvg {
    view_box: Option<String>,
    body: String,
}

fn render_display_list(bytes: &[u8]) -> Result<RenderedSvg, whisker_svg::ReplayError> {
    let mut visitor = SvgMarkupVisitor::default();
    replay(bytes, &mut visitor)?;
    Ok(RenderedSvg {
        view_box: visitor.view_box,
        body: visitor.body,
    })
}

impl SvgMarkupVisitor {
    fn push_number(target: &mut String, value: f32) {
        use std::fmt::Write;
        let _ = write!(target, "{value}");
    }

    fn append_path(&mut self, fill: bool, stroke: bool) {
        use std::fmt::Write;
        if self.path.is_empty() {
            return;
        }
        self.body.push_str("<path d=\"");
        self.body.push_str(&self.path);
        self.body.push('"');
        if fill {
            write_paint(&mut self.body, "fill", self.state.fill);
        } else {
            self.body.push_str(" fill=\"none\"");
        }
        if stroke {
            write_paint(&mut self.body, "stroke", self.state.stroke);
            let _ = write!(
                self.body,
                " stroke-width=\"{}\" stroke-linecap=\"butt\" stroke-linejoin=\"miter\"",
                self.state.stroke_width
            );
        } else {
            self.body.push_str(" stroke=\"none\"");
        }
        let transform = self.state.transform;
        if transform != Transform::IDENTITY {
            let _ = write!(
                self.body,
                " transform=\"matrix({} {} {} {} {} {})\"",
                transform.a, transform.b, transform.c, transform.d, transform.tx, transform.ty,
            );
        }
        if self.state.opacity < 1.0 {
            let _ = write!(self.body, " opacity=\"{}\"", self.state.opacity);
        }
        self.body.push_str("/>");
    }
}

impl Visitor for SvgMarkupVisitor {
    fn save(&mut self) {
        self.stack.push(self.state);
    }

    fn restore(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
    }

    fn concat(&mut self, transform: &Transform) {
        self.state.transform = multiply(self.state.transform, *transform);
    }

    fn viewport(&mut self, min_x: f32, min_y: f32, width: f32, height: f32) {
        self.view_box = Some(format!("{min_x} {min_y} {width} {height}"));
    }

    fn fill_color(&mut self, color: Color) {
        self.state.fill = Some(Paint::Literal(color));
    }

    fn stroke_color(&mut self, color: Color) {
        self.state.stroke = Some(Paint::Literal(color));
    }

    fn stroke_width(&mut self, width: f32) {
        self.state.stroke_width = width;
    }

    fn opacity(&mut self, alpha: f32) {
        self.state.opacity = alpha.clamp(0.0, 1.0);
    }

    fn fill_tint(&mut self) {
        self.state.fill = Some(Paint::Tint);
    }

    fn stroke_tint(&mut self) {
        self.state.stroke = Some(Paint::Tint);
    }

    fn path_begin(&mut self) {
        self.path.clear();
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.path.push('M');
        Self::push_number(&mut self.path, x);
        self.path.push(' ');
        Self::push_number(&mut self.path, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.push('L');
        Self::push_number(&mut self.path, x);
        self.path.push(' ');
        Self::push_number(&mut self.path, y);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.path.push('Q');
        for value in [cx, cy, x, y] {
            Self::push_number(&mut self.path, value);
            self.path.push(' ');
        }
    }

    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.path.push('C');
        for value in [c1x, c1y, c2x, c2y, x, y] {
            Self::push_number(&mut self.path, value);
            self.path.push(' ');
        }
    }

    fn close(&mut self) {
        self.path.push('Z');
    }

    fn fill(&mut self) {
        self.append_path(true, false);
    }

    fn stroke(&mut self) {
        self.append_path(false, true);
    }

    fn fill_and_stroke(&mut self) {
        self.append_path(true, true);
    }
}

fn write_paint(target: &mut String, name: &str, paint: Option<Paint>) {
    use std::fmt::Write;
    match paint {
        Some(Paint::Literal(color)) => {
            let _ = write!(
                target,
                " {name}=\"#{:02x}{:02x}{:02x}{:02x}\"",
                color.r, color.g, color.b, color.a,
            );
        }
        Some(Paint::Tint) => {
            let _ = write!(target, " {name}=\"currentColor\"");
        }
        None => {
            let _ = write!(target, " {name}=\"none\"");
        }
    }
}

fn multiply(left: Transform, right: Transform) -> Transform {
    Transform {
        a: left.a * right.a + left.c * right.b,
        b: left.b * right.a + left.d * right.b,
        c: left.a * right.c + left.c * right.d,
        d: left.b * right.c + left.d * right.d,
        tx: left.a * right.tx + left.c * right.ty + left.tx,
        ty: left.b * right.tx + left.d * right.ty + left.ty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_list_becomes_scalable_svg_markup() {
        let compiled = whisker_svg::compile(
            r#"<svg viewBox="0 0 24 24"><path d="M2 3 L20 3 L20 21 Z" fill="currentColor"/></svg>"#,
        )
        .unwrap();
        let rendered = render_display_list(&compiled.bytes).unwrap();
        assert_eq!(rendered.view_box.as_deref(), Some("0 0 24 24"));
        assert!(rendered.body.contains("fill=\"currentColor\""));
        assert!(rendered.body.contains("M2 3L20 3L20 21Z"));
    }

    #[test]
    fn literal_color_and_transform_are_preserved() {
        let compiled = whisker_svg::compile(
            r##"<svg viewBox="0 0 10 10"><g transform="translate(2 3)"><rect width="4" height="5" fill="#ff000080"/></g></svg>"##,
        )
        .unwrap();
        let rendered = render_display_list(&compiled.bytes).unwrap();
        assert!(rendered.body.contains("fill=\"#ff000080\""));
        assert!(rendered.body.contains("transform=\"matrix(1 0 0 1 2 3)\""));
    }
}
