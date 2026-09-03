//! Internal traversal of typed values that contain length-percentage leaves.

use crate::{
    BackgroundImageValue, BackgroundSizeValue, ClipPathCommandValue, ClipPathValue, ClipPointValue,
    ClipShapeValue, ColorValue, ComponentValue, CustomPropertyReference, GradientValue,
    GridMaxTrackSizingValue, GridMinTrackSizingValue, GridTemplateComponentValue,
    LengthPercentageAutoValue, LengthPercentageValue, LengthValue, OffsetPathValue,
    RadialGradientValue, SizeValue, StyleNumber, StyleValue, TextDecorationValue, TextShadowValue,
    TransformFunctionValue,
};

/// Expected type of one custom-property reference nested in a composite value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComponentKind {
    Color,
    Length,
    Number,
    Angle,
}

/// Visits every custom-property reference nested outside `calc()`.
pub(crate) fn visit_component_variables<'a>(
    value: &'a StyleValue,
    visit: &mut dyn FnMut(&'a CustomPropertyReference),
) {
    match value {
        StyleValue::BackgroundImages(images) => {
            for image in images {
                visit_background_image_components(image, visit);
            }
        }
        StyleValue::Background(background) => {
            visit_component(&background.color, visit);
            for layer in &background.layers {
                visit_background_image_components(&layer.image, visit);
            }
        }
        StyleValue::BackdropFilter(crate::BackdropFilterValue::Blur(radius)) => {
            visit_component(radius, visit);
        }
        StyleValue::BoxShadows(shadows) => {
            for shadow in shadows {
                visit_component(&shadow.offset_x, visit);
                visit_component(&shadow.offset_y, visit);
                visit_component(&shadow.blur_radius, visit);
                visit_component(&shadow.spread_radius, visit);
                visit_component(&shadow.color, visit);
            }
        }
        StyleValue::Transform(transform) => {
            for function in &transform.0 {
                visit_transform_components(function, visit);
            }
        }
        StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x,
            offset_y,
            blur_radius,
            color,
        }) => {
            visit_component(offset_x, visit);
            visit_component(offset_y, visit);
            visit_component(blur_radius, visit);
            visit_component(color, visit);
        }
        StyleValue::TextDecoration(TextDecorationValue {
            color: Some(color), ..
        }) => visit_component(color, visit),
        _ => {}
    }
}

/// Clones a value while resolving every non-`calc()` component variable.
pub(crate) fn try_map_component_variables(
    value: &StyleValue,
    resolve: &mut dyn FnMut(&CustomPropertyReference, ComponentKind) -> Option<StyleValue>,
) -> Option<StyleValue> {
    let mut mapped = value.clone();
    match &mut mapped {
        StyleValue::BackgroundImages(images) => {
            for image in images {
                map_background_image_components(image, resolve)?;
            }
        }
        StyleValue::Background(background) => {
            map_color_component(&mut background.color, resolve)?;
            for layer in &mut background.layers {
                map_background_image_components(&mut layer.image, resolve)?;
            }
        }
        StyleValue::BackdropFilter(crate::BackdropFilterValue::Blur(radius)) => {
            map_length_component(radius, resolve)?;
        }
        StyleValue::BoxShadows(shadows) => {
            for shadow in shadows {
                map_length_component(&mut shadow.offset_x, resolve)?;
                map_length_component(&mut shadow.offset_y, resolve)?;
                map_length_component(&mut shadow.blur_radius, resolve)?;
                map_length_component(&mut shadow.spread_radius, resolve)?;
                map_color_component(&mut shadow.color, resolve)?;
            }
        }
        StyleValue::Transform(transform) => {
            for function in &mut transform.0 {
                map_transform_components(function, resolve)?;
            }
        }
        StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x,
            offset_y,
            blur_radius,
            color,
        }) => {
            map_length_component(offset_x, resolve)?;
            map_length_component(offset_y, resolve)?;
            map_length_component(blur_radius, resolve)?;
            map_color_component(color, resolve)?;
        }
        StyleValue::TextDecoration(TextDecorationValue {
            color: Some(color), ..
        }) => map_color_component(color, resolve)?,
        _ => {}
    }
    Some(mapped)
}

fn visit_component<'a, T>(
    component: &'a ComponentValue<T>,
    visit: &mut dyn FnMut(&'a CustomPropertyReference),
) {
    if let ComponentValue::Variable(reference) = component {
        visit(reference);
    }
}

fn visit_background_image_components<'a>(
    image: &'a BackgroundImageValue,
    visit: &mut dyn FnMut(&'a CustomPropertyReference),
) {
    let BackgroundImageValue::Gradient(gradient) = image else {
        return;
    };
    match gradient {
        GradientValue::Linear {
            angle_degrees,
            stops,
        } => {
            visit_component(angle_degrees, visit);
            visit_stop_components(stops, visit);
        }
        GradientValue::Radial { stops, .. } => visit_stop_components(stops, visit),
        GradientValue::Conic {
            from_degrees,
            stops,
            ..
        } => {
            visit_component(from_degrees, visit);
            visit_stop_components(stops, visit);
        }
    }
}

fn visit_stop_components<'a>(
    stops: &'a [crate::GradientStopValue],
    visit: &mut dyn FnMut(&'a CustomPropertyReference),
) {
    for stop in stops {
        visit_component(&stop.color, visit);
    }
}

fn visit_transform_components<'a>(
    function: &'a TransformFunctionValue,
    visit: &mut dyn FnMut(&'a CustomPropertyReference),
) {
    match function {
        TransformFunctionValue::TranslateZ(value) => visit_component(value, visit),
        TransformFunctionValue::Translate3d(_, _, value) => visit_component(value, visit),
        TransformFunctionValue::Rotate(value)
        | TransformFunctionValue::RotateX(value)
        | TransformFunctionValue::RotateY(value)
        | TransformFunctionValue::RotateZ(value)
        | TransformFunctionValue::ScaleX(value)
        | TransformFunctionValue::ScaleY(value)
        | TransformFunctionValue::SkewX(value)
        | TransformFunctionValue::SkewY(value) => visit_component(value, visit),
        TransformFunctionValue::Scale(x, y) | TransformFunctionValue::Skew(x, y) => {
            visit_component(x, visit);
            visit_component(y, visit);
        }
        _ => {}
    }
}

