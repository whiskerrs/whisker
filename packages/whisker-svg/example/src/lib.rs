//! `whisker-svg` example app.
//!
//! Renders a small gallery of SVGs to verify the display-list
//! pipeline end-to-end on a real device:
//!
//! * Solid fill (`<rect>`)
//! * Solid fill path with `M / L / Z`
//! * Cubic Bézier path
//! * Stroke + stroke-width
//! * `fill="currentColor"` tinting (host's `color:` flows through)
//! * Nested `<g transform>` with scale
//!
//! The same SVG strings could be moved to fixtures if we want
//! snapshot-style on-device tests later — for the first cut they
//! live inline so a `whisker run` round-trip is the only
//! verification step.

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_svg::Svg;

const BG: &str = "#101012";
const CARD_BG: &str = "#1c1c1f";
const FG: &str = "#f0f0f3";
const ACCENT: &str = "#ff5577";

// ---- SVG payloads ----------------------------------------------------------
//
// All hand-authored. Coordinates are in user units of the
// declared viewBox. `currentColor` is used for the tint demo;
// everything else uses literal fills / strokes so we can also
// see solid-colour behaviour in isolation.

const SVG_RECT: &str = r##"<svg viewBox="0 0 24 24">
    <rect x="2" y="2" width="20" height="20" fill="#5e8df0"/>
</svg>"##;

const SVG_TRIANGLE: &str = r##"<svg viewBox="0 0 24 24">
    <path d="M 12 3 L 22 21 L 2 21 Z" fill="#5fcf80"/>
</svg>"##;

const SVG_CUBIC: &str = r##"<svg viewBox="0 0 24 24">
    <path d="M 2 18 C 6 4, 18 4, 22 18 L 22 22 L 2 22 Z" fill="#f0b860"/>
</svg>"##;

const SVG_STROKE: &str = r##"<svg viewBox="0 0 24 24">
    <path d="M 4 4 L 20 4 L 20 20 L 4 20 Z"
          fill="none" stroke="#d05050" stroke-width="2"/>
</svg>"##;

/// Two-cubic heart silhouette using `currentColor`. The host
/// passes `color: ACCENT`, the producer emits `FILL_TINT`, and
/// the replayer substitutes the accent at fill time.
const SVG_HEART: &str = r##"<svg viewBox="0 0 24 24">
    <path d="M 12 21
             C -2 12, 4 1, 12 8
             C 20 1, 26 12, 12 21 Z"
          fill="currentColor"/>
</svg>"##;

const SVG_NESTED: &str = r##"<svg viewBox="0 0 24 24">
    <g transform="translate(12 12)">
        <g transform="scale(1.5 1.5)">
            <path d="M 0 -6 L 5 5 L -5 5 Z" fill="#a060ff"/>
        </g>
    </g>
</svg>"##;

// ---- App -------------------------------------------------------------------

#[whisker::main]
pub fn app() -> Element {
    let page_style = format!(
        "background-color: {BG}; flex-grow: 1; flex-shrink: 1; \
         display: flex; flex-direction: column; \
         padding-top: 48px; padding-bottom: 24px;",
    );
    let header_style = format!(
        "color: {FG}; font-size: 22px; font-weight: 700; \
         margin-left: 20px; margin-bottom: 16px;",
    );
    let grid_style = "display: flex; flex-direction: row; flex-wrap: wrap; \
                      padding-left: 12px; padding-right: 12px;"
        .to_string();

    render! {
        view(style: page_style) {
            text(style: header_style, value: "whisker-svg gallery")
            view(style: grid_style) {
                tile(label: "Rect (solid)",        svg: SVG_RECT,     color: FG)
                tile(label: "Path (triangle)",     svg: SVG_TRIANGLE, color: FG)
                tile(label: "Path (cubic curve)",  svg: SVG_CUBIC,    color: FG)
                tile(label: "Stroke + width",      svg: SVG_STROKE,   color: FG)
                tile(label: "currentColor tint",   svg: SVG_HEART,    color: ACCENT)
                tile(label: "Nested <g> transform",svg: SVG_NESTED,   color: FG)
            }
        }
    }
}

/// One labelled tile in the gallery — `<Svg>` framed by a dark
/// card with a caption below. Same shape for every entry so the
/// only variable in the visual is the SVG itself.
#[component]
fn tile(label: String, svg: String, color: String) -> Element {
    let card_style = "width: 50%; \
                      display: flex; flex-direction: column; align-items: center; \
                      padding: 12px;"
        .to_string();
    let frame_style = format!(
        "width: 96px; height: 96px; \
         background-color: {CARD_BG}; \
         border-radius: 12px; \
         display: flex; align-items: center; justify-content: center;",
    );
    let svg_style = "width: 64px; height: 64px;".to_string();
    let caption_style = format!("color: {FG}; font-size: 12px; margin-top: 8px;");

    render! {
        view(style: card_style) {
            view(style: frame_style) {
                Svg(content: svg.clone(), color: color.clone(), style: svg_style.clone())
            }
            text(style: caption_style, value: label.clone())
        }
    }
}
