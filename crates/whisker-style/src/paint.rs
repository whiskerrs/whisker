//! Computed paint values that remain independent of every Host renderer.

use crate::{
    BackdropFilterValue, BackgroundAttachmentValue, BackgroundBoxValue, BackgroundImageValue,
    BackgroundPositionValue, BackgroundRepeatModeValue, BackgroundSizeValue, BoxShadowValue,
    ClipBoxValue, ClipFillRuleValue, ClipPathCommandValue, ClipPathValue, ClipShapeValue,
    ColorValue, ComponentValue, ComputedLengthPercentage, DirectionValue, Edges, GradientValue,
    ImageRenderingValue, InheritedStyle, LengthPercentageValue, MotionPathCommandValue,
    OffsetPathValue, OffsetRotateValue, RadialGradientValue, SpecifiedStyle, StyleEnvironment,
    StyleNumber, StyleProperty, StyleResolutionError, StyleValue, TransformFunctionValue,
    TransformOriginValue, TransformValue, layout::resolve_affine,
};

mod resolution;

use resolution::*;

/// Four physical corners in top-left, top-right, bottom-right, bottom-left order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Corners<T> {
    /// Top-left corner.
    pub top_left: T,
    /// Top-right corner.
    pub top_right: T,
    /// Bottom-right corner.
    pub bottom_right: T,
    /// Bottom-left corner.
    pub bottom_left: T,
}

/// A computed border radius retaining both percentage axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedCornerRadius {
    /// Horizontal radius, resolved against border-box width by the renderer.
    pub horizontal: ComputedLengthPercentage,
    /// Vertical radius, resolved against border-box height by the renderer.
    pub vertical: ComputedLengthPercentage,
}

impl ComputedCornerRadius {
    const ZERO: Self = Self {
        horizontal: ComputedLengthPercentage::ZERO,
        vertical: ComputedLengthPercentage::ZERO,
    };
}

impl<T: Copy> Corners<T> {
    const fn all(value: T) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }
}

/// One box shadow after environment-dependent values are resolved.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedBoxShadow {
    /// Horizontal offset in logical pixels.
    pub offset_x: StyleNumber,
    /// Vertical offset in logical pixels.
    pub offset_y: StyleNumber,
    /// Non-negative blur radius in logical pixels.
    pub blur_radius: StyleNumber,
    /// Signed spread radius in logical pixels.
    pub spread_radius: StyleNumber,
    /// Shadow color.
    pub color: ColorValue,
    /// Paint inside the border box when true.
    pub inset: bool,
}

/// One point in a computed clip path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedClipPoint {
    /// Horizontal coordinate.
    pub x: ComputedLengthPercentage,
    /// Vertical coordinate.
    pub y: ComputedLengthPercentage,
}

/// One command in a computed clip path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ComputedClipPathCommand {
    /// Start a subpath.
    MoveTo(ComputedClipPoint),
    /// Add a line.
    LineTo(ComputedClipPoint),
    /// Add a quadratic Bezier segment.
    QuadraticTo {
        /// Control point.
        control: ComputedClipPoint,
        /// Endpoint.
        end: ComputedClipPoint,
    },
    /// Add a cubic Bezier segment.
    CubicTo {
        /// First control point.
        control_1: ComputedClipPoint,
        /// Second control point.
        control_2: ComputedClipPoint,
        /// Endpoint.
        end: ComputedClipPoint,
    },
    /// Close the current subpath.
    Close,
}

/// A computed basic shape retained until Host lowering.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ComputedClipShape {
    /// Inset rectangle.
    Inset {
        /// Top, right, bottom, and left offsets.
        offsets: Edges<ComputedLengthPercentage>,
        /// Per-corner radii.
        radii: Corners<ComputedCornerRadius>,
    },
    /// Circle.
    Circle {
        /// Radius.
        radius: ComputedLengthPercentage,
        /// Center.
        center: ComputedClipPoint,
    },
    /// Ellipse.
    Ellipse {
        /// Horizontal radius.
        radius_x: ComputedLengthPercentage,
        /// Vertical radius.
        radius_y: ComputedLengthPercentage,
        /// Center.
        center: ComputedClipPoint,
    },
    /// Structured path.
    Path {
        /// Fill rule.
        fill_rule: ClipFillRuleValue,
        /// Command stream.
        commands: Vec<ComputedClipPathCommand>,
    },
}

