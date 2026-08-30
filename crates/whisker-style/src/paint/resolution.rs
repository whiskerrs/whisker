use super::*;

pub(super) fn valid_offset_path(commands: &[MotionPathCommandValue]) -> bool {
    let mut current = None;
    let mut subpath_start = None;
    let mut total_length = 0.0_f32;
    for command in commands {
        match *command {
            MotionPathCommandValue::MoveTo(point) => {
                if !point.x.get().is_finite() || !point.y.get().is_finite() {
                    return false;
                }
                let point = (point.x.get(), point.y.get());
                current = Some(point);
                subpath_start = Some(point);
            }
            MotionPathCommandValue::LineTo(point) => {
                let Some(from) = current else {
                    return false;
                };
                if !point.x.get().is_finite() || !point.y.get().is_finite() {
                    return false;
                }
                let to = (point.x.get(), point.y.get());
                total_length += (to.0 - from.0).hypot(to.1 - from.1);
                current = Some(to);
            }
            MotionPathCommandValue::QuadraticTo { control, to } => {
                let Some(from) = current else {
                    return false;
                };
                if !control.x.get().is_finite()
                    || !control.y.get().is_finite()
                    || !to.x.get().is_finite()
                    || !to.y.get().is_finite()
                {
                    return false;
                }
                let control = (control.x.get(), control.y.get());
                let to = (to.x.get(), to.y.get());
                total_length += (control.0 - from.0).hypot(control.1 - from.1)
                    + (to.0 - control.0).hypot(to.1 - control.1);
                current = Some(to);
            }
            MotionPathCommandValue::CubicTo {
                control1,
                control2,
                to,
            } => {
                let Some(from) = current else {
                    return false;
                };
                if !control1.x.get().is_finite()
                    || !control1.y.get().is_finite()
                    || !control2.x.get().is_finite()
                    || !control2.y.get().is_finite()
                    || !to.x.get().is_finite()
                    || !to.y.get().is_finite()
                {
                    return false;
                }
                let control1 = (control1.x.get(), control1.y.get());
                let control2 = (control2.x.get(), control2.y.get());
                let to = (to.x.get(), to.y.get());
                total_length += (control1.0 - from.0).hypot(control1.1 - from.1)
                    + (control2.0 - control1.0).hypot(control2.1 - control1.1)
                    + (to.0 - control2.0).hypot(to.1 - control2.1);
                current = Some(to);
            }
            MotionPathCommandValue::ArcTo {
                radius_x,
                radius_y,
                x_axis_rotation,
                to,
                ..
            } => {
                let Some(from) = current else {
                    return false;
                };
                if !radius_x.get().is_finite()
                    || !radius_y.get().is_finite()
                    || !x_axis_rotation.get().is_finite()
                    || !to.x.get().is_finite()
                    || !to.y.get().is_finite()
                {
                    return false;
                }
                let to = (to.x.get(), to.y.get());
                total_length += (to.0 - from.0).hypot(to.1 - from.1);
                current = Some(to);
            }
            MotionPathCommandValue::Close => {
                let (Some(from), Some(to)) = (current, subpath_start) else {
                    return false;
                };
                total_length += (to.0 - from.0).hypot(to.1 - from.1);
                current = Some(to);
            }
        }
        if !total_length.is_finite() {
            return false;
        }
    }
    total_length > 0.0
}

