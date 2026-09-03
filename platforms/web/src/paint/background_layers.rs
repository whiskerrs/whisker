use whisker_protocol::{
    BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, GradientStop, ImageRepeat,
    PaintBox, PaintCoordinate, PaintImage, PaintLengthPercentage, PaintPosition,
    RadialGradientExtent, RadialGradientShape, ResourceId,
};

use super::color::css_color;
use crate::{WebError, set_style};

/// Whether every layer belongs to the subset currently implemented by the DOM Host.
pub(crate) fn supports(layers: &[BackgroundLayer]) -> bool {
    layers.iter().all(supports_layer)
}

fn supports_layer(layer: &BackgroundLayer) -> bool {
    let supported_image = match &layer.image {
        PaintImage::Resource(_) => true,
        PaintImage::LinearGradient {
            repeating: false,
            stops,
            ..
        } => stops.iter().all(|stop| stop.position.is_some()),
        PaintImage::RadialGradient {
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
    let supported_geometry = matches!(
        layer.repeat_x,
        ImageRepeat::Repeat | ImageRepeat::NoRepeat | ImageRepeat::Space | ImageRepeat::Round
    ) && matches!(
        layer.repeat_y,
        ImageRepeat::Repeat | ImageRepeat::NoRepeat | ImageRepeat::Space | ImageRepeat::Round
    ) && matches!(
        layer.origin,
        PaintBox::Border | PaintBox::Padding | PaintBox::Content
    ) && matches!(
        layer.clip,
        PaintBox::Border | PaintBox::Padding | PaintBox::Content | PaintBox::BorderArea
    );
    supported_image
        && supported_geometry
        && layer.attachment == BackgroundAttachment::Scroll
        && layer.blend_mode == BlendMode::Normal
}

pub(crate) fn apply(
    element: &web_sys::Element,
    layers: &[BackgroundLayer],
    resolve_resource: impl Fn(ResourceId) -> Option<String>,
) -> Result<(), WebError> {
    if !supports(layers) {
        return Err(WebError(
            "DOM Host only implements supported gradients with explicit stops, supported boxes, background sizes, and repeat modes"
                .into(),
        ));
    }

    let images = if layers.is_empty() {
        "none".to_owned()
    } else {
        layers
            .iter()
            .map(|layer| background_image(layer, &resolve_resource))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    };
    let position = layer_values(layers, "0px 0px", background_position);
    let size = layer_values(layers, "auto", background_size);
    let repeat = layer_values(layers, "repeat", background_repeat);
    let origin = layer_values(layers, "padding-box", |layer| {
        background_box(layer.origin).into()
    });
    let clip = layer_values(layers, "border-box", |layer| {
        background_box(layer.clip).into()
    });
    let attachment = layer_values(layers, "scroll", |_| "scroll".into());
    let blend_mode = layer_values(layers, "normal", |_| "normal".into());
    set_style(element, "background-image", &images)?;
    set_style(element, "background-position", &position)?;
    set_style(element, "background-size", &size)?;
    set_style(element, "background-repeat", &repeat)?;
    set_style(element, "background-origin", &origin)?;
    set_style(element, "background-clip", &clip)?;
    set_style(element, "background-attachment", &attachment)?;
    set_style(element, "background-blend-mode", &blend_mode)
}

fn layer_values(
    layers: &[BackgroundLayer],
    empty: &str,
    value: impl Fn(&BackgroundLayer) -> String,
) -> String {
    if layers.is_empty() {
        empty.into()
    } else {
        layers.iter().map(value).collect::<Vec<_>>().join(", ")
    }
}

fn background_image(
    layer: &BackgroundLayer,
    resolve_resource: &impl Fn(ResourceId) -> Option<String>,
) -> Result<String, WebError> {
    match &layer.image {
        PaintImage::Resource(resource) => resolve_resource(*resource)
            .map(|url| format!("url(\"{}\")", escape_css_url(&url)))
            .ok_or_else(|| {
                WebError(format!(
                    "DOM Host background resource {} is not registered",
                    resource.get()
                ))
            }),
        PaintImage::LinearGradient {
            angle_degrees,
            repeating: false,
            stops,
        } => linear_gradient(*angle_degrees, stops),
        PaintImage::RadialGradient {
            shape,
            extent,
            center,
            radii,
            repeating: false,
            stops,
        } => radial_gradient(*shape, *extent, *center, *radii, stops),
        PaintImage::ConicGradient {
            from_degrees,
            center,
            repeating: false,
            stops,
        } => conic_gradient(*from_degrees, *center, stops),
        _ => Err(WebError("unsupported DOM background image".into())),
    }
}

fn escape_css_url(url: &str) -> String {
    url.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\a ")
        .replace('\r', "\\d ")
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
    shape: RadialGradientShape,
    extent: RadialGradientExtent,
    center: PaintPosition,
    radii: Option<(PaintLengthPercentage, PaintLengthPercentage)>,
    stops: &[GradientStop],
) -> Result<String, WebError> {
    let stops = stops
        .iter()
        .map(gradient_stop)
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let sizing = match (shape, extent, radii) {
        (RadialGradientShape::Circle, RadialGradientExtent::Explicit, Some((radius, _))) => {
            format!("circle {}", length_percentage(radius))
        }
        (RadialGradientShape::Ellipse, RadialGradientExtent::Explicit, Some((x, y))) => {
            format!("ellipse {} {}", length_percentage(x), length_percentage(y))
        }
        (shape, extent, None) if extent != RadialGradientExtent::Explicit => {
            format!("{} {}", radial_shape(shape), radial_extent(extent))
        }
        _ => return Err(WebError("invalid DOM radial gradient".into())),
    };
    Ok(format!(
        "radial-gradient({sizing} at {} {}, {stops})",
        coordinate(center.x),
        coordinate(center.y),
    ))
}

fn radial_shape(shape: RadialGradientShape) -> &'static str {
    match shape {
        RadialGradientShape::Circle => "circle",
        RadialGradientShape::Ellipse => "ellipse",
    }
}

fn radial_extent(extent: RadialGradientExtent) -> &'static str {
    match extent {
        RadialGradientExtent::ClosestSide => "closest-side",
        RadialGradientExtent::FarthestSide => "farthest-side",
        RadialGradientExtent::ClosestCorner => "closest-corner",
        RadialGradientExtent::FarthestCorner => "farthest-corner",
        RadialGradientExtent::Explicit => unreachable!(),
    }
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
        BackgroundSize::Cover => "cover".into(),
        BackgroundSize::Contain => "contain".into(),
        BackgroundSize::Explicit { width, height } => format!(
            "{} {}",
            width.map_or_else(|| "auto".into(), length_percentage),
            height.map_or_else(|| "auto".into(), length_percentage)
        ),
    }
}