/// Computed `clip-path` with its reference box.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedClipPath {
    /// Reference box.
    pub reference_box: ClipBoxValue,
    /// Basic shape.
    pub shape: ComputedClipShape,
}

/// Renderer-independent border line style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderStyleValue {
    /// No border is painted.
    #[default]
    None,
    /// Hidden border, equivalent to none outside table conflict resolution.
    Hidden,
    /// One solid line.
    Solid,
    /// Dashed line.
    Dashed,
    /// Dotted line.
    Dotted,
    /// Two parallel lines.
    Double,
    /// Grooved 3-D line.
    Groove,
    /// Ridged 3-D line.
    Ridge,
    /// Inset 3-D line.
    Inset,
    /// Outset 3-D line.
    Outset,
}

/// Whether overflow on one axis is visible or clipped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OverflowValue {
    /// Descendant paint may extend outside the box.
    #[default]
    Visible,
    /// Descendant paint is clipped to the box.
    Hidden,
}

/// Whether a node participates in painting while retaining layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VisibilityValue {
    /// Paint the node normally.
    #[default]
    Visible,
    /// Do not paint the node, but retain its layout box.
    Hidden,
}

/// Computed position of one background layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedBackgroundPosition {
    /// Horizontal length plus positioning-area fraction.
    pub horizontal: ComputedLengthPercentage,
    /// Vertical length plus positioning-area fraction.
    pub vertical: ComputedLengthPercentage,
}

/// Computed size of one background layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedBackgroundSize {
    /// Use the image's intrinsic dimensions.
    Auto,
    /// Preserve aspect ratio while covering the positioning area.
    Cover,
    /// Preserve aspect ratio while fitting inside the positioning area.
    Contain,
    /// Resolve an explicit width and height against the positioning area.
    Explicit {
        /// Computed image width, or intrinsic width for `auto`.
        width: Option<ComputedLengthPercentage>,
        /// Computed image height, or intrinsic height for `auto`.
        height: Option<ComputedLengthPercentage>,
    },
}

/// One gradient stop after environment-dependent lengths are resolved.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedGradientStop {
    /// Stop color.
    pub color: ColorValue,
    /// Optional distance along the gradient line.
    pub position: Option<ComputedLengthPercentage>,
}

/// Renderer-independent computed procedural gradient.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ComputedGradient {
    /// Linear gradient.
    Linear {
        /// Direction in degrees clockwise from the positive vertical axis.
        angle_degrees: StyleNumber,
        /// Ordered stops.
        stops: Vec<ComputedGradientStop>,
    },
    /// Radial gradient.
    Radial {
        /// Circle or ellipse.
        circle: bool,
        /// Explicit radii, or `None` for farthest-corner sizing.
        radii: Option<(ComputedLengthPercentage, ComputedLengthPercentage)>,
        /// Ordered stops.
        stops: Vec<ComputedGradientStop>,
    },
    /// Conic gradient.
    Conic {
        /// Starting angle in degrees.
        from_degrees: StyleNumber,
        /// Center in the image box.
        center: ComputedBackgroundPosition,
        /// Ordered stops.
        stops: Vec<ComputedGradientStop>,
    },
}

/// One background image after style resolution but before Host resource policy.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ComputedBackgroundImage {
    /// Explicit empty layer.
    None,
    /// URL awaiting Host resource loading.
    Url(String),
    /// Procedural image ready for protocol lowering.
    Gradient(ComputedGradient),
}