fn map_background_image_components(
    image: &mut BackgroundImageValue,
    resolve: &mut dyn FnMut(&CustomPropertyReference, ComponentKind) -> Option<StyleValue>,
) -> Option<()> {
    let BackgroundImageValue::Gradient(gradient) = image else {
        return Some(());
    };
    match gradient {
        GradientValue::Linear {
            angle_degrees,
            stops,
        } => {
            map_angle_component(angle_degrees, resolve)?;
            map_stop_components(stops, resolve)?;
        }
        GradientValue::Radial { stops, .. } => map_stop_components(stops, resolve)?,
        GradientValue::Conic {
            from_degrees,
            stops,
            ..
        } => {
            map_angle_component(from_degrees, resolve)?;
            map_stop_components(stops, resolve)?;
        }
    }
    Some(())
}

fn map_stop_components(
    stops: &mut [crate::GradientStopValue],
    resolve: &mut dyn FnMut(&CustomPropertyReference, ComponentKind) -> Option<StyleValue>,
) -> Option<()> {
    for stop in stops {
        map_color_component(&mut stop.color, resolve)?;
    }
    Some(())
}

fn map_transform_components(
    function: &mut TransformFunctionValue,
    resolve: &mut dyn FnMut(&CustomPropertyReference, ComponentKind) -> Option<StyleValue>,
) -> Option<()> {
    match function {
        TransformFunctionValue::TranslateZ(value) => map_length_component(value, resolve)?,
        TransformFunctionValue::Translate3d(_, _, value) => {
            map_length_component(value, resolve)?;
        }
        TransformFunctionValue::Rotate(value)
        | TransformFunctionValue::RotateX(value)
        | TransformFunctionValue::RotateY(value)
        | TransformFunctionValue::RotateZ(value)
        | TransformFunctionValue::SkewX(value)
        | TransformFunctionValue::SkewY(value) => map_angle_component(value, resolve)?,
        TransformFunctionValue::ScaleX(value) | TransformFunctionValue::ScaleY(value) => {
            map_number_component(value, resolve)?;
        }
        TransformFunctionValue::Scale(x, y) => {
            map_number_component(x, resolve)?;
            map_number_component(y, resolve)?;
        }
        TransformFunctionValue::Skew(x, y) => {
            map_angle_component(x, resolve)?;
            map_angle_component(y, resolve)?;
        }
        _ => {}
    }
    Some(())
}

fn map_component<T>(
    component: &mut ComponentValue<T>,
    kind: ComponentKind,
    resolve: &mut dyn FnMut(&CustomPropertyReference, ComponentKind) -> Option<StyleValue>,
    extract: impl FnOnce(StyleValue) -> Option<T>,
) -> Option<()> {
    let ComponentValue::Variable(reference) = component else {
        return Some(());
    };
    *component = ComponentValue::Value(extract(resolve(reference, kind)?)?);
    Some(())
}

fn map_color_component(
    component: &mut ComponentValue<ColorValue>,
    resolve: &mut dyn FnMut(&CustomPropertyReference, ComponentKind) -> Option<StyleValue>,
) -> Option<()> {
    map_component(
        component,
        ComponentKind::Color,
        resolve,
        |value| match value {
            StyleValue::Color(value) => Some(value),
            _ => None,
        },
    )
}

fn map_length_component(
    component: &mut ComponentValue<LengthValue>,
    resolve: &mut dyn FnMut(&CustomPropertyReference, ComponentKind) -> Option<StyleValue>,
) -> Option<()> {
    map_component(
        component,
        ComponentKind::Length,
        resolve,
        |value| match value {
            StyleValue::Length(value) => Some(value),
            _ => None,
        },
    )
}

fn map_number_component(
    component: &mut ComponentValue<StyleNumber>,
    resolve: &mut dyn FnMut(&CustomPropertyReference, ComponentKind) -> Option<StyleValue>,
) -> Option<()> {
    map_component(
        component,
        ComponentKind::Number,
        resolve,
        |value| match value {
            StyleValue::Number(value) => Some(value),
            _ => None,
        },
    )
}

fn map_angle_component(
    component: &mut ComponentValue<StyleNumber>,
    resolve: &mut dyn FnMut(&CustomPropertyReference, ComponentKind) -> Option<StyleValue>,
) -> Option<()> {
    map_component(
        component,
        ComponentKind::Angle,
        resolve,
        |value| match value {
            StyleValue::Angle(value) => Some(value),
            _ => None,
        },
    )
}

/// Visits every length-percentage leaf nested in a specified value.
pub(crate) fn visit_length_percentages<'a>(
    value: &'a StyleValue,
    visit: &mut dyn FnMut(&'a LengthPercentageValue),
) {
    match value {
        StyleValue::LengthPercentage(value) => visit(value),
        StyleValue::Size(SizeValue::LengthPercentage(value))
        | StyleValue::Size(SizeValue::FitContent(Some(value)))
        | StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(value))
        | StyleValue::FlexBasis(crate::FlexBasisValue::LengthPercentage(value))
        | StyleValue::LineHeight(crate::LineHeightValue::LengthPercentage(value)) => visit(value),
        StyleValue::BorderRadius(value) => {
            visit(&value.horizontal);
            visit(&value.vertical);
        }
        StyleValue::BackgroundImages(images) => {
            for image in images {
                visit_background_image(image, visit);
            }
        }
        StyleValue::Background(background) => {
            for layer in &background.layers {
                visit_background_image(&layer.image, visit);
                visit(&layer.position.horizontal);
                visit(&layer.position.vertical);
                visit_background_size(&layer.size, visit);
            }
        }
        StyleValue::BackgroundPosition(position) => {
            visit(&position.horizontal);
            visit(&position.vertical);
        }
        StyleValue::BackgroundSize(size) => visit_background_size(size, visit),
        StyleValue::Transform(transform) => {
            for function in &transform.0 {
                visit_transform(function, visit);
            }
        }
        StyleValue::TransformOrigin(origin) => {
            visit(&origin.horizontal);
            visit(&origin.vertical);
        }
        StyleValue::ClipPath(path) => visit_clip_path(path, visit),
        StyleValue::OffsetPath(path) => visit_offset_path(path, visit),
        StyleValue::GridTemplate(template) => {
            for component in &template.components {
                match component {
                    GridTemplateComponentValue::Track(track) => visit_grid_track(track, visit),
                    GridTemplateComponentValue::Repeat(repetition) => {
                        for track in &repetition.tracks {
                            visit_grid_track(track, visit);
                        }
                    }
                }
            }
        }
        StyleValue::GridTracks(tracks) => {
            for track in tracks {
                visit_grid_track(track, visit);
            }
        }
        _ => {}
    }
}

