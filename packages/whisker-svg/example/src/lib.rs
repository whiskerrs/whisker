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

use whisker::css::{AlignItems, FlexDirection, FlexWrap, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_svg::Svg;

const BG: u32 = 0x101012;
const CARD_BG: u32 = 0x1c1c1f;
const FG: u32 = 0xf0f0f3;
const FG_TEXT: &str = "#f0f0f3";
const ACCENT_TEXT: &str = "#ff5577";

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
    let page_style = Css::new()
        .background_color(Color::hex(BG))
        .flex_grow(1.0)
        .flex_shrink(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .padding_top(px(48))
        .padding_bottom(px(24));
    let header_style = Css::new()
        .color(Color::hex(FG))
        .font_size(px(22))
        .font_weight(FontWeight::Numeric(700))
        .margin_left(px(20))
        .margin_bottom(px(16));
    let grid_style = Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .flex_wrap(FlexWrap::Wrap)
        .padding_left(px(12))
        .padding_right(px(12));

    render! {
        view(style: page_style) {
            text(style: header_style, value: "whisker-svg gallery")
            view(style: grid_style) {
                tile(label: "Rect (solid)",        svg: SVG_RECT,     color: FG_TEXT)
                tile(label: "Path (triangle)",     svg: SVG_TRIANGLE, color: FG_TEXT)
                tile(label: "Path (cubic curve)",  svg: SVG_CUBIC,    color: FG_TEXT)
                tile(label: "Stroke + width",      svg: SVG_STROKE,   color: FG_TEXT)
                tile(label: "currentColor tint",   svg: SVG_HEART,    color: ACCENT_TEXT)
                tile(label: "Nested <g> transform",svg: SVG_NESTED,   color: FG_TEXT)
            }
        }
    }
}

/// One labelled tile in the gallery — `<Svg>` framed by a dark
/// card with a caption below. Same shape for every entry so the
/// only variable in the visual is the SVG itself.
#[component]
fn tile(label: String, svg: String, color: String) -> Element {
    let card_style = Css::new()
        .width(percent(50))
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .align_items(AlignItems::Center)
        .padding(px(12));
    let frame_style = Css::new()
        .width(px(96))
        .height(px(96))
        .background_color(Color::hex(CARD_BG))
        .border_radius(px(12))
        .display_flex()
        .align_items(AlignItems::Center)
        .justify_content(JustifyContent::Center);
    let svg_style = Css::new().width(px(64)).height(px(64));
    let caption_style = Css::new()
        .color(Color::hex(FG))
        .font_size(px(12))
        .margin_top(px(8));

    render! {
        view(style: card_style) {
            view(style: frame_style) {
                Svg(content: svg.clone(), color: color.clone(), style: svg_style.clone())
            }
            text(style: caption_style, value: label.clone())
        }
    }
}
