use whisker_protocol::{
    ClipShape, PaintBox, PaintCoordinate, PaintLengthPercentage, VisualEffects,
};

use super::color::css_color;
use crate::{WebError, set_style};

pub(crate) fn supports(effects: &VisualEffects) -> bool {
    let mut remainder = effects.clone();
    remainder.box_shadows.clear();
    remainder.clip_path = None;
    remainder == VisualEffects::default()
        && effects.clip_path.as_ref().is_none_or(|(reference, shape)| {
            matches!(
                reference,
                PaintBox::Border | PaintBox::Padding | PaintBox::Content
            ) && matches!(shape, ClipShape::Inset { .. })
        })
}

pub(crate) fn apply(element: &web_sys::Element, effects: &VisualEffects) -> Result<(), WebError> {
    if !supports(effects) {
        return Err(WebError(
            "DOM Host only implements the supported box-shadow and clip-path subset".into(),
        ));
    }
    let value = if effects.box_shadows.is_empty() {
        "none".into()
    } else {
        effects
            .box_shadows
            .iter()
            .map(|shadow| {
                format!(
                    "{} {}px {}px {}px {}px{}",
                    css_color(&shadow.color),
                    shadow.offset_x,
                    shadow.offset_y,
                    shadow.blur_radius,
                    shadow.spread_radius,
                    if shadow.inset { " inset" } else { "" },
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    set_style(element, "box-shadow", &value)?;
    let clip_path = effects
        .clip_path
        .as_ref()
        .map(clip_path_css)
        .transpose()?
        .unwrap_or_else(|| "none".into());
    set_style(element, "clip-path", &clip_path)
}

fn clip_path_css(value: &(PaintBox, ClipShape)) -> Result<String, WebError> {
    let reference_box = match value.0 {
        PaintBox::Border => "border-box",
        PaintBox::Padding => "padding-box",
        PaintBox::Content => "content-box",
        _ => return Err(WebError("unsupported DOM clip-path reference box".into())),
    };
    let ClipShape::Inset { edges, radii } = &value.1 else {
        return Err(WebError("unsupported DOM clip-path shape".into()));
    };
    Ok(format!(
        "inset({} {} {} {} round {} {} {} {} / {} {} {} {}) {reference_box}",
        coordinate(edges.top),
        coordinate(edges.right),
        coordinate(edges.bottom),
        coordinate(edges.left),
        length(radii.top_left.horizontal),
        length(radii.top_right.horizontal),
        length(radii.bottom_right.horizontal),
        length(radii.bottom_left.horizontal),
        length(radii.top_left.vertical),
        length(radii.top_right.vertical),
        length(radii.bottom_right.vertical),
        length(radii.bottom_left.vertical),
    ))
}

fn coordinate(value: PaintCoordinate) -> String {
    css_length(value.length, value.fraction)
}

fn length(value: PaintLengthPercentage) -> String {
    css_length(value.length, value.fraction)
}

fn css_length(length: f32, fraction: f32) -> String {
    if fraction == 0.0 {
        format!("{length}px")
    } else if length == 0.0 {
        format!("{}%", fraction * 100.0)
    } else {
        format!("calc({length}px + {}%)", fraction * 100.0)
    }
}
