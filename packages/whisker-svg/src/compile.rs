//! SVG XML → display-list compiler.
//!
//! Walks the input document with `roxmltree`, resolves paint
//! state inheritance through `<g>` nests, and emits the byte
//! stream defined in `packages/whisker-svg/SPEC.md` via
//! [`DisplayListBuilder`].
//!
//! ## v1 element coverage
//!
//! * `<svg>` — `viewBox` (required) becomes the `VIEWPORT` opcode.
//!   `width` / `height` are ignored (the host view's size is the
//!   render target).
//! * `<g>` — `transform`, `opacity`, and the paint-inheritance
//!   attributes (`fill`, `stroke`, `stroke-width`) are honoured.
//!   Group itself emits `SAVE` … `RESTORE` around its children.
//! * `<path d>` — the d attribute is parsed by [`path_parse`] and
//!   each command becomes the corresponding `PATH_*` opcode.
//! * `<rect x y width height>` — decomposed into `M / L / L / L /
//!   Z` (no rx/ry support in v1).
//! * `<circle cx cy r>`, `<ellipse cx cy rx ry>` — cubic Bézier
//!   approximation (4 quadrants × 1 cubic each).
//! * `<line x1 y1 x2 y2>` — `M / L`.
//! * `<polyline points>`, `<polygon points>` — list of `M / L (…)`,
//!   with `Z` for polygon.
//!
//! ## Inherited paint state
//!
//! `fill` and `stroke` cascade from `<svg>` → `<g>` → leaves. A
//! shape with no `fill` attribute and no inherited fill defaults
//! to `#000000FF` (SVG spec). `fill="none"` is honoured (= no
//! fill emitted), and `fill="currentColor"` emits `FILL_TINT`
//! instead of a literal colour.
//!
//! ## Error tolerance
//!
//! A malformed colour / transform / number on one element is
//! reported in [`CompileError::warnings`] but doesn't abort
//! compilation — partial display lists with one bad element
//! skipped are usually better than rendering nothing.

use crate::builder::{Color, DisplayListBuilder, Transform};
use crate::path_parse::{self, PathCommand};

/// Output of [`compile`].
#[derive(Debug)]
pub struct Compiled {
    /// The byte stream — pass to platform via `WhiskerValue::Bytes`.
    pub bytes: Vec<u8>,
    /// Non-fatal issues encountered (bad colour, unknown element, …).
    /// Producers might surface these to the user as a console warn.
    pub warnings: Vec<String>,
}

/// Hard errors that stop compilation entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// `roxmltree` couldn't parse the XML at all.
    XmlParse(String),
    /// The root element wasn't `<svg>`.
    NotSvg,
    /// `<svg>` had no `viewBox` and no `width`+`height` from which
    /// to derive one. We need an explicit user-unit coordinate
    /// system to emit the `VIEWPORT` opcode.
    NoViewBox,
}

