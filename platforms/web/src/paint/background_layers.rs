use whisker_protocol::{
    BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, GradientStop, ImageRepeat,
    PaintBox, PaintCoordinate, PaintImage,
};

use super::color::css_color;
use crate::{WebError, set_style};

/// Whether every layer belongs to the subset currently implemented by the DOM Host.
pub(crate) fn supports(layers: &[BackgroundLayer]) -> bool {
    layers.iter().all(|layer| {
        matches!(
            &layer.image,
            PaintImage::LinearGradient {
                repeating: false,
                stops,
                ..
            } if stops.iter().all(|stop| stop.position.is_some())
        ) && layer.position == Default::default()
            && layer.size == BackgroundSize::Auto
            && layer.repeat_x == ImageRepeat::Repeat
            && layer.repeat_y == ImageRepeat::Repeat
            && layer.origin == PaintBox::Padding
            && layer.clip == PaintBox::Border
            && layer.attachment == BackgroundAttachment::Scroll
            && layer.blend_mode == BlendMode::Normal
    })
}

pub(crate) fn apply(
    element: &web_sys::Element,
    layers: &[BackgroundLayer],
) -> Result<(), WebError> {
    if !supports(layers) {
        return Err(WebError(
            "DOM Host only implements non-repeating linear-gradient background layers with explicit stops and CSS initial layer values"
                .into(),
        ));
    }

    let images = if layers.is_empty() {
        "none".to_owned()
    } else {
        layers
            .iter()
            .map(linear_gradient)
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    };
    let layer_count = layers.len().max(1);
    set_style(element, "background-image", &images)?;
    set_style(
        element,
        "background-position",
        &initial_list("0px 0px", layer_count),
    )?;
    set_style(
        element,
        "background-size",
        &initial_list("auto", layer_count),
    )?;
    set_style(
        element,
        "background-repeat",
        &initial_list("repeat", layer_count),
    )?;
    set_style(
        element,
        "background-origin",
        &initial_list("padding-box", layer_count),
    )?;
    set_style(
        element,
        "background-clip",
        &initial_list("border-box", layer_count),
    )?;
    set_style(
        element,
        "background-attachment",
        &initial_list("scroll", layer_count),
    )?;
    set_style(
        element,
        "background-blend-mode",
        &initial_list("normal", layer_count),
    )
}

fn linear_gradient(layer: &BackgroundLayer) -> Result<String, WebError> {
    let PaintImage::LinearGradient {
        angle_degrees,
        repeating: false,
        stops,
    } = &layer.image
    else {
        return Err(WebError("unsupported DOM background image".into()));
    };
    let stops = stops
        .iter()
        .map(gradient_stop)
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("linear-gradient({angle_degrees}deg, {stops})"))
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

fn coordinate(value: PaintCoordinate) -> String {
    if value.length == 0.0 {
        format!("{}%", value.fraction * 100.0)
    } else if value.fraction == 0.0 {
        format!("{}px", value.length)
    } else {
        format!("calc({}px + {}%)", value.length, value.fraction * 100.0)
    }
}

fn initial_list(value: &str, count: usize) -> String {
    std::iter::repeat_n(value, count)
        .collect::<Vec<_>>()
        .join(", ")
}