pub(super) fn resolve_offset_path(
    value: &OffsetPathValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedOffsetPathValue, StyleResolutionError> {
    let resolve = |value: &LengthPercentageValue| {
        resolve_affine(value, inherited.font_size(), environment, property)
    };
    Ok(match value {
        OffsetPathValue::None => ComputedOffsetPathValue::None,
        OffsetPathValue::Path(commands) => {
            if !valid_offset_path(commands) {
                return Err(invalid(property));
            }
            ComputedOffsetPathValue::Path(commands.clone())
        }
        OffsetPathValue::Circle {
            radius,
            center_x,
            center_y,
        } => ComputedOffsetPathValue::Circle {
            radius: resolve(radius)?,
            center_x: resolve(center_x)?,
            center_y: resolve(center_y)?,
        },
        OffsetPathValue::Ellipse {
            radius_x,
            radius_y,
            center_x,
            center_y,
        } => ComputedOffsetPathValue::Ellipse {
            radius_x: resolve(radius_x)?,
            radius_y: resolve(radius_y)?,
            center_x: resolve(center_x)?,
            center_y: resolve(center_y)?,
        },
        OffsetPathValue::Inset(value) => {
            let [top, right, bottom, left] = &value.offsets;
            let radii = value
                .radii
                .as_ref()
                .map(|radii| {
                    let [top_left, top_right, bottom_right, bottom_left] = radii;
                    Ok(Corners {
                        top_left: resolve_radius_axes(
                            &top_left.horizontal,
                            &top_left.vertical,
                            inherited,
                            environment,
                            property,
                        )?,
                        top_right: resolve_radius_axes(
                            &top_right.horizontal,
                            &top_right.vertical,
                            inherited,
                            environment,
                            property,
                        )?,
                        bottom_right: resolve_radius_axes(
                            &bottom_right.horizontal,
                            &bottom_right.vertical,
                            inherited,
                            environment,
                            property,
                        )?,
                        bottom_left: resolve_radius_axes(
                            &bottom_left.horizontal,
                            &bottom_left.vertical,
                            inherited,
                            environment,
                            property,
                        )?,
                    })
                })
                .transpose()?;
            ComputedOffsetPathValue::Inset(Box::new(ComputedInsetPathValue {
                offsets: Edges {
                    top: resolve(top)?,
                    right: resolve(right)?,
                    bottom: resolve(bottom)?,
                    left: resolve(left)?,
                },
                radii,
            }))
        }
    })
}

pub(super) fn resolve_transform_functions(
    value: &TransformValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<Vec<ComputedTransformFunction>, StyleResolutionError> {
    let length_percentage = |value: &LengthPercentageValue| {
        resolve_affine(value, inherited.font_size(), environment, property)
    };
    let length = |value: &crate::LengthValue| {
        resolve_affine(
            &LengthPercentageValue::Length(*value),
            inherited.font_size(),
            environment,
            property,
        )
        .map(|value| StyleNumber::new(value.length()))
    };
    let finite = |value: StyleNumber| {
        if value.get().is_finite() {
            Ok(value)
        } else {
            Err(invalid(property))
        }
    };

    value
        .0
        .iter()
        .map(|function| {
            Ok(match function {
                TransformFunctionValue::Translate(x, y) => ComputedTransformFunction::Translate {
                    x: length_percentage(x)?,
                    y: length_percentage(y)?,
                    z: StyleNumber::new(0.0),
                },
                TransformFunctionValue::TranslateX(x) => ComputedTransformFunction::Translate {
                    x: length_percentage(x)?,
                    y: ComputedLengthPercentage::ZERO,
                    z: StyleNumber::new(0.0),
                },
                TransformFunctionValue::TranslateY(y) => ComputedTransformFunction::Translate {
                    x: ComputedLengthPercentage::ZERO,
                    y: length_percentage(y)?,
                    z: StyleNumber::new(0.0),
                },
                TransformFunctionValue::TranslateZ(z) => {
                    let z = length(component(z))?;
                    ComputedTransformFunction::Translate {
                        x: ComputedLengthPercentage::ZERO,
                        y: ComputedLengthPercentage::ZERO,
                        z,
                    }
                }
                TransformFunctionValue::Translate3d(x, y, z) => {
                    let z = length(component(z))?;
                    ComputedTransformFunction::Translate {
                        x: length_percentage(x)?,
                        y: length_percentage(y)?,
                        z,
                    }
                }
                TransformFunctionValue::Rotate(angle) | TransformFunctionValue::RotateZ(angle) => {
                    ComputedTransformFunction::RotateZ(finite(*component(angle))?)
                }
                TransformFunctionValue::RotateX(angle) => {
                    ComputedTransformFunction::RotateX(finite(*component(angle))?)
                }
                TransformFunctionValue::RotateY(angle) => {
                    ComputedTransformFunction::RotateY(finite(*component(angle))?)
                }
                TransformFunctionValue::Scale(x, y) => ComputedTransformFunction::Scale {
                    x: finite(*component(x))?,
                    y: finite(*component(y))?,
                    z: StyleNumber::new(1.0),
                },
                TransformFunctionValue::ScaleX(x) => ComputedTransformFunction::Scale {
                    x: finite(*component(x))?,
                    y: StyleNumber::new(1.0),
                    z: StyleNumber::new(1.0),
                },
                TransformFunctionValue::ScaleY(y) => ComputedTransformFunction::Scale {
                    x: StyleNumber::new(1.0),
                    y: finite(*component(y))?,
                    z: StyleNumber::new(1.0),
                },
                TransformFunctionValue::Skew(x, y) => ComputedTransformFunction::Skew {
                    x_degrees: finite(*component(x))?,
                    y_degrees: finite(*component(y))?,
                },
                TransformFunctionValue::SkewX(x) => ComputedTransformFunction::Skew {
                    x_degrees: finite(*component(x))?,
                    y_degrees: StyleNumber::new(0.0),
                },
                TransformFunctionValue::SkewY(y) => ComputedTransformFunction::Skew {
                    x_degrees: StyleNumber::new(0.0),
                    y_degrees: finite(*component(y))?,
                },
                TransformFunctionValue::Matrix(values) => {
                    if !values.iter().all(|value| value.get().is_finite()) {
                        return Err(invalid(property));
                    }
                    let [a, b, c, d, tx, ty] = *values;
                    ComputedTransformFunction::Matrix([
                        a,
                        b,
                        StyleNumber::new(0.0),
                        StyleNumber::new(0.0),
                        c,
                        d,
                        StyleNumber::new(0.0),
                        StyleNumber::new(0.0),
                        StyleNumber::new(0.0),
                        StyleNumber::new(0.0),
                        StyleNumber::new(1.0),
                        StyleNumber::new(0.0),
                        tx,
                        ty,
                        StyleNumber::new(0.0),
                        StyleNumber::new(1.0),
                    ])
                }
                TransformFunctionValue::Matrix3d(values) => {
                    if !values.iter().all(|value| value.get().is_finite()) {
                        return Err(invalid(property));
                    }
                    ComputedTransformFunction::Matrix(*values)
                }
            })
        })
        .collect()
}

pub(super) fn resolve_transform_origin(
    value: &TransformOriginValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<(ComputedLengthPercentage, ComputedLengthPercentage), StyleResolutionError> {
    Ok((
        resolve_affine(
            &value.horizontal,
            inherited.font_size(),
            environment,
            property,
        )?,
        resolve_affine(
            &value.vertical,
            inherited.font_size(),
            environment,
            property,
        )?,
    ))
}

pub(super) fn resolve_background_image(
    image: &BackgroundImageValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedBackgroundImage, StyleResolutionError> {
    Ok(match image {
        BackgroundImageValue::None => ComputedBackgroundImage::None,
        BackgroundImageValue::Url(url) if url.trim().is_empty() => return Err(invalid(property)),
        BackgroundImageValue::Url(url) => ComputedBackgroundImage::Url(url.clone()),
        BackgroundImageValue::Gradient(gradient) => ComputedBackgroundImage::Gradient(
            resolve_gradient(gradient, inherited, environment, property)?,
        ),
    })
}

pub(super) fn resolve_gradient(
    gradient: &GradientValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedGradient, StyleResolutionError> {
    let stops = |values: &[crate::GradientStopValue]| {
        if values.len() < 2 {
            return Err(invalid(property));
        }
        let mut resolved = values
            .iter()
            .map(|stop| {
                Ok(ComputedGradientStop {
                    color: color(&StyleValue::Color(component(&stop.color).clone()), property)?,
                    position: stop
                        .position
                        .as_ref()
                        .map(|position| {
                            resolve_length_percentage(
                                &StyleValue::LengthPercentage(position.clone()),
                                inherited,
                                environment,
                                property,
                            )
                        })
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>, StyleResolutionError>>()?;
        normalize_gradient_stops(&mut resolved);
        Ok(resolved)
    };
    Ok(match gradient {
        GradientValue::Linear {
            angle_degrees,
            stops: values,
        } => {
            let angle_degrees = component(angle_degrees);
            if !angle_degrees.get().is_finite() {
                return Err(invalid(property));
            }
            ComputedGradient::Linear {
                angle_degrees: *angle_degrees,
                stops: stops(values)?,
            }
        }
        GradientValue::Radial {
            shape,
            stops: values,
        } => {
            let resolve_radius = |value: &crate::LengthPercentageValue| {
                resolve_length_percentage(
                    &StyleValue::LengthPercentage(value.clone()),
                    inherited,
                    environment,
                    property,
                )
            };
            let (circle, radii) = match shape {
                RadialGradientValue::Circle => (true, None),
                RadialGradientValue::Ellipse => (false, None),
                RadialGradientValue::CircleSized(radius) => {
                    let radius = resolve_radius(radius)?;
                    (true, Some((radius, radius)))
                }
                RadialGradientValue::EllipseSized(x, y) => {
                    (false, Some((resolve_radius(x)?, resolve_radius(y)?)))
                }
            };
            ComputedGradient::Radial {
                circle,
                radii,
                stops: stops(values)?,
            }
        }
        GradientValue::Conic {
            from_degrees,
            center,
            stops: values,
        } => {
            let from_degrees = component(from_degrees);
            if !from_degrees.get().is_finite() {
                return Err(invalid(property));
            }
            ComputedGradient::Conic {
                from_degrees: *from_degrees,
                center: resolve_background_position(center, inherited, environment, property)?,
                stops: stops(values)?,
            }
        }
    })
}

pub(super) fn normalize_gradient_stops(stops: &mut [ComputedGradientStop]) {
    let last = stops.len() - 1;
    stops[0]
        .position
        .get_or_insert(ComputedLengthPercentage::ZERO);
    stops[last]
        .position
        .get_or_insert(ComputedLengthPercentage::new(0.0, 1.0));

    let mut start = 0;
    while start < last {
        let mut end = start + 1;
        while stops[end].position.is_none() {
            end += 1;
        }
        if end > start + 1 {
            let from = stops[start].position.unwrap();
            let to = stops[end].position.unwrap();
            let span = (end - start) as f32;
            for (offset, stop) in stops[(start + 1)..end].iter_mut().enumerate() {
                let progress = (offset + 1) as f32 / span;
                stop.position = Some(ComputedLengthPercentage::new(
                    from.length() + (to.length() - from.length()) * progress,
                    from.fraction() + (to.fraction() - from.fraction()) * progress,
                ));
            }
        }
        start = end;
    }
}

pub(super) fn color(
    value: &StyleValue,
    property: StyleProperty,
) -> Result<ColorValue, StyleResolutionError> {
    let StyleValue::Color(value) = value else {
        return Err(invalid(property));
    };
    crate::resolution::normalize_color_for(value, property)
}

pub(super) fn border_style(
    value: &StyleValue,
    property: StyleProperty,
) -> Result<BorderStyleValue, StyleResolutionError> {
    let StyleValue::BorderStyle(value) = value else {
        return Err(invalid(property));
    };
    Ok(*value)
}

pub(super) fn radius(
    value: &StyleValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedCornerRadius, StyleResolutionError> {
    let (horizontal, vertical) = match value {
        StyleValue::LengthPercentage(value) => (value, value),
        StyleValue::BorderRadius(value) => (&value.horizontal, &value.vertical),
        _ => return Err(invalid(property)),
    };
    resolve_radius_axes(horizontal, vertical, inherited, environment, property)
}

pub(super) fn resolve_radius_axes(
    horizontal: &LengthPercentageValue,
    vertical: &LengthPercentageValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedCornerRadius, StyleResolutionError> {
    let horizontal = resolve_affine(horizontal, inherited.font_size(), environment, property)?;
    let vertical = resolve_affine(vertical, inherited.font_size(), environment, property)?;
    if horizontal.length() < 0.0
        || horizontal.fraction() < 0.0
        || vertical.length() < 0.0
        || vertical.fraction() < 0.0
    {
        return Err(invalid(property));
    }
    Ok(ComputedCornerRadius {
        horizontal,
        vertical,
    })
}

pub(super) fn resolve_length_percentage(
    value: &StyleValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedLengthPercentage, StyleResolutionError> {
    let StyleValue::LengthPercentage(value) = value else {
        return Err(invalid(property));
    };
    resolve_affine(value, inherited.font_size(), environment, property)
}

pub(super) fn resolve_background_position(
    value: &BackgroundPositionValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedBackgroundPosition, StyleResolutionError> {
    Ok(ComputedBackgroundPosition {
        horizontal: resolve_affine(
            &value.horizontal,
            inherited.font_size(),
            environment,
            property,
        )?,
        vertical: resolve_affine(
            &value.vertical,
            inherited.font_size(),
            environment,
            property,
        )?,
    })
}

pub(super) fn resolve_background_size(
    value: &BackgroundSizeValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedBackgroundSize, StyleResolutionError> {
    let size = match value {
        BackgroundSizeValue::Auto => ComputedBackgroundSize::Auto,
        BackgroundSizeValue::Cover => ComputedBackgroundSize::Cover,
        BackgroundSizeValue::Contain => ComputedBackgroundSize::Contain,
        BackgroundSizeValue::Explicit { width, height } => {
            let resolve_axis = |value: &Option<_>| {
                value
                    .as_ref()
                    .map(|value| {
                        resolve_affine(value, inherited.font_size(), environment, property)
                    })
                    .transpose()
            };
            let width = resolve_axis(width)?;
            let height = resolve_axis(height)?;
            if width
                .into_iter()
                .chain(height)
                .any(|value| value.length() < 0.0 || value.fraction() < 0.0)
            {
                return Err(invalid(property));
            }
            if width.is_none() && height.is_none() {
                ComputedBackgroundSize::Auto
            } else {
                ComputedBackgroundSize::Explicit { width, height }
            }
        }
    };
    Ok(size)
}

pub(super) fn invalid(property: StyleProperty) -> StyleResolutionError {
    StyleResolutionError::InvalidPropertyValue(property)
}

pub(super) fn component<T>(value: &ComponentValue<T>) -> &T {
    value
        .value()
        .expect("custom-property components are materialized before paint resolution")
}