/// Clones a specified value while fallibly replacing every length-percentage leaf.
pub(crate) fn try_map_length_percentages(
    value: &StyleValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<StyleValue> {
    let mut mapped = value.clone();
    try_visit_length_percentages_mut(&mut mapped, map)?;
    Some(mapped)
}

fn visit_background_image<'a>(
    image: &'a BackgroundImageValue,
    visit: &mut dyn FnMut(&'a LengthPercentageValue),
) {
    let BackgroundImageValue::Gradient(gradient) = image else {
        return;
    };
    match gradient {
        GradientValue::Linear { stops, .. } => visit_stops(stops, visit),
        GradientValue::Radial { shape, stops } => {
            match shape {
                RadialGradientValue::CircleSized(radius) => visit(radius),
                RadialGradientValue::EllipseSized(horizontal, vertical) => {
                    visit(horizontal);
                    visit(vertical);
                }
                RadialGradientValue::Circle | RadialGradientValue::Ellipse => {}
            }
            visit_stops(stops, visit);
        }
        GradientValue::Conic { center, stops, .. } => {
            visit(&center.horizontal);
            visit(&center.vertical);
            visit_stops(stops, visit);
        }
    }
}

fn visit_stops<'a>(
    stops: &'a [crate::GradientStopValue],
    visit: &mut dyn FnMut(&'a LengthPercentageValue),
) {
    for stop in stops {
        if let Some(position) = &stop.position {
            visit(position);
        }
    }
}

fn visit_background_size<'a>(
    size: &'a BackgroundSizeValue,
    visit: &mut dyn FnMut(&'a LengthPercentageValue),
) {
    if let BackgroundSizeValue::Explicit { width, height } = size {
        if let Some(width) = width {
            visit(width);
        }
        if let Some(height) = height {
            visit(height);
        }
    }
}

fn visit_transform<'a>(
    function: &'a TransformFunctionValue,
    visit: &mut dyn FnMut(&'a LengthPercentageValue),
) {
    match function {
        TransformFunctionValue::Translate(horizontal, vertical)
        | TransformFunctionValue::Translate3d(horizontal, vertical, _) => {
            visit(horizontal);
            visit(vertical);
        }
        TransformFunctionValue::TranslateX(value) | TransformFunctionValue::TranslateY(value) => {
            visit(value);
        }
        _ => {}
    }
}

fn visit_offset_path<'a>(
    path: &'a OffsetPathValue,
    visit: &mut dyn FnMut(&'a LengthPercentageValue),
) {
    match path {
        OffsetPathValue::Circle {
            radius,
            center_x,
            center_y,
        } => {
            visit(radius);
            visit(center_x);
            visit(center_y);
        }
        OffsetPathValue::Ellipse {
            radius_x,
            radius_y,
            center_x,
            center_y,
        } => {
            visit(radius_x);
            visit(radius_y);
            visit(center_x);
            visit(center_y);
        }
        OffsetPathValue::Inset(inset) => {
            for offset in &inset.offsets {
                visit(offset);
            }
            if let Some(radii) = &inset.radii {
                for radius in radii {
                    visit(&radius.horizontal);
                    visit(&radius.vertical);
                }
            }
        }
        OffsetPathValue::None | OffsetPathValue::Path(_) => {}
    }
}

fn visit_clip_path<'a>(path: &'a ClipPathValue, visit: &mut dyn FnMut(&'a LengthPercentageValue)) {
    let ClipPathValue::Shape { shape, .. } = path else {
        return;
    };
    match shape {
        ClipShapeValue::Inset { offsets, radii } => {
            for offset in offsets {
                visit(offset);
            }
            if let Some(radii) = radii {
                for radius in radii {
                    visit(&radius.horizontal);
                    visit(&radius.vertical);
                }
            }
        }
        ClipShapeValue::Circle {
            radius,
            center_x,
            center_y,
        } => {
            visit(radius);
            visit(center_x);
            visit(center_y);
        }
        ClipShapeValue::Ellipse {
            radius_x,
            radius_y,
            center_x,
            center_y,
        } => {
            visit(radius_x);
            visit(radius_y);
            visit(center_x);
            visit(center_y);
        }
        ClipShapeValue::Path { commands, .. } => {
            for command in commands {
                visit_clip_path_command(command, visit);
            }
        }
    }
}

fn visit_clip_path_command<'a>(
    command: &'a ClipPathCommandValue,
    visit: &mut dyn FnMut(&'a LengthPercentageValue),
) {
    match command {
        ClipPathCommandValue::MoveTo(point) | ClipPathCommandValue::LineTo(point) => {
            visit_clip_point(point, visit);
        }
        ClipPathCommandValue::QuadraticTo { control, end } => {
            visit_clip_point(control, visit);
            visit_clip_point(end, visit);
        }
        ClipPathCommandValue::CubicTo {
            control_1,
            control_2,
            end,
        } => {
            visit_clip_point(control_1, visit);
            visit_clip_point(control_2, visit);
            visit_clip_point(end, visit);
        }
        ClipPathCommandValue::Close => {}
    }
}

fn visit_clip_point<'a>(
    point: &'a ClipPointValue,
    visit: &mut dyn FnMut(&'a LengthPercentageValue),
) {
    visit(&point.x);
    visit(&point.y);
}

fn visit_grid_track<'a>(
    track: &'a crate::GridTrackSizingValue,
    visit: &mut dyn FnMut(&'a LengthPercentageValue),
) {
    if let GridMinTrackSizingValue::Fixed(value) = &track.min {
        visit(value);
    }
    match &track.max {
        GridMaxTrackSizingValue::Fixed(value) | GridMaxTrackSizingValue::FitContent(value) => {
            visit(value);
        }
        _ => {}
    }
}

fn try_visit_length_percentages_mut(
    value: &mut StyleValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    match value {
        StyleValue::LengthPercentage(value) => map_one(value, map)?,
        StyleValue::Size(SizeValue::LengthPercentage(value))
        | StyleValue::Size(SizeValue::FitContent(Some(value)))
        | StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(value))
        | StyleValue::FlexBasis(crate::FlexBasisValue::LengthPercentage(value))
        | StyleValue::LineHeight(crate::LineHeightValue::LengthPercentage(value)) => {
            map_one(value, map)?;
        }
        StyleValue::BorderRadius(value) => {
            map_one(&mut value.horizontal, map)?;
            map_one(&mut value.vertical, map)?;
        }
        StyleValue::BackgroundImages(images) => {
            for image in images {
                map_background_image(image, map)?;
            }
        }
        StyleValue::Background(background) => {
            for layer in &mut background.layers {
                map_background_image(&mut layer.image, map)?;
                map_one(&mut layer.position.horizontal, map)?;
                map_one(&mut layer.position.vertical, map)?;
                map_background_size(&mut layer.size, map)?;
            }
        }
        StyleValue::BackgroundPosition(position) => {
            map_one(&mut position.horizontal, map)?;
            map_one(&mut position.vertical, map)?;
        }
        StyleValue::BackgroundSize(size) => map_background_size(size, map)?,
        StyleValue::Transform(transform) => {
            for function in &mut transform.0 {
                map_transform(function, map)?;
            }
        }
        StyleValue::TransformOrigin(origin) => {
            map_one(&mut origin.horizontal, map)?;
            map_one(&mut origin.vertical, map)?;
        }
        StyleValue::ClipPath(path) => map_clip_path(path, map)?,
        StyleValue::OffsetPath(path) => map_offset_path(path, map)?,
        StyleValue::GridTemplate(template) => {
            for component in &mut template.components {
                match component {
                    GridTemplateComponentValue::Track(track) => map_grid_track(track, map)?,
                    GridTemplateComponentValue::Repeat(repetition) => {
                        for track in &mut repetition.tracks {
                            map_grid_track(track, map)?;
                        }
                    }
                }
            }
        }
        StyleValue::GridTracks(tracks) => {
            for track in tracks {
                map_grid_track(track, map)?;
            }
        }
        _ => {}
    }
    Some(())
}