/// One transform function after environment-dependent units are resolved.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ComputedTransformFunction {
    /// Three-axis translation. Horizontal and vertical percentages remain
    /// relative to the node's border box.
    Translate {
        /// Horizontal translation.
        x: ComputedLengthPercentage,
        /// Vertical translation.
        y: ComputedLengthPercentage,
        /// Depth translation in logical pixels.
        z: StyleNumber,
    },
    /// Rotation around the x axis in degrees.
    RotateX(StyleNumber),
    /// Rotation around the y axis in degrees.
    RotateY(StyleNumber),
    /// Rotation around the z axis in degrees.
    RotateZ(StyleNumber),
    /// Three-axis scale.
    Scale {
        /// Horizontal scale.
        x: StyleNumber,
        /// Vertical scale.
        y: StyleNumber,
        /// Depth scale.
        z: StyleNumber,
    },
    /// Two-axis skew in degrees.
    Skew {
        /// Horizontal skew.
        x_degrees: StyleNumber,
        /// Vertical skew.
        y_degrees: StyleNumber,
    },
    /// A fully specified column-major matrix.
    Matrix([StyleNumber; 16]),
}

/// Border-box-resolvable motion path retained by computed style.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum ComputedOffsetPathValue {
    /// Disable motion-path positioning.
    #[default]
    None,
    /// Follow an absolute SVG path.
    Path(Vec<MotionPathCommandValue>),
    /// Follow a circle relative to the node border box.
    Circle {
        /// Radius; percentages use the normalized box diagonal.
        radius: ComputedLengthPercentage,
        /// Horizontal center relative to box width.
        center_x: ComputedLengthPercentage,
        /// Vertical center relative to box height.
        center_y: ComputedLengthPercentage,
    },
    /// Follow an ellipse relative to the node border box.
    Ellipse {
        /// Horizontal radius relative to box width.
        radius_x: ComputedLengthPercentage,
        /// Vertical radius relative to box height.
        radius_y: ComputedLengthPercentage,
        /// Horizontal center relative to box width.
        center_x: ComputedLengthPercentage,
        /// Vertical center relative to box height.
        center_y: ComputedLengthPercentage,
    },
    /// Follow a possibly-rounded rectangle relative to the node border box.
    Inset(Box<ComputedInsetPathValue>),
}

/// Border-box-resolvable `inset()` motion path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedInsetPathValue {
    /// Physical offsets from the border-box edges.
    pub offsets: Edges<ComputedLengthPercentage>,
    /// Optional physical corner radii.
    pub radii: Option<Corners<ComputedCornerRadius>>,
}

/// Transform functions and origin retained until border-box layout is known.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedTransformStyle {
    /// Lynx-compatible perspective distance applied to this node, or none.
    pub perspective: Option<StyleNumber>,
    /// Motion path followed by the current node.
    pub offset_path: ComputedOffsetPathValue,
    /// Normalized progress along `offset_path`.
    pub offset_distance: StyleNumber,
    /// Tangent-following or fixed motion-path rotation.
    pub offset_rotate: OffsetRotateValue,
    /// Ordered transform functions.
    pub functions: Vec<ComputedTransformFunction>,
    /// Horizontal origin relative to border-box width.
    pub origin_x: ComputedLengthPercentage,
    /// Vertical origin relative to border-box height.
    pub origin_y: ComputedLengthPercentage,
}

impl Default for ComputedTransformStyle {
    fn default() -> Self {
        Self {
            perspective: None,
            offset_path: ComputedOffsetPathValue::None,
            offset_distance: StyleNumber::new(0.0),
            offset_rotate: OffsetRotateValue::Auto,
            functions: Vec::new(),
            origin_x: ComputedLengthPercentage::new(0.0, 0.5),
            origin_y: ComputedLengthPercentage::new(0.0, 0.5),
        }
    }
}

/// Computed geometry and scrolling values paired with one background image.
///
/// The authoring API currently exposes one scalar set of longhands. Keeping
/// them grouped makes later CSS list alignment an expansion from one layer to
/// many rather than a collection of unrelated parallel fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedBackgroundLayerStyle {
    /// Position within the selected origin box.
    pub position: ComputedBackgroundPosition,
    /// Image sizing behavior.
    pub size: ComputedBackgroundSize,
    /// Horizontal tiling behavior.
    pub repeat_x: BackgroundRepeatModeValue,
    /// Vertical tiling behavior.
    pub repeat_y: BackgroundRepeatModeValue,
    /// Box defining the positioning area.
    pub origin: BackgroundBoxValue,
    /// Box clipping background paint.
    pub clip: BackgroundBoxValue,
    /// Relationship to scrolling.
    pub attachment: BackgroundAttachmentValue,
}