fn background_repeat(layer: &BackgroundLayer) -> String {
    match (layer.repeat_x, layer.repeat_y) {
        (ImageRepeat::Repeat, ImageRepeat::Repeat) => "repeat".into(),
        (ImageRepeat::NoRepeat, ImageRepeat::NoRepeat) => "no-repeat".into(),
        (ImageRepeat::Repeat, ImageRepeat::NoRepeat) => "repeat no-repeat".into(),
        (ImageRepeat::NoRepeat, ImageRepeat::Repeat) => "no-repeat repeat".into(),
        (ImageRepeat::Space, ImageRepeat::Space) => "space".into(),
        (ImageRepeat::Space, ImageRepeat::Repeat) => "space repeat".into(),
        (ImageRepeat::Space, ImageRepeat::NoRepeat) => "space no-repeat".into(),
        (ImageRepeat::Repeat, ImageRepeat::Space) => "repeat space".into(),
        (ImageRepeat::NoRepeat, ImageRepeat::Space) => "no-repeat space".into(),
        (ImageRepeat::Round, ImageRepeat::Round) => "round".into(),
        (ImageRepeat::Round, ImageRepeat::Repeat) => "round repeat".into(),
        (ImageRepeat::Round, ImageRepeat::NoRepeat) => "round no-repeat".into(),
        (ImageRepeat::Round, ImageRepeat::Space) => "round space".into(),
        (ImageRepeat::Repeat, ImageRepeat::Round) => "repeat round".into(),
        (ImageRepeat::NoRepeat, ImageRepeat::Round) => "no-repeat round".into(),
        (ImageRepeat::Space, ImageRepeat::Round) => "space round".into(),
    }
}

fn background_box(value: PaintBox) -> &'static str {
    match value {
        PaintBox::Border => "border-box",
        PaintBox::Padding => "padding-box",
        PaintBox::Content => "content-box",
        PaintBox::BorderArea => "border-area",
        _ => unreachable!("unsupported background box passed preflight"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_protocol::PaintColor;

    fn stops() -> Vec<GradientStop> {
        [0.0, 1.0]
            .into_iter()
            .map(|fraction| GradientStop {
                color: PaintColor::Srgba {
                    red: (fraction * 255.0) as u8,
                    green: 0,
                    blue: 0,
                    alpha: 1.0,
                },
                position: Some(PaintCoordinate {
                    length: 0.0,
                    fraction,
                }),
            })
            .collect()
    }

    #[test]
    fn radial_gradient_preserves_keyword_shape_and_extent() {
        assert_eq!(
            radial_gradient(
                RadialGradientShape::Circle,
                RadialGradientExtent::FarthestCorner,
                PaintPosition::default(),
                None,
                &stops(),
            )
            .unwrap(),
            "radial-gradient(circle farthest-corner at 0% 0%, rgba(0, 0, 0, 1) 0%, rgba(255, 0, 0, 1) 100%)"
        );
    }

    #[test]
    fn radial_gradient_preserves_explicit_circle_radius() {
        let radius = PaintLengthPercentage {
            length: 24.0,
            fraction: 0.0,
        };
        assert!(
            radial_gradient(
                RadialGradientShape::Circle,
                RadialGradientExtent::Explicit,
                PaintPosition::default(),
                Some((radius, radius)),
                &stops(),
            )
            .unwrap()
            .starts_with("radial-gradient(circle 24px at")
        );
    }
}