fn map_one(
    value: &mut LengthPercentageValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    *value = map(value)?;
    Some(())
}

fn map_background_image(
    image: &mut BackgroundImageValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    let BackgroundImageValue::Gradient(gradient) = image else {
        return Some(());
    };
    match gradient {
        GradientValue::Linear { stops, .. } => map_stops(stops, map)?,
        GradientValue::Radial { shape, stops } => {
            match shape {
                RadialGradientValue::CircleSized(radius) => map_one(radius, map)?,
                RadialGradientValue::EllipseSized(horizontal, vertical) => {
                    map_one(horizontal, map)?;
                    map_one(vertical, map)?;
                }
                RadialGradientValue::Circle | RadialGradientValue::Ellipse => {}
            }
            map_stops(stops, map)?;
        }
        GradientValue::Conic { center, stops, .. } => {
            map_one(&mut center.horizontal, map)?;
            map_one(&mut center.vertical, map)?;
            map_stops(stops, map)?;
        }
    }
    Some(())
}

fn map_stops(
    stops: &mut [crate::GradientStopValue],
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    for stop in stops {
        if let Some(position) = &mut stop.position {
            map_one(position, map)?;
        }
    }
    Some(())
}

fn map_background_size(
    size: &mut BackgroundSizeValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    if let BackgroundSizeValue::Explicit { width, height } = size {
        if let Some(width) = width {
            map_one(width, map)?;
        }
        if let Some(height) = height {
            map_one(height, map)?;
        }
    }
    Some(())
}

fn map_transform(
    function: &mut TransformFunctionValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    match function {
        TransformFunctionValue::Translate(horizontal, vertical)
        | TransformFunctionValue::Translate3d(horizontal, vertical, _) => {
            map_one(horizontal, map)?;
            map_one(vertical, map)?;
        }
        TransformFunctionValue::TranslateX(value) | TransformFunctionValue::TranslateY(value) => {
            map_one(value, map)?;
        }
        _ => {}
    }
    Some(())
}

fn map_offset_path(
    path: &mut OffsetPathValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    match path {
        OffsetPathValue::Circle {
            radius,
            center_x,
            center_y,
        } => {
            map_one(radius, map)?;
            map_one(center_x, map)?;
            map_one(center_y, map)?;
        }
        OffsetPathValue::Ellipse {
            radius_x,
            radius_y,
            center_x,
            center_y,
        } => {
            map_one(radius_x, map)?;
            map_one(radius_y, map)?;
            map_one(center_x, map)?;
            map_one(center_y, map)?;
        }
        OffsetPathValue::Inset(inset) => {
            for offset in &mut inset.offsets {
                map_one(offset, map)?;
            }
            if let Some(radii) = &mut inset.radii {
                for radius in radii {
                    map_one(&mut radius.horizontal, map)?;
                    map_one(&mut radius.vertical, map)?;
                }
            }
        }
        OffsetPathValue::None | OffsetPathValue::Path(_) => {}
    }
    Some(())
}