/// Compile an SVG XML string into a v1 display-list byte stream.
pub fn compile(svg_xml: &str) -> Result<Compiled, CompileError> {
    let doc =
        roxmltree::Document::parse(svg_xml).map_err(|e| CompileError::XmlParse(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(CompileError::NotSvg);
    }

    let viewbox = resolve_viewbox(&root).ok_or(CompileError::NoViewBox)?;
    let mut builder = DisplayListBuilder::new();
    builder.viewport(viewbox.0, viewbox.1, viewbox.2, viewbox.3);

    let mut ctx = Ctx {
        warnings: Vec::new(),
    };
    let initial_state = PaintState::default();
    for child in root.children() {
        if child.is_element() {
            walk(&child, &mut builder, &mut ctx, &initial_state);
        }
    }

    Ok(Compiled {
        bytes: builder.finish(),
        warnings: ctx.warnings,
    })
}

/// Inherited paint state — `walk()` clones + overrides per element.
#[derive(Debug, Clone)]
struct PaintState {
    fill: Paint,
    stroke: Paint,
    stroke_width: f32,
    opacity: f32,
}

impl Default for PaintState {
    fn default() -> Self {
        Self {
            // SVG default for shapes is black solid.
            fill: Paint::Color(Color::BLACK),
            stroke: Paint::None,
            stroke_width: 1.0,
            opacity: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Paint {
    None,
    Color(Color),
    Tint,
}

struct Ctx {
    warnings: Vec<String>,
}

fn walk(node: &roxmltree::Node, b: &mut DisplayListBuilder, ctx: &mut Ctx, inherited: &PaintState) {
    let mut state = inherited.clone();
    // Update paint state from this node's own attributes (inherited
    // from parent if unset).
    update_paint(node, &mut state, ctx);

    let name = node.tag_name().name();
    // <g> and <svg>-as-inner-group: wrap children in SAVE/RESTORE
    // so transforms / opacity stack properly.
    if name == "g" {
        b.save();
        if let Some(t) = parse_transform(node.attribute("transform"), ctx) {
            b.concat(&t);
        }
        if state.opacity != inherited.opacity {
            b.opacity(state.opacity);
        }
        for child in node.children() {
            if child.is_element() {
                walk(&child, b, ctx, &state);
            }
        }
        b.restore();
        return;
    }

    // Leaf shapes — collect their path commands and emit.
    let cmds = shape_to_path(node, ctx);
    if cmds.is_empty() && name != "defs" && name != "title" && name != "desc" {
        ctx.warnings.push(format!("unsupported element <{name}>"));
        return;
    }

    if !cmds.is_empty() {
        // Per-shape SAVE/RESTORE if transform present, so it
        // doesn't leak to siblings.
        let local_t = parse_transform(node.attribute("transform"), ctx);
        if local_t.is_some() {
            b.save();
            if let Some(t) = local_t {
                b.concat(&t);
            }
        }

        emit_paint(b, &state);
        b.path_begin();
        for cmd in &cmds {
            match *cmd {
                PathCommand::MoveTo(x, y) => b.move_to(x, y),
                PathCommand::LineTo(x, y) => b.line_to(x, y),
                PathCommand::QuadTo(cx, cy, x, y) => b.quad_to(cx, cy, x, y),
                PathCommand::CubicTo(c1x, c1y, c2x, c2y, x, y) => {
                    b.cubic_to(c1x, c1y, c2x, c2y, x, y)
                }
                PathCommand::Close => b.close(),
            }
        }
        match (
            matches!(state.fill, Paint::None),
            matches!(state.stroke, Paint::None),
        ) {
            (false, false) => b.fill_and_stroke(),
            (false, true) => b.fill(),
            (true, false) => b.stroke(),
            (true, true) => {} // both none — emit no execution op
        }

        if local_t.is_some() {
            b.restore();
        }
    }
}

fn emit_paint(b: &mut DisplayListBuilder, s: &PaintState) {
    match s.fill {
        Paint::None => {} // OP_PATH_FILL won't be emitted either
        Paint::Color(c) => b.fill_color(c),
        Paint::Tint => b.fill_tint(),
    }
    match s.stroke {
        Paint::None => {}
        Paint::Color(c) => b.stroke_color(c),
        Paint::Tint => b.stroke_tint(),
    }
    if !matches!(s.stroke, Paint::None) {
        b.stroke_width(s.stroke_width);
    }
}

fn update_paint(node: &roxmltree::Node, s: &mut PaintState, ctx: &mut Ctx) {
    if let Some(v) = attr_or_style(node, "fill") {
        if let Some(p) = parse_paint(v, ctx) {
            s.fill = p;
        }
    }
    if let Some(v) = attr_or_style(node, "stroke") {
        if let Some(p) = parse_paint(v, ctx) {
            s.stroke = p;
        }
    }
    if let Some(v) = attr_or_style(node, "stroke-width") {
        if let Ok(n) = v.trim().parse::<f32>() {
            s.stroke_width = n;
        }
    }
    if let Some(v) = attr_or_style(node, "opacity") {
        if let Ok(n) = v.trim().parse::<f32>() {
            s.opacity *= n.clamp(0.0, 1.0);
        }
    }
}

/// Look up an attribute by name, falling back to the `style="…"`
/// declarations. SVG icons in the wild mix the two interchangeably.
fn attr_or_style<'a>(node: &'a roxmltree::Node, name: &str) -> Option<&'a str> {
    if let Some(v) = node.attribute(name) {
        return Some(v);
    }
    let style = node.attribute("style")?;
    for decl in style.split(';') {
        if let Some((k, v)) = decl.split_once(':') {
            if k.trim() == name {
                return Some(v.trim());
            }
        }
    }
    None
}

fn parse_paint(v: &str, ctx: &mut Ctx) -> Option<Paint> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("none") {
        return Some(Paint::None);
    }
    if v.eq_ignore_ascii_case("currentcolor") || v.eq_ignore_ascii_case("currentColor") {
        return Some(Paint::Tint);
    }
    parse_color(v).map(Paint::Color).or_else(|| {
        ctx.warnings.push(format!("unknown colour `{v}`"));
        None
    })
}

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(rest) = s.strip_prefix("rgb(").and_then(|t| t.strip_suffix(')')) {
        let parts: Vec<&str> = rest.split(',').map(str::trim).collect();
        if parts.len() == 3 {
            let r = parts[0].parse::<i32>().ok()?.clamp(0, 255) as u8;
            let g = parts[1].parse::<i32>().ok()?.clamp(0, 255) as u8;
            let b = parts[2].parse::<i32>().ok()?.clamp(0, 255) as u8;
            return Some(Color::rgb(r, g, b));
        }
    }
    if let Some(rest) = s.strip_prefix("rgba(").and_then(|t| t.strip_suffix(')')) {
        let parts: Vec<&str> = rest.split(',').map(str::trim).collect();
        if parts.len() == 4 {
            let r = parts[0].parse::<i32>().ok()?.clamp(0, 255) as u8;
            let g = parts[1].parse::<i32>().ok()?.clamp(0, 255) as u8;
            let b = parts[2].parse::<i32>().ok()?.clamp(0, 255) as u8;
            let a = (parts[3].parse::<f32>().ok()?.clamp(0.0, 1.0) * 255.0).round() as u8;
            return Some(Color::rgba(r, g, b, a));
        }
    }
    // Limited named-colour subset — enough for the in-tree fixtures.
    Some(match s {
        "black" => Color::BLACK,
        "white" => Color::rgb(255, 255, 255),
        "red" => Color::rgb(255, 0, 0),
        "green" => Color::rgb(0, 128, 0),
        "blue" => Color::rgb(0, 0, 255),
        "transparent" => Color::TRANSPARENT,
        _ => return None,
    })
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    let to_byte = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let bytes = hex.as_bytes();
    match bytes.len() {
        3 => {
            let r = to_byte(bytes[0])?;
            let g = to_byte(bytes[1])?;
            let b = to_byte(bytes[2])?;
            Some(Color::rgb(r * 16 + r, g * 16 + g, b * 16 + b))
        }
        6 => {
            let r = to_byte(bytes[0])? * 16 + to_byte(bytes[1])?;
            let g = to_byte(bytes[2])? * 16 + to_byte(bytes[3])?;
            let b = to_byte(bytes[4])? * 16 + to_byte(bytes[5])?;
            Some(Color::rgb(r, g, b))
        }
        8 => {
            let r = to_byte(bytes[0])? * 16 + to_byte(bytes[1])?;
            let g = to_byte(bytes[2])? * 16 + to_byte(bytes[3])?;
            let b = to_byte(bytes[4])? * 16 + to_byte(bytes[5])?;
            let a = to_byte(bytes[6])? * 16 + to_byte(bytes[7])?;
            Some(Color::rgba(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_transform(s: Option<&str>, ctx: &mut Ctx) -> Option<Transform> {
    let s = s?.trim();
    let mut result = Transform::IDENTITY;
    let mut rest = s;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let Some(open) = rest.find('(') else { break };
        let name = rest[..open].trim();
        rest = &rest[open + 1..];
        let Some(close) = rest.find(')') else {
            ctx.warnings
                .push(format!("unterminated transform `{name}(`"));
            break;
        };
        let args: Vec<f32> = rest[..close]
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        rest = &rest[close + 1..];
        let m = match (name, args.as_slice()) {
            ("translate", &[tx]) => Transform::translate(tx, 0.0),
            ("translate", &[tx, ty]) => Transform::translate(tx, ty),
            ("scale", &[s]) => Transform::scale(s, s),
            ("scale", &[sx, sy]) => Transform::scale(sx, sy),
            ("rotate", &[deg]) => {
                let r = deg.to_radians();
                Transform {
                    a: r.cos(),
                    b: r.sin(),
                    c: -r.sin(),
                    d: r.cos(),
                    tx: 0.0,
                    ty: 0.0,
                }
            }
            ("rotate", &[deg, cx, cy]) => {
                let r = deg.to_radians();
                // translate(cx, cy) * rotate(r) * translate(-cx, -cy)
                let cos = r.cos();
                let sin = r.sin();
                Transform {
                    a: cos,
                    b: sin,
                    c: -sin,
                    d: cos,
                    tx: cx - cx * cos + cy * sin,
                    ty: cy - cx * sin - cy * cos,
                }
            }
            ("matrix", &[a, b, c, d, tx, ty]) => Transform { a, b, c, d, tx, ty },
            _ => {
                ctx.warnings.push(format!("unsupported transform `{name}`"));
                continue;
            }
        };
        result = matmul(result, m);
    }
    Some(result)
}

fn matmul(a: Transform, b: Transform) -> Transform {
    Transform {
        a: a.a * b.a + a.c * b.b,
        b: a.b * b.a + a.d * b.b,
        c: a.a * b.c + a.c * b.d,
        d: a.b * b.c + a.d * b.d,
        tx: a.a * b.tx + a.c * b.ty + a.tx,
        ty: a.b * b.tx + a.d * b.ty + a.ty,
    }
}

fn resolve_viewbox(svg: &roxmltree::Node) -> Option<(f32, f32, f32, f32)> {
    if let Some(vb) = svg.attribute("viewBox") {
        let parts: Vec<f32> = vb
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if parts.len() == 4 {
            return Some((parts[0], parts[1], parts[2], parts[3]));
        }
    }
    // Fall back to width / height if both present and unitless.
    let w = svg.attribute("width").and_then(strip_unit_f32);
    let h = svg.attribute("height").and_then(strip_unit_f32);
    if let (Some(w), Some(h)) = (w, h) {
        return Some((0.0, 0.0, w, h));
    }
    None
}

fn strip_unit_f32(s: &str) -> Option<f32> {
    let s = s.trim();
    let digits_end = s
        .bytes()
        .position(|b| !(b.is_ascii_digit() || b == b'.' || b == b'-' || b == b'+'))
        .unwrap_or(s.len());
    s[..digits_end].parse::<f32>().ok()
}

// ---- shape → path normalisation -------------------------------------------

fn shape_to_path(node: &roxmltree::Node, ctx: &mut Ctx) -> Vec<PathCommand> {
    match node.tag_name().name() {
        "path" => node
            .attribute("d")
            .map(path_parse::parse)
            .unwrap_or_default(),
        "rect" => rect_to_path(node),
        "circle" => circle_to_path(node),
        "ellipse" => ellipse_to_path(node),
        "line" => line_to_path(node),
        "polyline" => poly_to_path(node, false),
        "polygon" => poly_to_path(node, true),
        _ => {
            let _ = ctx;
            Vec::new()
        }
    }
}

fn attr_f32(node: &roxmltree::Node, name: &str) -> f32 {
    node.attribute(name).and_then(strip_unit_f32).unwrap_or(0.0)
}

fn rect_to_path(node: &roxmltree::Node) -> Vec<PathCommand> {
    let x = attr_f32(node, "x");
    let y = attr_f32(node, "y");
    let w = attr_f32(node, "width");
    let h = attr_f32(node, "height");
    if w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    vec![
        PathCommand::MoveTo(x, y),
        PathCommand::LineTo(x + w, y),
        PathCommand::LineTo(x + w, y + h),
        PathCommand::LineTo(x, y + h),
        PathCommand::Close,
    ]
}

fn circle_to_path(node: &roxmltree::Node) -> Vec<PathCommand> {
    let cx = attr_f32(node, "cx");
    let cy = attr_f32(node, "cy");
    let r = attr_f32(node, "r");
    if r <= 0.0 {
        return Vec::new();
    }
    ellipse_path(cx, cy, r, r)
}

fn ellipse_to_path(node: &roxmltree::Node) -> Vec<PathCommand> {
    let cx = attr_f32(node, "cx");
    let cy = attr_f32(node, "cy");
    let rx = attr_f32(node, "rx");
    let ry = attr_f32(node, "ry");
    if rx <= 0.0 || ry <= 0.0 {
        return Vec::new();
    }
    ellipse_path(cx, cy, rx, ry)
}

fn ellipse_path(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<PathCommand> {
    // 4-cubic ellipse — standard kappa = 4*(sqrt(2)-1)/3 ≈ 0.5522847
    const K: f32 = 0.552_284_8;
    let krx = K * rx;
    let kry = K * ry;
    vec![
        PathCommand::MoveTo(cx, cy - ry),
        PathCommand::CubicTo(cx + krx, cy - ry, cx + rx, cy - kry, cx + rx, cy),
        PathCommand::CubicTo(cx + rx, cy + kry, cx + krx, cy + ry, cx, cy + ry),
        PathCommand::CubicTo(cx - krx, cy + ry, cx - rx, cy + kry, cx - rx, cy),
        PathCommand::CubicTo(cx - rx, cy - kry, cx - krx, cy - ry, cx, cy - ry),
        PathCommand::Close,
    ]
}

fn line_to_path(node: &roxmltree::Node) -> Vec<PathCommand> {
    let x1 = attr_f32(node, "x1");
    let y1 = attr_f32(node, "y1");
    let x2 = attr_f32(node, "x2");
    let y2 = attr_f32(node, "y2");
    vec![PathCommand::MoveTo(x1, y1), PathCommand::LineTo(x2, y2)]
}

fn poly_to_path(node: &roxmltree::Node, close: bool) -> Vec<PathCommand> {
    let pts = node.attribute("points").unwrap_or("");
    let nums: Vec<f32> = pts
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f32>().ok())
        .collect();
    if nums.len() < 4 || nums.len() % 2 != 0 {
        return Vec::new();
    }
    let mut out = vec![PathCommand::MoveTo(nums[0], nums[1])];
    for chunk in nums[2..].chunks_exact(2) {
        out.push(PathCommand::LineTo(chunk[0], chunk[1]));
    }
    if close {
        out.push(PathCommand::Close);
    }
    out
}
