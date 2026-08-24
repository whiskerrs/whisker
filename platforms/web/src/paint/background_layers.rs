use whisker_protocol::{
    BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, GradientStop, ImageRepeat,
    PaintBox, PaintCoordinate, PaintImage, PaintLengthPercentage, PaintPosition,
    RadialGradientExtent, RadialGradientShape,
};

use super::color::css_color;
use crate::{WebError, set_style};

/// Whether every layer belongs to the subset currently implemented by the DOM Host.
pub(crate) fn supports(layers: &[BackgroundLayer]) -> bool {
    if layers.is_empty() {
        return true;
    }
    let [layer] = layers else {
        return false;
    };
    let supported_image = match &layer.image {
        PaintImage::LinearGradient {
            repeating: false,
            stops,
            ..
        } => stops.iter().all(|stop| stop.position.is_some()),
        PaintImage::RadialGradient {
            shape: RadialGradientShape::Ellipse,
            extent: RadialGradientExtent::Explicit,
            radii: Some(_),
            repeating: false,
            stops,
            ..
        } => stops.iter().all(|stop| stop.position.is_some()),
        PaintImage::ConicGradient {
            repeating: false,
            stops,
            ..
        } => stops
            .iter()
            .all(|stop| stop.position.is_some_and(|position| position.length == 0.0)),
        _ => false,
    };
    let initial_geometry = layer.position == Default::default()
        && layer.size == BackgroundSize::Auto
        && layer.repeat_x == ImageRepeat::Repeat
        && layer.repeat_y == ImageRepeat::Repeat;
    let explicit_geometry =
        matches!(
            layer.size,
            BackgroundSize::Explicit {
                width: Some(_),
                height: Some(_),
            }
        ) && matches!(layer.repeat_x, ImageRepeat::Repeat | ImageRepeat::NoRepeat)
            && matches!(layer.repeat_y, ImageRepeat::Repeat | ImageRepeat::NoRepeat);
    let supported_geometry =
        (initial_geometry && layer.origin == PaintBox::Padding && layer.clip == PaintBox::Border)
            || (explicit_geometry
                && matches!(layer.origin, PaintBox::Border | PaintBox::Padding)
                && matches!(layer.clip, PaintBox::Border | PaintBox::Padding));
    supported_image
        && supported_geometry
        && layer.attachment == BackgroundAttachment::Scroll
        && layer.blend_mode == BlendMode::Normal
}

pub(crate) fn apply(
    element: &web_sys::Element,
    layers: &[BackgroundLayer],
) -> Result<(), WebError> {
    if !supports(layers) {
        return Err(WebError(
            "DOM Host only implements one supported gradient with explicit stops, supported border/padding boxes, two-axis auto or explicit size, and repeat/no-repeat"
                .into(),
        ));
    }

    let images = if layers.is_empty() {
        "none".to_owned()
    } else {
        layers
            .iter()
            .map(background_image)
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    };
    let position = layers
        .first()
        .map_or_else(|| "0px 0px".into(), background_position);
    let size = layers
        .first()
        .map_or_else(|| "auto".into(), background_size);
    let repeat = layers
        .first()
        .map_or_else(|| "repeat".into(), background_repeat);
    let origin = layers
        .first()
        .map_or("padding-box", |layer| background_box(layer.origin));
    let clip = layers
        .first()
        .map_or("border-box", |layer| background_box(layer.clip));
    set_style(element, "background-image", &images)?;
    set_style(element, "background-position", &position)?;
    set_style(element, "background-size", &size)?;
    set_style(element, "background-repeat", &repeat)?;
    set_style(element, "background-origin", origin)?;
    set_style(element, "background-clip", clip)?;
    set_style(element, "background-attachment", "scroll")?;
    set_style(element, "background-blend-mode", "normal")
}

fn background_image(layer: &BackgroundLayer) -> Result<String, WebError> {
    match &layer.image {
        PaintImage::LinearGradient {
            angle_degrees,
            repeating: false,
            stops,
        } => linear_gradient(*angle_degrees, stops),
        PaintImage::RadialGradient {
            shape: RadialGradientShape::Ellipse,
            extent: RadialGradientExtent::Explicit,
            center,
            radii: Some(radii),
            repeating: false,
            stops,
        } => radial_gradient(*center, *radii, stops),
        PaintImage::ConicGradient {
            from_degrees,
            center,
            repeating: false,
            stops,
        } => conic_gradient(*from_degrees, *center, stops),
        _ => Err(WebError("unsupported DOM background image".into())),
    }
}