fn map_clip_path(
    path: &mut ClipPathValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    let ClipPathValue::Shape { shape, .. } = path else {
        return Some(());
    };
    match shape {
        ClipShapeValue::Inset { offsets, radii } => {
            for offset in offsets {
                map_one(offset, map)?;
            }
            if let Some(radii) = radii {
                for radius in radii {
                    map_one(&mut radius.horizontal, map)?;
                    map_one(&mut radius.vertical, map)?;
                }
            }
        }
        ClipShapeValue::Circle {
            radius,
            center_x,
            center_y,
        } => {
            map_one(radius, map)?;
            map_one(center_x, map)?;
            map_one(center_y, map)?;
        }
        ClipShapeValue::Ellipse {
            radius_x,
            radius_y,
            center_x,
            center_y,
        } => {
            map_one(radius_x, map)?;
            map_one(radius_y, map)?;
            map_one(center_x, map)?;
            map_one(center_y, map)?;
        }
        ClipShapeValue::Path { commands, .. } => {
            for command in commands {
                map_clip_path_command(command, map)?;
            }
        }
    }
    Some(())
}

fn map_clip_path_command(
    command: &mut ClipPathCommandValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    match command {
        ClipPathCommandValue::MoveTo(point) | ClipPathCommandValue::LineTo(point) => {
            map_clip_point(point, map)?;
        }
        ClipPathCommandValue::QuadraticTo { control, end } => {
            map_clip_point(control, map)?;
            map_clip_point(end, map)?;
        }
        ClipPathCommandValue::CubicTo {
            control_1,
            control_2,
            end,
        } => {
            map_clip_point(control_1, map)?;
            map_clip_point(control_2, map)?;
            map_clip_point(end, map)?;
        }
        ClipPathCommandValue::Close => {}
    }
    Some(())
}

fn map_clip_point(
    point: &mut ClipPointValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    map_one(&mut point.x, map)?;
    map_one(&mut point.y, map)
}

