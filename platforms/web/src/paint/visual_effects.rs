use whisker_protocol::{
    ClipShape, FillRule, ImageRendering, PaintBox, PaintCoordinate, PaintLengthPercentage,
    PaintPosition, PathCommand, VisualEffects,
};

use super::color::css_color;
use crate::{WebError, set_style};

pub(crate) fn supports(effects: &VisualEffects) -> bool {
    let mut remainder = effects.clone();
    remainder.box_shadows.clear();
    remainder.clip_path = None;
    remainder.backdrop_blur = None;
    remainder.image_rendering = ImageRendering::Auto;
    remainder == VisualEffects::default()
        && matches!(
            effects.image_rendering,
            ImageRendering::Auto | ImageRendering::Pixelated | ImageRendering::CrispEdges
        )
        && effects.clip_path.as_ref().is_none_or(|(reference, shape)| {
            matches!(
                reference,
                PaintBox::Border | PaintBox::Padding | PaintBox::Content
            ) && (matches!(
                shape,
                ClipShape::Inset { .. } | ClipShape::Circle { .. } | ClipShape::Ellipse { .. }
            ) || matches!(shape, ClipShape::Path { commands, .. } if path_is_absolute(commands)))
        })
}

pub(crate) fn apply(element: &web_sys::Element, effects: &VisualEffects) -> Result<(), WebError> {
    if !supports(effects) {
        return Err(WebError(
            "DOM Host only implements box-shadow, clip-path, backdrop blur, and image-rendering"
                .into(),
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
    let backdrop_filter = effects
        .backdrop_blur
        .map(|radius| format!("blur({radius}px)"))
        .unwrap_or_else(|| "none".into());
    set_style(element, "backdrop-filter", &backdrop_filter)?;
    set_style(element, "-webkit-backdrop-filter", &backdrop_filter)?;
    set_style(
        element,
        "image-rendering",
        match effects.image_rendering {
            ImageRendering::Pixelated => "pixelated",
            ImageRendering::CrispEdges => "crisp-edges",
            ImageRendering::Auto => "auto",
            _ => unreachable!("unsupported image-rendering rejected above"),
        },
    )?;
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
    let shape = match &value.1 {
        ClipShape::Inset { edges, radii } => format!(
            "inset({} {} {} {} round {} {} {} {} / {} {} {} {})",
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
        ),
        ClipShape::Circle { radius, center } => format!(
            "circle({} at {} {})",
            length(*radius),
            coordinate(center.x),
            coordinate(center.y),
        ),
        ClipShape::Ellipse {
            radius_x,
            radius_y,
            center,
        } => format!(
            "ellipse({} {} at {} {})",
            length(*radius_x),
            length(*radius_y),
            coordinate(center.x),
            coordinate(center.y),
        ),
        ClipShape::Path {
            fill_rule,
            commands,
        } => format!(
            "path({}, \"{}\")",
            match fill_rule {
                FillRule::NonZero => "nonzero",
                FillRule::EvenOdd => "evenodd",
            },
            path_data(commands)?,
        ),
        _ => return Err(WebError("unsupported DOM clip-path shape".into())),
    };
    Ok(format!("{shape} {reference_box}"))
}

fn path_is_absolute(commands: &[PathCommand]) -> bool {
    let position = |point: &PaintPosition| point.x.fraction == 0.0 && point.y.fraction == 0.0;
    commands.iter().all(|command| match command {
        PathCommand::MoveTo(point) | PathCommand::LineTo(point) => position(point),
        PathCommand::QuadraticTo { control, end } => position(control) && position(end),
        PathCommand::CubicTo {
            control_1,
            control_2,
            end,
        } => position(control_1) && position(control_2) && position(end),
        PathCommand::Close => true,
    })
}

fn path_data(commands: &[PathCommand]) -> Result<String, WebError> {
    if !path_is_absolute(commands) {
        return Err(WebError(
            "CSS path() coordinates cannot contain percentages".into(),
        ));
    }
    let point = |value: &PaintPosition| format!("{} {}", value.x.length, value.y.length);
    Ok(commands
        .iter()
        .map(|command| match command {
            PathCommand::MoveTo(value) => format!("M {}", point(value)),
            PathCommand::LineTo(value) => format!("L {}", point(value)),
            PathCommand::QuadraticTo { control, end } => {
                format!("Q {} {}", point(control), point(end))
            }
            PathCommand::CubicTo {
                control_1,
                control_2,
                end,
            } => format!("C {} {} {}", point(control_1), point(control_2), point(end)),
            PathCommand::Close => "Z".into(),
        })
        .collect::<Vec<_>>()
        .join(" "))
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
