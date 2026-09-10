use super::theme;
use whisker::css::{PointerEvents, PositionKind};
use whisker::prelude::*;
use whisker_input::InputSize;
use whisker_svg::Svg;

pub const ACTION_SPACE: f32 = 80.0;
pub const INPUT_LEFT: f32 = 17.0;
pub const INPUT_RIGHT: f32 = ACTION_SPACE + 8.0;
pub const INPUT_VERTICAL: f32 = 9.0;
const BREAK_POINT: f32 = 0.84;

#[component]
pub fn composer_surface(progress: ReadSignal<f32>, input_size: ReadSignal<InputSize>) -> Element {
    let appearance = theme::use_appearance();
    render! {
        Svg(
            content: computed(move || {
                let palette = appearance.get().palette();
                let input = input_size.get();
                surface_shape(
                    input.width + INPUT_LEFT + INPUT_RIGHT,
                    input.height + INPUT_VERTICAL * 2.0,
                    progress.get(),
                    palette.paper,
                    palette.border,
                )
            }),
            color: "currentColor",
            style: Css::new()
                .position(PositionKind::Absolute)
                .left(px(0))
                .top(px(0))
                .width(percent(100))
                .height(percent(100))
                .pointer_events(PointerEvents::None),
        )
    }
}

pub fn reveal(progress: f32) -> f32 {
    smoothstep((progress - 0.4) / 0.5)
}

fn smoothstep(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn surface_shape(width: f32, height: f32, progress: f32, fill: u32, border: u32) -> String {
    let progress = progress.clamp(0.0, 1.0);
    let right = width - 0.5;
    let bottom = height - 0.5;
    let body = right - ACTION_SPACE * progress;
    let corner = 12.0 + 12.0 * progress;
    let shoulder = body - corner;
    let lower_corner = bottom - corner;
    let cy = height - 32.0;
    let cx = body.max(width - 27.0);
    let rx = right - cx;
    let neck = 15.0 * (1.0 - smoothstep((progress - 0.2) / (BREAK_POINT - 0.2))).powf(0.65);
    let ry = 15.0 + 11.0 * smoothstep(progress / 0.65);
    let upper = cy - neck;
    let lower = cy + neck;
    let top_ball = cy - ry;
    let bottom_ball = cy + ry;
    let left_ball = body.max(cx - rx);
    let waist = (body + left_ball) / 2.0;
    let upper_join = (upper - 12.0).max(corner);
    let lower_join = (lower + 12.0).min(lower_corner);
    let c = 0.552_284_8;
    let mut outline = format!("M 12.5 0.5 H {shoulder} Q {body} 0.5 {body} {corner}");
    if progress < BREAK_POINT {
        outline.push_str(&format!(
            " V {upper_join} C {body} {upper} {waist} {upper} {waist} {upper} \
             C {left_ball} {upper} {} {top_ball} {cx} {top_ball} \
             C {} {top_ball} {right} {} {right} {cy} \
             C {right} {} {} {bottom_ball} {cx} {bottom_ball} \
             C {} {bottom_ball} {left_ball} {lower} {waist} {lower} \
             C {waist} {lower} {body} {lower} {body} {lower_join}",
            left_ball.max(cx - rx * c),
            cx + rx * c,
            cy - ry * c,
            cy + ry * c,
            cx + rx * c,
            left_ball.max(cx - rx * c),
        ));
    }
    outline.push_str(&format!(
        " V {lower_corner} Q {body} {bottom} {shoulder} {bottom} \
         H 12.5 Q 0.5 {bottom} 0.5 {} V 12.5 Q 0.5 0.5 12.5 0.5 Z",
        bottom - 12.0,
    ));
    let mut svg = format!(
        r##"<svg viewBox="0 0 {width} {height}"><path d="{outline}" fill="#{fill:06x}" stroke="#{border:06x}" stroke-width="1"/>"##
    );
    if progress >= BREAK_POINT {
        let release = (progress - BREAK_POINT) / (1.0 - BREAK_POINT);
        let stretch = 1.0 + 0.045 * (release * std::f32::consts::PI).sin();
        svg.push_str(&format!(
            r##"<ellipse cx="{cx}" cy="{cy}" rx="{}" ry="{}" fill="#{fill:06x}" stroke="#{border:06x}" stroke-width="1"/>"##,
            rx * stretch, ry / stretch,
        ));
    }
    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_has_one_outline_until_the_button_separates() {
        for (width, height) in [(358.0, 62.0), (728.0, 230.0)] {
            for step in 0..=100 {
                let progress = step as f32 / 100.0;
                let svg = surface_shape(width, height, progress, 0x202124, 0x404144);
                assert!(whisker_svg::compile(&svg).is_ok(), "{progress}: {svg}");
                assert_eq!(svg.matches("<path").count(), 1);
                assert_eq!(svg.contains("<ellipse"), progress >= BREAK_POINT);
                assert!(!svg.contains("NaN"));
            }
        }
        assert_eq!(reveal(0.0), 0.0);
        assert_eq!(reveal(0.4), 0.0);
        assert_eq!(reveal(1.0), 1.0);
    }
}
