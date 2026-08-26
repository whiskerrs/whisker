//! Internal traversal of typed values that contain length-percentage leaves.

use crate::{
    BackgroundImageValue, BackgroundSizeValue, GradientValue, GridMaxTrackSizingValue,
    GridMinTrackSizingValue, GridTemplateComponentValue, LengthPercentageAutoValue,
    LengthPercentageValue, OffsetPathValue, RadialGradientValue, SizeValue, StyleValue,
    TransformFunctionValue,
};

/// Visits every length-percentage leaf nested in a specified value.
pub(crate) fn visit_length_percentages<'a>(
    value: &'a StyleValue,
    visit: &mut impl FnMut(&'a LengthPercentageValue),
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
    mut map: impl FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<StyleValue> {
    let mut mapped = value.clone();
    try_visit_length_percentages_mut(&mut mapped, &mut map)?;
    Some(mapped)
}

fn visit_background_image<'a>(
    image: &'a BackgroundImageValue,
    visit: &mut impl FnMut(&'a LengthPercentageValue),
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
    visit: &mut impl FnMut(&'a LengthPercentageValue),
) {
    for stop in stops {
        if let Some(position) = &stop.position {
            visit(position);
        }
    }
}

fn visit_background_size<'a>(
    size: &'a BackgroundSizeValue,
    visit: &mut impl FnMut(&'a LengthPercentageValue),
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
    visit: &mut impl FnMut(&'a LengthPercentageValue),
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
    visit: &mut impl FnMut(&'a LengthPercentageValue),
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

fn visit_grid_track<'a>(
    track: &'a crate::GridTrackSizingValue,
    visit: &mut impl FnMut(&'a LengthPercentageValue),
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
    map: &mut impl FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
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
    map: &mut impl FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<()> {
    *value = map(value)?;
    Some(())
}

fn map_background_image(
    image: &mut BackgroundImageValue,
    map: &mut impl FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
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
    map: &mut impl FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
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
    map: &mut impl FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
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
    map: &mut impl FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
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
    map: &mut impl FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
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

fn map_grid_track(
    track: &mut crate::GridTrackSizingValue,
    map: &mut impl FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
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
        BorderRadiusValue, ColorValue, GradientStopValue, GridRepetitionCountValue,
        GridTemplateRepetitionValue, GridTemplateValue, GridTrackSizingValue, InsetPathValue,
        StyleNumber, TransformOriginValue, TransformValue,
    };

    fn lp(value: f32) -> LengthPercentageValue {
        LengthPercentageValue::Percentage(StyleNumber::new(value))
    }

    fn stop() -> GradientStopValue {
        GradientStopValue {
            color: ColorValue::Named("red".into()),
            position: Some(lp(1.0)),
        }
    }

    fn track() -> GridTrackSizingValue {
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Fixed(lp(1.0)),
            max: GridMaxTrackSizingValue::FitContent(lp(1.0)),
        }
    }

    fn assert_maps_every_leaf(value: StyleValue, expected: usize) {
        let mut visited = 0;
        visit_length_percentages(&value, &mut |_| visited += 1);
        assert_eq!(visited, expected);

        let replacement = lp(99.0);
        let mapped = try_map_length_percentages(&value, |_| Some(replacement.clone())).unwrap();
        let mut mapped_count = 0;
        visit_length_percentages(&mapped, &mut |value| {
            mapped_count += 1;
            assert_eq!(value, &replacement);
        });
        assert_eq!(mapped_count, expected);
    }

    #[test]
    fn background_walk_covers_images_geometry_and_shorthand_layers() {
        assert_maps_every_leaf(
            StyleValue::BackgroundImages(vec![
                BackgroundImageValue::Gradient(GradientValue::Linear {
                    angle_degrees: StyleNumber::new(0.0),
                    stops: vec![stop()],
                }),
                BackgroundImageValue::Gradient(GradientValue::Radial {
                    shape: RadialGradientValue::EllipseSized(lp(1.0), lp(1.0)),
                    stops: vec![stop()],
                }),
                BackgroundImageValue::Gradient(GradientValue::Conic {
                    from_degrees: StyleNumber::new(0.0),
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
                color: ColorValue::Named("transparent".into()),
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
                TransformFunctionValue::Translate3d(lp(1.0), lp(1.0), crate::LengthValue::Zero),
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
    }
}