impl Default for ComputedBackgroundLayerStyle {
    fn default() -> Self {
        Self {
            position: ComputedBackgroundPosition {
                horizontal: ComputedLengthPercentage::ZERO,
                vertical: ComputedLengthPercentage::ZERO,
            },
            size: ComputedBackgroundSize::Auto,
            repeat_x: BackgroundRepeatModeValue::Repeat,
            repeat_y: BackgroundRepeatModeValue::Repeat,
            origin: BackgroundBoxValue::Padding,
            clip: BackgroundBoxValue::Border,
            attachment: BackgroundAttachmentValue::Scroll,
        }
    }
}

/// Computed background, border, clip, and compositing values for one node.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedPaintStyle {
    /// Blur radius applied to pixels behind this node, or `None` for no effect.
    pub backdrop_blur: Option<StyleNumber>,
    /// Raster-image sampling behavior for images painted by this node.
    pub image_rendering: ImageRenderingValue,
    /// Ordered box shadows, front to back.
    pub box_shadows: Vec<ComputedBoxShadow>,
    /// Optional basic-shape clip.
    pub clip_path: Option<ComputedClipPath>,
    /// Resolved background color. Transparent is represented explicitly.
    pub background_color: ColorValue,
    /// Ordered Host-independent background image sources, front to back.
    pub background_images: Vec<ComputedBackgroundImage>,
    /// Per-layer geometry aligned with `background_images` using CSS list cycling.
    pub background_layers: Vec<ComputedBackgroundLayerStyle>,
    /// Resolved border colors in physical edge order.
    pub border_colors: Edges<ColorValue>,
    /// Border line styles in physical edge order.
    pub border_styles: Edges<BorderStyleValue>,
    /// Corner radii retaining their border-box percentage component.
    pub border_radii: Corners<ComputedCornerRadius>,
    /// Transform retained until the border-box size is known.
    pub transform: ComputedTransformStyle,
    /// Group opacity, clamped to `0.0..=1.0`.
    pub opacity: StyleNumber,
    /// Paint visibility.
    pub visibility: VisibilityValue,
    /// Horizontal overflow behavior.
    pub overflow_x: OverflowValue,
    /// Vertical overflow behavior.
    pub overflow_y: OverflowValue,
    /// Sibling stacking key.
    pub z_index: i32,
}

impl ComputedPaintStyle {
    fn initial(current_color: &ColorValue) -> Self {
        let transparent = ColorValue::Rgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: StyleNumber::new(0.0),
        };
        Self {
            backdrop_blur: None,
            image_rendering: ImageRenderingValue::Auto,
            box_shadows: Vec::new(),
            clip_path: None,
            background_color: transparent,
            background_images: Vec::new(),
            background_layers: vec![ComputedBackgroundLayerStyle::default()],
            border_colors: Edges {
                top: current_color.clone(),
                right: current_color.clone(),
                bottom: current_color.clone(),
                left: current_color.clone(),
            },
            border_styles: Edges {
                top: BorderStyleValue::None,
                right: BorderStyleValue::None,
                bottom: BorderStyleValue::None,
                left: BorderStyleValue::None,
            },
            border_radii: Corners::all(ComputedCornerRadius::ZERO),
            transform: ComputedTransformStyle::default(),
            opacity: StyleNumber::new(1.0),
            visibility: VisibilityValue::Visible,
            overflow_x: OverflowValue::Visible,
            overflow_y: OverflowValue::Visible,
            z_index: 0,
        }
    }

    /// Returns paint invalidation when any computed presentation value changed.
    pub fn changes_from(&self, previous: &Self) -> crate::PropertyImpactSet {
        if self == previous {
            crate::PropertyImpactSet::EMPTY
        } else {
            crate::PropertyImpactSet::PAINT
        }
    }
}