fn map_grid_track(
    track: &mut crate::GridTrackSizingValue,
    map: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    if let GridMinTrackSizingValue::Fixed(value) = &mut track.min {
        map_one(value, map)?;
    }
    match &mut track.max {
        GridMaxTrackSizingValue::Fixed(value) | GridMaxTrackSizingValue::FitContent(value) => {
            map_one(value, map)?;
        }
        _ => {}
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BackgroundAttachmentValue, BackgroundBoxValue, BackgroundLayerValue,
        BackgroundPositionValue, BackgroundRepeatModeValue, BackgroundRepeatValue, BackgroundValue,
        BorderRadiusValue, BoxShadowValue, ClipBoxValue, ClipFillRuleValue, ColorValue,
        CustomPropertyName, GradientStopValue, GridRepetitionCountValue,
        GridTemplateRepetitionValue, GridTemplateValue, GridTrackSizingValue, InsetPathValue,
        StyleNumber, TransformOriginValue, TransformValue,
    };

    fn lp(value: f32) -> LengthPercentageValue {
        LengthPercentageValue::Percentage(StyleNumber::new(value))
    }

    fn stop() -> GradientStopValue {
        GradientStopValue {
            color: ColorValue::Named("red".into()).into(),
            position: Some(lp(1.0)),
        }
    }

    fn track() -> GridTrackSizingValue {
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Fixed(lp(1.0)),
            max: GridMaxTrackSizingValue::FitContent(lp(1.0)),
        }
    }

    fn component_reference<T>(name: &CustomPropertyName) -> ComponentValue<T> {
        ComponentValue::Variable(CustomPropertyReference::new(name.clone()))
    }

    fn transparent() -> ColorValue {
        ColorValue::Named("transparent".into())
    }

    fn assert_maps_every_leaf(value: StyleValue, expected: usize) {
        let mut visited = 0;
        visit_length_percentages(&value, &mut |_| visited += 1);
        assert_eq!(visited, expected);

        let replacement = lp(99.0);
        let mut replace = |_: &LengthPercentageValue| Some(replacement.clone());
        let mapped = try_map_length_percentages(&value, &mut replace).unwrap();
        let mut mapped_count = 0;
        visit_length_percentages(&mapped, &mut |value| {
            mapped_count += 1;
            assert_eq!(value, &replacement);
        });
        assert_eq!(mapped_count, expected);

        for failure_index in 0..expected {
            let mut index = 0;
            let mut fail_once = |value: &LengthPercentageValue| {
                let should_fail = index == failure_index;
                index += 1;
                (!should_fail).then(|| value.clone())
            };
            let failed = try_map_length_percentages(&value, &mut fail_once);
            assert!(failed.is_none());
        }
    }

    fn assert_maps_every_component(value: StyleValue, expected: usize) {
        let mut visited = 0;
        visit_component_variables(&value, &mut |_| visited += 1);
        assert_eq!(visited, expected);

        let replacement = |kind| match kind {
            ComponentKind::Color => StyleValue::Color(transparent()),
            ComponentKind::Length => StyleValue::Length(LengthValue::Zero),
            ComponentKind::Number => StyleValue::Number(StyleNumber::new(2.0)),
            ComponentKind::Angle => StyleValue::Angle(StyleNumber::new(45.0)),
        };
        let mapped = try_map_component_variables(&value, &mut |_, kind| Some(replacement(kind)))
            .expect("every typed component maps");
        let mut remaining = 0;
        visit_component_variables(&mapped, &mut |_| remaining += 1);
        assert_eq!(remaining, 0);

        for failure_index in 0..expected {
            let mut index = 0;
            let failed = try_map_component_variables(&value, &mut |_, kind| {
                let should_fail = index == failure_index;
                index += 1;
                (!should_fail).then(|| replacement(kind))
            });
            assert!(failed.is_none());
        }
    }

    fn variable_stop(name: &CustomPropertyName) -> GradientStopValue {
        GradientStopValue {
            color: component_reference(name),
            position: None,
        }
    }

    #[test]
    fn composite_mapping_propagates_failure_from_every_component_slot() {
        let name = CustomPropertyName::new("--value").unwrap();
        assert_maps_every_component(
            StyleValue::BackgroundImages(vec![
                BackgroundImageValue::Gradient(GradientValue::Linear {
                    angle_degrees: component_reference(&name),
                    stops: vec![variable_stop(&name)],
                }),
                BackgroundImageValue::Gradient(GradientValue::Radial {
                    shape: RadialGradientValue::Circle,
                    stops: vec![variable_stop(&name)],
                }),
                BackgroundImageValue::Gradient(GradientValue::Conic {
                    from_degrees: component_reference(&name),
                    center: BackgroundPositionValue {
                        horizontal: lp(0.0),
                        vertical: lp(0.0),
                    },
                    stops: vec![variable_stop(&name)],
                }),
            ]),
            5,
        );
        assert_maps_every_component(
            StyleValue::Background(BackgroundValue {
                layers: vec![BackgroundLayerValue {
                    image: BackgroundImageValue::Gradient(GradientValue::Linear {
                        angle_degrees: component_reference(&name),
                        stops: vec![variable_stop(&name)],
                    }),
                    position: BackgroundPositionValue {
                        horizontal: lp(0.0),
                        vertical: lp(0.0),
                    },
                    size: BackgroundSizeValue::Auto,
                    repeat: BackgroundRepeatValue {
                        horizontal: BackgroundRepeatModeValue::Repeat,
                        vertical: BackgroundRepeatModeValue::Repeat,
                    },
                    origin: BackgroundBoxValue::Padding,
                    clip: BackgroundBoxValue::Border,
                    attachment: BackgroundAttachmentValue::Scroll,
                }],
                color: component_reference(&name),
            }),
            3,
        );
        assert_maps_every_component(
            StyleValue::BackdropFilter(crate::BackdropFilterValue::Blur(component_reference(
                &name,
            ))),
            1,
        );
        assert_maps_every_component(
            StyleValue::BoxShadows(vec![BoxShadowValue {
                offset_x: component_reference(&name),
                offset_y: component_reference(&name),
                blur_radius: component_reference(&name),
                spread_radius: component_reference(&name),
                color: component_reference(&name),
                inset: false,
            }]),
            5,
        );
        assert_maps_every_component(
            StyleValue::Transform(TransformValue(vec![
                TransformFunctionValue::TranslateZ(component_reference(&name)),
                TransformFunctionValue::Translate3d(lp(0.0), lp(0.0), component_reference(&name)),
                TransformFunctionValue::Rotate(component_reference(&name)),
                TransformFunctionValue::RotateX(component_reference(&name)),
                TransformFunctionValue::RotateY(component_reference(&name)),
                TransformFunctionValue::RotateZ(component_reference(&name)),
                TransformFunctionValue::ScaleX(component_reference(&name)),
                TransformFunctionValue::ScaleY(component_reference(&name)),
                TransformFunctionValue::Scale(
                    component_reference(&name),
                    component_reference(&name),
                ),
                TransformFunctionValue::Skew(
                    component_reference(&name),
                    component_reference(&name),
                ),
                TransformFunctionValue::SkewX(component_reference(&name)),
                TransformFunctionValue::SkewY(component_reference(&name)),
            ])),
            14,
        );
        assert_maps_every_component(
            StyleValue::TextShadow(TextShadowValue::Shadow {
                offset_x: component_reference(&name),
                offset_y: component_reference(&name),
                blur_radius: component_reference(&name),
                color: component_reference(&name),
            }),
            4,
        );
        assert_maps_every_component(
            StyleValue::TextDecoration(TextDecorationValue {
                line: crate::TextDecorationLineValue::Underline,
                style: crate::TextDecorationStyleValue::Solid,
                color: Some(component_reference(&name)),
            }),
            1,
        );
    }

    #[test]
    fn component_variables_visit_map_and_reject_each_wrong_type() {
        let name = CustomPropertyName::new("--value").unwrap();

        let mut visited = 0;
        let mut visit = |_| {
            visited += 1;
        };
        let color_reference = component_reference::<ColorValue>(&name);
        let color_literal = ComponentValue::Value(transparent());
        let length_reference = component_reference::<LengthValue>(&name);
        let length_literal = ComponentValue::Value(LengthValue::Zero);
        let number_reference = component_reference::<StyleNumber>(&name);
        let number_literal = ComponentValue::Value(StyleNumber::new(1.0));
        visit_component(&color_reference, &mut visit);
        visit_component(&color_literal, &mut visit);
        visit_component(&length_reference, &mut visit);
        visit_component(&length_literal, &mut visit);
        visit_component(&number_reference, &mut visit);
        visit_component(&number_literal, &mut visit);
        assert_eq!(visited, 3);

        let mut color = component_reference(&name);
        assert!(
            map_color_component(&mut color, &mut |_, kind| {
                assert_eq!(kind, ComponentKind::Color);
                Some(StyleValue::Color(transparent()))
            })
            .is_some()
        );
        assert_eq!(color, ComponentValue::Value(transparent()));

        let mut length = component_reference(&name);
        assert!(
            map_length_component(&mut length, &mut |_, kind| {
                assert_eq!(kind, ComponentKind::Length);
                Some(StyleValue::Length(LengthValue::Zero))
            })
            .is_some()
        );
        assert_eq!(length, ComponentValue::Value(LengthValue::Zero));

        let mut number = component_reference(&name);
        assert!(
            map_number_component(&mut number, &mut |_, kind| {
                assert_eq!(kind, ComponentKind::Number);
                Some(StyleValue::Number(StyleNumber::new(2.0)))
            })
            .is_some()
        );
        assert_eq!(number, ComponentValue::Value(StyleNumber::new(2.0)));

        let mut angle = component_reference(&name);
        assert!(
            map_angle_component(&mut angle, &mut |_, kind| {
                assert_eq!(kind, ComponentKind::Angle);
                Some(StyleValue::Angle(StyleNumber::new(45.0)))
            })
            .is_some()
        );
        assert_eq!(angle, ComponentValue::Value(StyleNumber::new(45.0)));

        let mut literal = ComponentValue::Value(transparent());
        assert!(map_color_component(&mut literal, &mut |_, _| None).is_some());
        let mut literal = ComponentValue::Value(LengthValue::Zero);
        assert!(map_length_component(&mut literal, &mut |_, _| None).is_some());
        let mut literal = ComponentValue::Value(StyleNumber::new(1.0));
        assert!(map_number_component(&mut literal, &mut |_, _| None).is_some());

        let wrong_color = StyleValue::Length(LengthValue::Zero);
        let wrong_scalar = StyleValue::Color(transparent());
        let mut color = component_reference(&name);
        assert!(map_color_component(&mut color, &mut |_, _| Some(wrong_color.clone())).is_none());
        let mut length = component_reference(&name);
        assert!(
            map_length_component(&mut length, &mut |_, _| Some(wrong_scalar.clone())).is_none()
        );
        let mut number = component_reference(&name);
        assert!(
            map_number_component(&mut number, &mut |_, _| Some(wrong_scalar.clone())).is_none()
        );
        let mut angle = component_reference(&name);
        assert!(map_angle_component(&mut angle, &mut |_, _| Some(wrong_scalar.clone())).is_none());

        let mut missing = component_reference::<ColorValue>(&name);
        assert!(map_color_component(&mut missing, &mut |_, _| None).is_none());
        let mut missing = component_reference::<LengthValue>(&name);
        assert!(map_length_component(&mut missing, &mut |_, _| None).is_none());
        let mut missing = component_reference::<StyleNumber>(&name);
        assert!(map_number_component(&mut missing, &mut |_, _| None).is_none());
    }

    #[test]
    fn background_walk_covers_images_geometry_and_shorthand_layers() {
        assert_maps_every_leaf(
            StyleValue::BackgroundImages(vec![
                BackgroundImageValue::Gradient(GradientValue::Linear {
                    angle_degrees: StyleNumber::new(0.0).into(),
                    stops: vec![stop()],
                }),
                BackgroundImageValue::Gradient(GradientValue::Radial {
                    shape: RadialGradientValue::EllipseSized(lp(1.0), lp(1.0)),
                    stops: vec![stop()],
                }),
                BackgroundImageValue::Gradient(GradientValue::Conic {
                    from_degrees: StyleNumber::new(0.0).into(),
                    center: BackgroundPositionValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                    stops: vec![stop()],
                }),
            ]),
            7,
        );
        assert_maps_every_leaf(
            StyleValue::Background(BackgroundValue {
                layers: vec![BackgroundLayerValue {
                    image: BackgroundImageValue::Gradient(GradientValue::Radial {
                        shape: RadialGradientValue::CircleSized(lp(1.0)),
                        stops: vec![stop()],
                    }),
                    position: BackgroundPositionValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                    size: BackgroundSizeValue::Explicit {
                        width: Some(lp(1.0)),
                        height: Some(lp(1.0)),
                    },
                    repeat: BackgroundRepeatValue {
                        horizontal: BackgroundRepeatModeValue::Repeat,
                        vertical: BackgroundRepeatModeValue::NoRepeat,
                    },
                    origin: BackgroundBoxValue::Padding,
                    clip: BackgroundBoxValue::Border,
                    attachment: BackgroundAttachmentValue::Scroll,
                }],
                color: ColorValue::Named("transparent".into()).into(),
            }),
            6,
        );
        assert_maps_every_leaf(
            StyleValue::BackgroundPosition(BackgroundPositionValue {
                horizontal: lp(1.0),
                vertical: lp(1.0),
            }),
            2,
        );
        assert_maps_every_leaf(
            StyleValue::BackgroundSize(BackgroundSizeValue::Explicit {
                width: Some(lp(1.0)),
                height: Some(lp(1.0)),
            }),
            2,
        );
    }

    #[test]
    fn transform_and_motion_path_walk_covers_every_length_percentage_branch() {
        assert_maps_every_leaf(
            StyleValue::Transform(TransformValue(vec![
                TransformFunctionValue::Translate(lp(1.0), lp(1.0)),
                TransformFunctionValue::TranslateX(lp(1.0)),
                TransformFunctionValue::TranslateY(lp(1.0)),
                TransformFunctionValue::Translate3d(
                    lp(1.0),
                    lp(1.0),
                    crate::LengthValue::Zero.into(),
                ),
            ])),
            6,
        );
        assert_maps_every_leaf(
            StyleValue::TransformOrigin(TransformOriginValue {
                horizontal: lp(1.0),
                vertical: lp(1.0),
            }),
            2,
        );
        assert_maps_every_leaf(
            StyleValue::OffsetPath(OffsetPathValue::Circle {
                radius: lp(1.0),
                center_x: lp(1.0),
                center_y: lp(1.0),
            }),
            3,
        );
        assert_maps_every_leaf(
            StyleValue::OffsetPath(OffsetPathValue::Ellipse {
                radius_x: lp(1.0),
                radius_y: lp(1.0),
                center_x: lp(1.0),
                center_y: lp(1.0),
            }),
            4,
        );
        assert_maps_every_leaf(
            StyleValue::OffsetPath(OffsetPathValue::Inset(Box::new(InsetPathValue {
                offsets: [lp(1.0), lp(1.0), lp(1.0), lp(1.0)],
                radii: Some([
                    BorderRadiusValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                    BorderRadiusValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                    BorderRadiusValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                    BorderRadiusValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                ]),
            }))),
            12,
        );
    }

    #[test]
    fn clip_path_walk_covers_every_shape_and_command_coordinate() {
        let point = || ClipPointValue {
            x: lp(1.0),
            y: lp(1.0),
        };
        let clip = |shape| {
            StyleValue::ClipPath(ClipPathValue::Shape {
                reference_box: ClipBoxValue::Border,
                shape,
            })
        };
        assert_maps_every_leaf(
            clip(ClipShapeValue::Inset {
                offsets: [lp(1.0), lp(1.0), lp(1.0), lp(1.0)],
                radii: Some([
                    BorderRadiusValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                    BorderRadiusValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                    BorderRadiusValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                    BorderRadiusValue {
                        horizontal: lp(1.0),
                        vertical: lp(1.0),
                    },
                ]),
            }),
            12,
        );
        assert_maps_every_leaf(
            clip(ClipShapeValue::Circle {
                radius: lp(1.0),
                center_x: lp(1.0),
                center_y: lp(1.0),
            }),
            3,
        );
        assert_maps_every_leaf(
            clip(ClipShapeValue::Ellipse {
                radius_x: lp(1.0),
                radius_y: lp(1.0),
                center_x: lp(1.0),
                center_y: lp(1.0),
            }),
            4,
        );
        assert_maps_every_leaf(
            clip(ClipShapeValue::Path {
                fill_rule: ClipFillRuleValue::NonZero,
                commands: vec![
                    ClipPathCommandValue::MoveTo(point()),
                    ClipPathCommandValue::LineTo(point()),
                    ClipPathCommandValue::QuadraticTo {
                        control: point(),
                        end: point(),
                    },
                    ClipPathCommandValue::CubicTo {
                        control_1: point(),
                        control_2: point(),
                        end: point(),
                    },
                    ClipPathCommandValue::Close,
                ],
            }),
            14,
        );
        assert_maps_every_leaf(StyleValue::ClipPath(ClipPathValue::None), 0);
    }

    #[test]
    fn grid_walk_covers_plain_repeated_and_implicit_tracks() {
        assert_maps_every_leaf(
            StyleValue::GridTemplate(GridTemplateValue {
                components: vec![
                    GridTemplateComponentValue::Track(track()),
                    GridTemplateComponentValue::Repeat(GridTemplateRepetitionValue {
                        count: GridRepetitionCountValue::Count(2),
                        tracks: vec![track()],
                        line_names: vec![Vec::new(), Vec::new()],
                    }),
                ],
                line_names: vec![Vec::new(), Vec::new(), Vec::new()],
            }),
            4,
        );
        assert_maps_every_leaf(StyleValue::GridTracks(vec![track()]), 2);
        assert_maps_every_leaf(
            StyleValue::GridTracks(vec![GridTrackSizingValue {
                min: GridMinTrackSizingValue::Auto,
                max: GridMaxTrackSizingValue::Fixed(lp(1.0)),
            }]),
            1,
        );
        assert_maps_every_leaf(
            StyleValue::GridTracks(vec![GridTrackSizingValue {
                min: GridMinTrackSizingValue::Auto,
                max: GridMaxTrackSizingValue::Fraction(StyleNumber::new(1.0)),
            }]),
            0,
        );
    }

    #[test]
    fn walk_covers_direct_wrappers_and_leafless_composite_variants() {
        for value in [
            StyleValue::LengthPercentage(lp(1.0)),
            StyleValue::Size(SizeValue::LengthPercentage(lp(1.0))),
            StyleValue::Size(SizeValue::FitContent(Some(lp(1.0)))),
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(lp(1.0))),
            StyleValue::FlexBasis(crate::FlexBasisValue::LengthPercentage(lp(1.0))),
            StyleValue::LineHeight(crate::LineHeightValue::LengthPercentage(lp(1.0))),
        ] {
            assert_maps_every_leaf(value, 1);
        }
        assert_maps_every_leaf(
            StyleValue::BorderRadius(BorderRadiusValue {
                horizontal: lp(1.0),
                vertical: lp(1.0),
            }),
            2,
        );
        for value in [
            StyleValue::Color(ColorValue::Named("red".into())),
            StyleValue::BackgroundImages(vec![BackgroundImageValue::None]),
            StyleValue::BackgroundImages(vec![BackgroundImageValue::Url("image.png".into())]),
            StyleValue::BackgroundImages(vec![BackgroundImageValue::Gradient(
                GradientValue::Radial {
                    shape: RadialGradientValue::Circle,
                    stops: Vec::new(),
                },
            )]),
            StyleValue::BackgroundImages(vec![BackgroundImageValue::Gradient(
                GradientValue::Radial {
                    shape: RadialGradientValue::Ellipse,
                    stops: Vec::new(),
                },
            )]),
            StyleValue::BackgroundImages(vec![BackgroundImageValue::Gradient(
                GradientValue::Linear {
                    angle_degrees: StyleNumber::new(0.0).into(),
                    stops: vec![GradientStopValue {
                        color: ColorValue::Named("red".into()).into(),
                        position: None,
                    }],
                },
            )]),
            StyleValue::BackgroundSize(BackgroundSizeValue::Auto),
            StyleValue::BackgroundSize(BackgroundSizeValue::Explicit {
                width: None,
                height: None,
            }),
            StyleValue::Transform(TransformValue(vec![TransformFunctionValue::Rotate(
                StyleNumber::new(45.0).into(),
            )])),
            StyleValue::OffsetPath(OffsetPathValue::None),
            StyleValue::OffsetPath(OffsetPathValue::Path(Vec::new())),
            StyleValue::OffsetPath(OffsetPathValue::Inset(Box::new(InsetPathValue {
                offsets: [lp(1.0), lp(1.0), lp(1.0), lp(1.0)],
                radii: None,
            }))),
        ] {
            let expected = if matches!(&value, StyleValue::OffsetPath(OffsetPathValue::Inset(_))) {
                4
            } else {
                0
            };
            assert_maps_every_leaf(value, expected);
        }
    }
}