fn linear_gradient(angle_degrees: f32, stops: &[GradientStop]) -> Result<String, WebError> {
    let stops = stops
        .iter()
        .map(gradient_stop)
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("linear-gradient({angle_degrees}deg, {stops})"))
}

fn radial_gradient(
    center: PaintPosition,
    radii: (PaintLengthPercentage, PaintLengthPercentage),
    stops: &[GradientStop],
) -> Result<String, WebError> {
    let stops = stops
        .iter()
        .map(gradient_stop)
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!(
        "radial-gradient(ellipse {} {} at {} {}, {stops})",
        length_percentage(radii.0),
        length_percentage(radii.1),
        coordinate(center.x),
        coordinate(center.y),
    ))
}

fn conic_gradient(
    from_degrees: f32,
    center: PaintPosition,
    stops: &[GradientStop],
) -> Result<String, WebError> {
    let stops = stops
        .iter()
        .map(conic_gradient_stop)
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!(
        "conic-gradient(from {from_degrees}deg at {} {}, {stops})",
        coordinate(center.x),
        coordinate(center.y),
    ))
}

fn gradient_stop(stop: &GradientStop) -> Result<String, WebError> {
    let position = stop
        .position
        .ok_or_else(|| WebError("DOM linear-gradient requires resolved stop positions".into()))?;
    Ok(format!(
        "{} {}",
        css_color(&stop.color),
        coordinate(position)
    ))
}

fn conic_gradient_stop(stop: &GradientStop) -> Result<String, WebError> {
    let position = stop
        .position
        .filter(|position| position.length == 0.0)
        .ok_or_else(|| WebError("DOM conic-gradient requires resolved turn stops".into()))?;
    Ok(format!(
        "{} {}turn",
        css_color(&stop.color),
        position.fraction
    ))
}

fn coordinate(value: PaintCoordinate) -> String {
    if value.length == 0.0 {
        format!("{}%", value.fraction * 100.0)
    } else if value.fraction == 0.0 {
        format!("{}px", value.length)
    } else {
        format!("calc({}px + {}%)", value.length, value.fraction * 100.0)
    }
}

fn length_percentage(value: PaintLengthPercentage) -> String {
    if value.length == 0.0 {
        format!("{}%", value.fraction * 100.0)
    } else if value.fraction == 0.0 {
        format!("{}px", value.length)
    } else {
        format!("calc({}px + {}%)", value.length, value.fraction * 100.0)
    }
}

fn background_position(layer: &BackgroundLayer) -> String {
    if layer.position == Default::default() {
        return "0px 0px".into();
    }
    format!(
        "{} {}",
        coordinate(layer.position.x),
        coordinate(layer.position.y)
    )
}

fn background_size(layer: &BackgroundLayer) -> String {
    match layer.size {
        BackgroundSize::Auto => "auto".into(),
        BackgroundSize::Explicit {
            width: Some(width),
            height: Some(height),
        } => format!("{} {}", length_percentage(width), length_percentage(height)),
        _ => unreachable!("unsupported background size passed preflight"),
    }
}

fn background_repeat(layer: &BackgroundLayer) -> String {
    match (layer.repeat_x, layer.repeat_y) {
        (ImageRepeat::Repeat, ImageRepeat::Repeat) => "repeat".into(),
        (ImageRepeat::NoRepeat, ImageRepeat::NoRepeat) => "no-repeat".into(),
        (ImageRepeat::Repeat, ImageRepeat::NoRepeat) => "repeat no-repeat".into(),
        (ImageRepeat::NoRepeat, ImageRepeat::Repeat) => "no-repeat repeat".into(),
        _ => unreachable!("unsupported background repeat passed preflight"),
    }
}

fn background_box(value: PaintBox) -> &'static str {
    match value {
        PaintBox::Border => "border-box",
        PaintBox::Padding => "padding-box",
        _ => unreachable!("unsupported background box passed preflight"),
    }
}