fn resolve_shadow_length(
    value: &ComponentValue<crate::LengthValue>,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<f32, StyleResolutionError> {
    Ok(resolve_affine(
        &LengthPercentageValue::Length(*component(value)),
        inherited.font_size(),
        environment,
        property,
    )?
    .length())
}

fn resolve_box_shadow(
    value: &BoxShadowValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedBoxShadow, StyleResolutionError> {
    let blur_radius = resolve_shadow_length(&value.blur_radius, inherited, environment, property)?;
    if blur_radius < 0.0 {
        return Err(invalid(property));
    }
    Ok(ComputedBoxShadow {
        offset_x: StyleNumber::new(resolve_shadow_length(
            &value.offset_x,
            inherited,
            environment,
            property,
        )?),
        offset_y: StyleNumber::new(resolve_shadow_length(
            &value.offset_y,
            inherited,
            environment,
            property,
        )?),
        blur_radius: StyleNumber::new(blur_radius),
        spread_radius: StyleNumber::new(resolve_shadow_length(
            &value.spread_radius,
            inherited,
            environment,
            property,
        )?),
        color: component(&value.color).clone(),
        inset: value.inset,
    })
}

fn resolve_clip_path(
    value: &ClipPathValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<Option<ComputedClipPath>, StyleResolutionError> {
    let ClipPathValue::Shape {
        reference_box,
        shape,
    } = value
    else {
        return Ok(None);
    };
    let length = |value: &LengthPercentageValue| {
        resolve_affine(value, inherited.font_size(), environment, property)
    };
    let point = |value: &crate::ClipPointValue| {
        Ok(ComputedClipPoint {
            x: length(&value.x)?,
            y: length(&value.y)?,
        })
    };
    let shape = match shape {
        ClipShapeValue::Inset { offsets, radii } => {
            let offsets = Edges {
                top: length(&offsets[0])?,
                right: length(&offsets[1])?,
                bottom: length(&offsets[2])?,
                left: length(&offsets[3])?,
            };
            let radii = match radii {
                Some(radii) => Corners {
                    top_left: resolve_radius_axes(
                        &radii[0].horizontal,
                        &radii[0].vertical,
                        inherited,
                        environment,
                        property,
                    )?,
                    top_right: resolve_radius_axes(
                        &radii[1].horizontal,
                        &radii[1].vertical,
                        inherited,
                        environment,
                        property,
                    )?,
                    bottom_right: resolve_radius_axes(
                        &radii[2].horizontal,
                        &radii[2].vertical,
                        inherited,
                        environment,
                        property,
                    )?,
                    bottom_left: resolve_radius_axes(
                        &radii[3].horizontal,
                        &radii[3].vertical,
                        inherited,
                        environment,
                        property,
                    )?,
                },
                None => Corners::all(ComputedCornerRadius::ZERO),
            };
            ComputedClipShape::Inset { offsets, radii }
        }
        ClipShapeValue::Circle {
            radius,
            center_x,
            center_y,
        } => ComputedClipShape::Circle {
            radius: length(radius)?,
            center: ComputedClipPoint {
                x: length(center_x)?,
                y: length(center_y)?,
            },
        },
        ClipShapeValue::Ellipse {
            radius_x,
            radius_y,
            center_x,
            center_y,
        } => ComputedClipShape::Ellipse {
            radius_x: length(radius_x)?,
            radius_y: length(radius_y)?,
            center: ComputedClipPoint {
                x: length(center_x)?,
                y: length(center_y)?,
            },
        },
        ClipShapeValue::Path {
            fill_rule,
            commands,
        } => ComputedClipShape::Path {
            fill_rule: *fill_rule,
            commands: commands
                .iter()
                .map(|command| {
                    Ok(match command {
                        ClipPathCommandValue::MoveTo(value) => {
                            ComputedClipPathCommand::MoveTo(point(value)?)
                        }
                        ClipPathCommandValue::LineTo(value) => {
                            ComputedClipPathCommand::LineTo(point(value)?)
                        }
                        ClipPathCommandValue::QuadraticTo { control, end } => {
                            ComputedClipPathCommand::QuadraticTo {
                                control: point(control)?,
                                end: point(end)?,
                            }
                        }
                        ClipPathCommandValue::CubicTo {
                            control_1,
                            control_2,
                            end,
                        } => ComputedClipPathCommand::CubicTo {
                            control_1: point(control_1)?,
                            control_2: point(control_2)?,
                            end: point(end)?,
                        },
                        ClipPathCommandValue::Close => ComputedClipPathCommand::Close,
                    })
                })
                .collect::<Result<Vec<_>, StyleResolutionError>>()?,
        },
    };
    Ok(Some(ComputedClipPath {
        reference_box: *reference_box,
        shape,
    }))
}

pub(crate) fn resolve_paint_style(
    specified: &SpecifiedStyle,
    inherited: &InheritedStyle,
    direction: DirectionValue,
    environment: StyleEnvironment,
) -> Result<ComputedPaintStyle, StyleResolutionError> {
    let mut paint = ComputedPaintStyle::initial(inherited.color());
    for declaration in specified.resolved() {
        let property = declaration.property();
        let value = declaration.value();
        match property {
            StyleProperty::ImageRendering => {
                let StyleValue::ImageRendering(value) = value else {
                    return Err(invalid(property));
                };
                paint.image_rendering = *value;
            }
            StyleProperty::BackdropFilter => {
                let StyleValue::BackdropFilter(value) = value else {
                    return Err(invalid(property));
                };
                paint.backdrop_blur = match value {
                    BackdropFilterValue::None => None,
                    BackdropFilterValue::Blur(radius) => {
                        let radius = component(radius);
                        let radius = resolve_affine(
                            &LengthPercentageValue::Length(*radius),
                            inherited.font_size(),
                            environment,
                            property,
                        )?
                        .length();
                        if radius < 0.0 {
                            return Err(invalid(property));
                        }
                        Some(StyleNumber::new(radius))
                    }
                };
            }
            StyleProperty::BoxShadow => {
                let StyleValue::BoxShadows(values) = value else {
                    return Err(invalid(property));
                };
                paint.box_shadows = values
                    .iter()
                    .map(|value| resolve_box_shadow(value, inherited, environment, property))
                    .collect::<Result<_, _>>()?;
            }
            StyleProperty::ClipPath => {
                let StyleValue::ClipPath(value) = value else {
                    return Err(invalid(property));
                };
                paint.clip_path = resolve_clip_path(value, inherited, environment, property)?;
            }
            StyleProperty::Transform => {
                let StyleValue::Transform(value) = value else {
                    return Err(invalid(property));
                };
                paint.transform.functions =
                    resolve_transform_functions(value, inherited, environment, property)?;
            }
            StyleProperty::Perspective => {
                let StyleValue::Length(value) = value else {
                    return Err(invalid(property));
                };
                let distance = resolve_affine(
                    &LengthPercentageValue::Length(*value),
                    inherited.font_size(),
                    environment,
                    property,
                )?
                .length();
                if distance < 0.0 {
                    return Err(invalid(property));
                }
                paint.transform.perspective = Some(StyleNumber::new(distance));
            }
            StyleProperty::OffsetPath => {
                let StyleValue::OffsetPath(value) = value else {
                    return Err(invalid(property));
                };
                paint.transform.offset_path =
                    resolve_offset_path(value, inherited, environment, property)?;
            }
            StyleProperty::OffsetDistance => {
                let distance = match value {
                    StyleValue::Number(value) => value.get(),
                    StyleValue::LengthPercentage(LengthPercentageValue::Percentage(value)) => {
                        value.get() / 100.0
                    }
                    _ => return Err(invalid(property)),
                };
                if !distance.is_finite() || !(0.0..=1.0).contains(&distance) {
                    return Err(invalid(property));
                }
                paint.transform.offset_distance = StyleNumber::new(distance);
            }
            StyleProperty::OffsetRotate => {
                let StyleValue::OffsetRotate(value) = value else {
                    return Err(invalid(property));
                };
                if matches!(value, OffsetRotateValue::Angle(angle) if !angle.get().is_finite()) {
                    return Err(invalid(property));
                }
                paint.transform.offset_rotate = *value;
            }
            StyleProperty::TransformOrigin => {
                let StyleValue::TransformOrigin(value) = value else {
                    return Err(invalid(property));
                };
                let (horizontal, vertical) =
                    resolve_transform_origin(value, inherited, environment, property)?;
                paint.transform.origin_x = horizontal;
                paint.transform.origin_y = vertical;
            }
            StyleProperty::Background => {
                let StyleValue::Background(value) = value else {
                    return Err(invalid(property));
                };
                paint.background_color = component(&value.color).clone();
                paint.background_images = value
                    .layers
                    .iter()
                    .map(|layer| {
                        resolve_background_image(&layer.image, inherited, environment, property)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                paint.background_layers = value
                    .layers
                    .iter()
                    .map(|layer| {
                        Ok(ComputedBackgroundLayerStyle {
                            position: resolve_background_position(
                                &layer.position,
                                inherited,
                                environment,
                                property,
                            )?,
                            size: resolve_background_size(
                                &layer.size,
                                inherited,
                                environment,
                                property,
                            )?,
                            repeat_x: layer.repeat.horizontal,
                            repeat_y: layer.repeat.vertical,
                            origin: layer.origin,
                            clip: layer.clip,
                            attachment: layer.attachment,
                        })
                    })
                    .collect::<Result<Vec<_>, StyleResolutionError>>()?;
                if paint.background_layers.is_empty() {
                    paint
                        .background_layers
                        .push(ComputedBackgroundLayerStyle::default());
                }
            }
            StyleProperty::BackgroundColor => {
                paint.background_color = color(value, property)?;
            }
            StyleProperty::BackgroundImage => {
                let StyleValue::BackgroundImages(images) = value else {
                    return Err(invalid(property));
                };
                paint.background_images = images
                    .iter()
                    .map(|image| resolve_background_image(image, inherited, environment, property))
                    .collect::<Result<Vec<_>, _>>()?;
            }
            StyleProperty::BackgroundRepeat => {
                let StyleValue::BackgroundRepeat(value) = value else {
                    return Err(invalid(property));
                };
                for layer in &mut paint.background_layers {
                    layer.repeat_x = value.horizontal;
                    layer.repeat_y = value.vertical;
                }
            }
            StyleProperty::BackgroundPosition => {
                let StyleValue::BackgroundPosition(value) = value else {
                    return Err(invalid(property));
                };
                let position =
                    resolve_background_position(value, inherited, environment, property)?;
                for layer in &mut paint.background_layers {
                    layer.position = position;
                }
            }
            StyleProperty::BackgroundPositionX => {
                let horizontal =
                    resolve_length_percentage(value, inherited, environment, property)?;
                for layer in &mut paint.background_layers {
                    layer.position.horizontal = horizontal;
                }
            }
            StyleProperty::BackgroundPositionY => {
                let vertical = resolve_length_percentage(value, inherited, environment, property)?;
                for layer in &mut paint.background_layers {
                    layer.position.vertical = vertical;
                }
            }
            StyleProperty::BackgroundSize => {
                let StyleValue::BackgroundSize(value) = value else {
                    return Err(invalid(property));
                };
                let size = resolve_background_size(value, inherited, environment, property)?;
                for layer in &mut paint.background_layers {
                    layer.size = size;
                }
            }
            StyleProperty::BackgroundOrigin => {
                let StyleValue::BackgroundBox(value) = value else {
                    return Err(invalid(property));
                };
                for layer in &mut paint.background_layers {
                    layer.origin = *value;
                }
            }
            StyleProperty::BackgroundClip => {
                let StyleValue::BackgroundBox(value) = value else {
                    return Err(invalid(property));
                };
                for layer in &mut paint.background_layers {
                    layer.clip = *value;
                }
            }
            StyleProperty::BackgroundAttachment => {
                let StyleValue::BackgroundAttachment(value) = value else {
                    return Err(invalid(property));
                };
                for layer in &mut paint.background_layers {
                    layer.attachment = *value;
                }
            }
            StyleProperty::BorderTopColor => paint.border_colors.top = color(value, property)?,
            StyleProperty::BorderRightColor => paint.border_colors.right = color(value, property)?,
            StyleProperty::BorderBottomColor => {
                paint.border_colors.bottom = color(value, property)?;
            }
            StyleProperty::BorderLeftColor => paint.border_colors.left = color(value, property)?,
            StyleProperty::BorderInlineStartColor if direction == DirectionValue::Ltr => {
                paint.border_colors.left = color(value, property)?;
            }
            StyleProperty::BorderInlineStartColor => {
                paint.border_colors.right = color(value, property)?;
            }
            StyleProperty::BorderInlineEndColor if direction == DirectionValue::Ltr => {
                paint.border_colors.right = color(value, property)?;
            }
            StyleProperty::BorderInlineEndColor => {
                paint.border_colors.left = color(value, property)?;
            }
            StyleProperty::BorderTopStyle => {
                paint.border_styles.top = border_style(value, property)?
            }
            StyleProperty::BorderRightStyle => {
                paint.border_styles.right = border_style(value, property)?;
            }
            StyleProperty::BorderBottomStyle => {
                paint.border_styles.bottom = border_style(value, property)?;
            }
            StyleProperty::BorderLeftStyle => {
                paint.border_styles.left = border_style(value, property)?;
            }
            StyleProperty::BorderInlineStartStyle if direction == DirectionValue::Ltr => {
                paint.border_styles.left = border_style(value, property)?;
            }
            StyleProperty::BorderInlineStartStyle => {
                paint.border_styles.right = border_style(value, property)?;
            }
            StyleProperty::BorderInlineEndStyle if direction == DirectionValue::Ltr => {
                paint.border_styles.right = border_style(value, property)?;
            }
            StyleProperty::BorderInlineEndStyle => {
                paint.border_styles.left = border_style(value, property)?;
            }
            StyleProperty::BorderTopLeftRadius => {
                paint.border_radii.top_left = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderTopRightRadius => {
                paint.border_radii.top_right = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderBottomRightRadius => {
                paint.border_radii.bottom_right = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderBottomLeftRadius => {
                paint.border_radii.bottom_left = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderStartStartRadius if direction == DirectionValue::Ltr => {
                paint.border_radii.top_left = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderStartStartRadius => {
                paint.border_radii.top_right = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderStartEndRadius if direction == DirectionValue::Ltr => {
                paint.border_radii.top_right = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderStartEndRadius => {
                paint.border_radii.top_left = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderEndStartRadius if direction == DirectionValue::Ltr => {
                paint.border_radii.bottom_left = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderEndStartRadius => {
                paint.border_radii.bottom_right = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderEndEndRadius if direction == DirectionValue::Ltr => {
                paint.border_radii.bottom_right = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderEndEndRadius => {
                paint.border_radii.bottom_left = radius(value, inherited, environment, property)?;
            }
            StyleProperty::Opacity => {
                let StyleValue::Number(value) = value else {
                    return Err(invalid(property));
                };
                let value = value.get();
                if !value.is_finite() {
                    return Err(invalid(property));
                }
                paint.opacity = StyleNumber::new(value.clamp(0.0, 1.0));
            }
            StyleProperty::Visibility => {
                let StyleValue::Visibility(value) = value else {
                    return Err(invalid(property));
                };
                paint.visibility = *value;
            }
            StyleProperty::OverflowX => {
                let StyleValue::Overflow(value) = value else {
                    return Err(invalid(property));
                };
                paint.overflow_x = *value;
            }
            StyleProperty::OverflowY => {
                let StyleValue::Overflow(value) = value else {
                    return Err(invalid(property));
                };
                paint.overflow_y = *value;
            }
            StyleProperty::ZIndex => {
                let StyleValue::Integer(value) = value else {
                    return Err(invalid(property));
                };
                paint.z_index = i32::try_from(*value).map_err(|_| invalid(property))?;
            }
            _ => {}
        }
    }
    Ok(paint)
}

#[cfg(test)]
mod tests;
