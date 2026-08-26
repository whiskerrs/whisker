//! Computed paint values that remain independent of every Host renderer.

use crate::{
    BackdropFilterValue, BackgroundAttachmentValue, BackgroundBoxValue, BackgroundImageValue,
    BackgroundPositionValue, BackgroundRepeatModeValue, BackgroundSizeValue, ColorValue,
    ComputedLengthPercentage, DirectionValue, Edges, GradientValue, InheritedStyle,
    LengthPercentageValue, MotionPathCommandValue, OffsetPathValue, OffsetRotateValue,
    RadialGradientValue, SpecifiedStyle, StyleEnvironment, StyleNumber, StyleProperty,
    StyleResolutionError, StyleValue, TransformFunctionValue, TransformOriginValue, TransformValue,
    layout::resolve_affine,
};

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
            StyleProperty::BackdropFilter => {
                let StyleValue::BackdropFilter(value) = value else {
                    return Err(invalid(property));
                };
                paint.backdrop_blur = match value {
                    BackdropFilterValue::None => None,
                    BackdropFilterValue::Blur(radius) => {
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
                paint.background_color = value.color.clone();
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

fn valid_offset_path(commands: &[MotionPathCommandValue]) -> bool {
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

fn resolve_offset_path(
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

fn resolve_transform_functions(
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
                    let z = length(z)?;
                    ComputedTransformFunction::Translate {
                        x: ComputedLengthPercentage::ZERO,
                        y: ComputedLengthPercentage::ZERO,
                        z,
                    }
                }
                TransformFunctionValue::Translate3d(x, y, z) => {
                    let z = length(z)?;
                    ComputedTransformFunction::Translate {
                        x: length_percentage(x)?,
                        y: length_percentage(y)?,
                        z,
                    }
                }
                TransformFunctionValue::Rotate(angle) | TransformFunctionValue::RotateZ(angle) => {
                    ComputedTransformFunction::RotateZ(finite(*angle)?)
                }
                TransformFunctionValue::RotateX(angle) => {
                    ComputedTransformFunction::RotateX(finite(*angle)?)
                }
                TransformFunctionValue::RotateY(angle) => {
                    ComputedTransformFunction::RotateY(finite(*angle)?)
                }
                TransformFunctionValue::Scale(x, y) => ComputedTransformFunction::Scale {
                    x: finite(*x)?,
                    y: finite(*y)?,
                    z: StyleNumber::new(1.0),
                },
                TransformFunctionValue::ScaleX(x) => ComputedTransformFunction::Scale {
                    x: finite(*x)?,
                    y: StyleNumber::new(1.0),
                    z: StyleNumber::new(1.0),
                },
                TransformFunctionValue::ScaleY(y) => ComputedTransformFunction::Scale {
                    x: StyleNumber::new(1.0),
                    y: finite(*y)?,
                    z: StyleNumber::new(1.0),
                },
                TransformFunctionValue::Skew(x, y) => ComputedTransformFunction::Skew {
                    x_degrees: finite(*x)?,
                    y_degrees: finite(*y)?,
                },
                TransformFunctionValue::SkewX(x) => ComputedTransformFunction::Skew {
                    x_degrees: finite(*x)?,
                    y_degrees: StyleNumber::new(0.0),
                },
                TransformFunctionValue::SkewY(y) => ComputedTransformFunction::Skew {
                    x_degrees: StyleNumber::new(0.0),
                    y_degrees: finite(*y)?,
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

fn resolve_transform_origin(
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

fn resolve_background_image(
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

fn resolve_gradient(
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
                    color: color(&StyleValue::Color(stop.color.clone()), property)?,
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

fn normalize_gradient_stops(stops: &mut [ComputedGradientStop]) {
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

fn color(value: &StyleValue, property: StyleProperty) -> Result<ColorValue, StyleResolutionError> {
    let StyleValue::Color(value) = value else {
        return Err(invalid(property));
    };
    crate::resolution::normalize_color_for(value, property)
}

fn border_style(
    value: &StyleValue,
    property: StyleProperty,
) -> Result<BorderStyleValue, StyleResolutionError> {
    let StyleValue::BorderStyle(value) = value else {
        return Err(invalid(property));
    };
    Ok(*value)
}

fn radius(
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

fn resolve_radius_axes(
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

fn resolve_length_percentage(
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

fn resolve_background_position(
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

fn resolve_background_size(
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

fn invalid(property: StyleProperty) -> StyleResolutionError {
    StyleResolutionError::InvalidPropertyValue(property)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BackdropFilterValue, BackgroundLayerValue, BackgroundRepeatValue, BackgroundValue,
        BorderRadiusValue, GradientStopValue, LengthPercentageValue, LengthUnit, LengthValue,
    };

    fn number(value: f32) -> StyleNumber {
        StyleNumber::new(value)
    }

    fn px_length(value: f32) -> LengthPercentageValue {
        LengthPercentageValue::Length(LengthValue::Dimension {
            value: number(value),
            unit: LengthUnit::Px,
        })
    }

    fn px(value: f32) -> StyleValue {
        StyleValue::LengthPercentage(px_length(value))
    }

    fn percentage(value: f32) -> LengthPercentageValue {
        LengthPercentageValue::Percentage(number(value))
    }

    #[test]
    fn backdrop_blur_resolves_relative_lengths_and_rejects_negative_radii() {
        let blur = |value| {
            StyleValue::BackdropFilter(BackdropFilterValue::Blur(LengthValue::Dimension {
                value: number(value),
                unit: LengthUnit::Rem,
            }))
        };
        let resolved = crate::resolve_style(
            &SpecifiedStyle::new().push(StyleProperty::BackdropFilter, blur(2.0)),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            resolved.computed().paint().backdrop_blur,
            Some(number(28.0))
        );
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(StyleProperty::BackdropFilter, blur(-1.0)),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::BackdropFilter
            ))
        );
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::BackdropFilter,
                    StyleValue::BackdropFilter(BackdropFilterValue::None),
                ),
                None,
                StyleEnvironment::default(),
            )
            .unwrap()
            .computed()
            .paint()
            .backdrop_blur,
            None
        );
        for value in [
            StyleValue::Number(number(1.0)),
            StyleValue::BackdropFilter(BackdropFilterValue::Blur(LengthValue::Dimension {
                value: number(f32::NAN),
                unit: LengthUnit::Px,
            })),
        ] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(StyleProperty::BackdropFilter, value),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::BackdropFilter
                ))
            );
        }
    }

    #[test]
    fn transform_retains_box_percentages_and_three_dimensional_functions() {
        let transform = StyleValue::Transform(TransformValue(vec![
            TransformFunctionValue::Translate(
                LengthPercentageValue::Percentage(number(50.0)),
                px_length(4.0),
            ),
            TransformFunctionValue::Scale(number(2.0), number(3.0)),
        ]));
        let origin = StyleValue::TransformOrigin(TransformOriginValue {
            horizontal: LengthPercentageValue::Percentage(number(25.0)),
            vertical: LengthPercentageValue::Percentage(number(75.0)),
        });
        let resolved = crate::resolve_style(
            &SpecifiedStyle::new()
                .push(StyleProperty::Transform, transform)
                .push(StyleProperty::TransformOrigin, origin),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        let transform = &resolved.computed().paint().transform;
        assert_eq!(transform.origin_x, ComputedLengthPercentage::new(0.0, 0.25));
        assert_eq!(transform.origin_y, ComputedLengthPercentage::new(0.0, 0.75));
        assert_eq!(
            transform.functions,
            [
                ComputedTransformFunction::Translate {
                    x: ComputedLengthPercentage::new(0.0, 0.5),
                    y: ComputedLengthPercentage::new(4.0, 0.0),
                    z: number(0.0),
                },
                ComputedTransformFunction::Scale {
                    x: number(2.0),
                    y: number(3.0),
                    z: number(1.0),
                },
            ]
        );

        let rotated = crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::Transform,
                StyleValue::Transform(TransformValue(vec![
                    TransformFunctionValue::RotateX(number(30.0)),
                    TransformFunctionValue::TranslateZ(LengthValue::Dimension {
                        value: number(8.0),
                        unit: LengthUnit::Px,
                    }),
                ])),
            ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            rotated.computed().paint().transform.functions,
            [
                ComputedTransformFunction::RotateX(number(30.0)),
                ComputedTransformFunction::Translate {
                    x: ComputedLengthPercentage::ZERO,
                    y: ComputedLengthPercentage::ZERO,
                    z: number(8.0),
                },
            ]
        );
    }

    #[test]
    fn transform_resolves_every_function_and_rejects_invalid_inputs() {
        let length = |value| LengthValue::Dimension {
            value: number(value),
            unit: LengthUnit::Px,
        };
        let mut matrix_3d = [number(0.0); 16];
        for index in [0, 5, 10, 15] {
            matrix_3d[index] = number(1.0);
        }
        let functions = vec![
            TransformFunctionValue::TranslateX(percentage(10.0)),
            TransformFunctionValue::TranslateY(px_length(2.0)),
            TransformFunctionValue::TranslateZ(LengthValue::Zero),
            TransformFunctionValue::Translate3d(
                px_length(3.0),
                percentage(20.0),
                LengthValue::Zero,
            ),
            TransformFunctionValue::Rotate(number(10.0)),
            TransformFunctionValue::RotateX(number(0.0)),
            TransformFunctionValue::RotateY(number(0.0)),
            TransformFunctionValue::RotateZ(number(20.0)),
            TransformFunctionValue::ScaleX(number(2.0)),
            TransformFunctionValue::ScaleY(number(3.0)),
            TransformFunctionValue::Skew(number(4.0), number(5.0)),
            TransformFunctionValue::SkewX(number(6.0)),
            TransformFunctionValue::SkewY(number(7.0)),
            TransformFunctionValue::Matrix([
                number(1.0),
                number(2.0),
                number(3.0),
                number(4.0),
                number(5.0),
                number(6.0),
            ]),
            TransformFunctionValue::Matrix3d(matrix_3d),
        ];
        let resolved = crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::Transform,
                StyleValue::Transform(TransformValue(functions)),
            ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            resolved.computed().paint().transform.functions,
            [
                ComputedTransformFunction::Translate {
                    x: ComputedLengthPercentage::new(0.0, 0.1),
                    y: ComputedLengthPercentage::ZERO,
                    z: number(0.0),
                },
                ComputedTransformFunction::Translate {
                    x: ComputedLengthPercentage::ZERO,
                    y: ComputedLengthPercentage::new(2.0, 0.0),
                    z: number(0.0),
                },
                ComputedTransformFunction::Translate {
                    x: ComputedLengthPercentage::ZERO,
                    y: ComputedLengthPercentage::ZERO,
                    z: number(0.0),
                },
                ComputedTransformFunction::Translate {
                    x: ComputedLengthPercentage::new(3.0, 0.0),
                    y: ComputedLengthPercentage::new(0.0, 0.2),
                    z: number(0.0),
                },
                ComputedTransformFunction::RotateZ(number(10.0)),
                ComputedTransformFunction::RotateX(number(0.0)),
                ComputedTransformFunction::RotateY(number(0.0)),
                ComputedTransformFunction::RotateZ(number(20.0)),
                ComputedTransformFunction::Scale {
                    x: number(2.0),
                    y: number(1.0),
                    z: number(1.0),
                },
                ComputedTransformFunction::Scale {
                    x: number(1.0),
                    y: number(3.0),
                    z: number(1.0),
                },
                ComputedTransformFunction::Skew {
                    x_degrees: number(4.0),
                    y_degrees: number(5.0),
                },
                ComputedTransformFunction::Skew {
                    x_degrees: number(6.0),
                    y_degrees: number(0.0),
                },
                ComputedTransformFunction::Skew {
                    x_degrees: number(0.0),
                    y_degrees: number(7.0),
                },
                ComputedTransformFunction::Matrix([
                    number(1.0),
                    number(2.0),
                    number(0.0),
                    number(0.0),
                    number(3.0),
                    number(4.0),
                    number(0.0),
                    number(0.0),
                    number(0.0),
                    number(0.0),
                    number(1.0),
                    number(0.0),
                    number(5.0),
                    number(6.0),
                    number(0.0),
                    number(1.0),
                ]),
                ComputedTransformFunction::Matrix(matrix_3d),
            ]
        );

        let invalid_transform = |function| {
            crate::resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::Transform,
                    StyleValue::Transform(TransformValue(vec![function])),
                ),
                None,
                StyleEnvironment::default(),
            )
        };
        let mut non_finite_matrix = [number(0.0); 6];
        non_finite_matrix[0] = number(f32::NAN);
        let mut non_finite_matrix_3d = matrix_3d;
        non_finite_matrix_3d[0] = number(f32::INFINITY);
        let mut spatial_matrix_3d = matrix_3d;
        spatial_matrix_3d[14] = number(1.0);
        for function in [
            TransformFunctionValue::Translate(
                LengthPercentageValue::Length(length(f32::NAN)),
                px_length(0.0),
            ),
            TransformFunctionValue::Translate(
                px_length(0.0),
                LengthPercentageValue::Length(length(f32::NAN)),
            ),
            TransformFunctionValue::TranslateX(LengthPercentageValue::Length(length(f32::NAN))),
            TransformFunctionValue::TranslateY(LengthPercentageValue::Length(length(f32::NAN))),
            TransformFunctionValue::TranslateZ(length(f32::NAN)),
            TransformFunctionValue::Translate3d(px_length(0.0), px_length(0.0), length(f32::NAN)),
            TransformFunctionValue::Translate3d(
                LengthPercentageValue::Length(length(f32::NAN)),
                px_length(0.0),
                LengthValue::Zero,
            ),
            TransformFunctionValue::Translate3d(
                px_length(0.0),
                LengthPercentageValue::Length(length(f32::NAN)),
                LengthValue::Zero,
            ),
            TransformFunctionValue::Rotate(number(f32::NAN)),
            TransformFunctionValue::RotateX(number(f32::NAN)),
            TransformFunctionValue::RotateY(number(f32::NAN)),
            TransformFunctionValue::Scale(number(f32::NAN), number(1.0)),
            TransformFunctionValue::Scale(number(1.0), number(f32::NAN)),
            TransformFunctionValue::ScaleX(number(f32::INFINITY)),
            TransformFunctionValue::ScaleY(number(f32::INFINITY)),
            TransformFunctionValue::Skew(number(f32::NAN), number(0.0)),
            TransformFunctionValue::Skew(number(0.0), number(f32::NAN)),
            TransformFunctionValue::SkewX(number(f32::NAN)),
            TransformFunctionValue::SkewY(number(f32::NAN)),
            TransformFunctionValue::Matrix(non_finite_matrix),
            TransformFunctionValue::Matrix3d(non_finite_matrix_3d),
        ] {
            assert_eq!(
                invalid_transform(function),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::Transform
                ))
            );
        }
        for function in [
            TransformFunctionValue::TranslateZ(length(1.0)),
            TransformFunctionValue::Translate3d(px_length(0.0), px_length(0.0), length(1.0)),
            TransformFunctionValue::RotateX(number(1.0)),
            TransformFunctionValue::RotateY(number(1.0)),
            TransformFunctionValue::Matrix3d(spatial_matrix_3d),
        ] {
            assert!(invalid_transform(function).is_ok());
        }

        for (property, value) in [
            (StyleProperty::Transform, StyleValue::Number(number(1.0))),
            (
                StyleProperty::TransformOrigin,
                StyleValue::Number(number(1.0)),
            ),
            (
                StyleProperty::TransformOrigin,
                StyleValue::TransformOrigin(TransformOriginValue {
                    horizontal: LengthPercentageValue::Length(length(f32::NAN)),
                    vertical: px_length(0.0),
                }),
            ),
            (
                StyleProperty::TransformOrigin,
                StyleValue::TransformOrigin(TransformOriginValue {
                    horizontal: px_length(0.0),
                    vertical: LengthPercentageValue::Length(length(f32::NAN)),
                }),
            ),
        ] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(property, value),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(property))
            );
        }
    }

    #[test]
    fn perspective_resolves_absolute_length_and_rejects_negative_distance() {
        let perspective = |value| {
            StyleValue::Length(LengthValue::Dimension {
                value: number(value),
                unit: LengthUnit::Rem,
            })
        };
        let resolved = crate::resolve_style(
            &SpecifiedStyle::new().push(StyleProperty::Perspective, perspective(2.0)),
            None,
            StyleEnvironment::new(320.0, 480.0, 2.0, 16.0),
        )
        .unwrap();
        assert_eq!(
            resolved.computed().paint().transform.perspective,
            Some(number(32.0))
        );
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(StyleProperty::Perspective, perspective(-1.0)),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::Perspective
            ))
        );
        for value in [StyleValue::Number(number(1.0)), perspective(f32::NAN)] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(StyleProperty::Perspective, value),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::Perspective
                ))
            );
        }
    }

    #[test]
    fn motion_path_resolves_progress_and_rotation_and_rejects_invalid_values() {
        let point = |x, y| crate::MotionPathPointValue {
            x: number(x),
            y: number(y),
        };
        let commands = vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::LineTo(point(40.0, 0.0)),
            MotionPathCommandValue::QuadraticTo {
                control: point(50.0, 10.0),
                to: point(60.0, 0.0),
            },
            MotionPathCommandValue::CubicTo {
                control1: point(70.0, -10.0),
                control2: point(80.0, 10.0),
                to: point(90.0, 0.0),
            },
            MotionPathCommandValue::ArcTo {
                radius_x: number(25.0),
                radius_y: number(10.0),
                x_axis_rotation: number(30.0),
                large_arc: true,
                sweep: false,
                to: point(100.0, 20.0),
            },
        ];
        let path = OffsetPathValue::Path(commands.clone());
        let resolved = crate::resolve_style(
            &SpecifiedStyle::new()
                .push(
                    StyleProperty::OffsetPath,
                    StyleValue::OffsetPath(path.clone()),
                )
                .push(
                    StyleProperty::OffsetDistance,
                    StyleValue::LengthPercentage(percentage(75.0)),
                )
                .push(
                    StyleProperty::OffsetRotate,
                    StyleValue::OffsetRotate(OffsetRotateValue::Auto),
                ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        let transform = &resolved.computed().paint().transform;
        assert_eq!(
            transform.offset_path,
            ComputedOffsetPathValue::Path(commands)
        );
        assert_eq!(transform.offset_distance, number(0.75));
        assert_eq!(transform.offset_rotate, OffsetRotateValue::Auto);

        let fixed = crate::resolve_style(
            &SpecifiedStyle::new()
                .push(
                    StyleProperty::OffsetPath,
                    StyleValue::OffsetPath(OffsetPathValue::None),
                )
                .push(
                    StyleProperty::OffsetDistance,
                    StyleValue::Number(number(0.5)),
                )
                .push(
                    StyleProperty::OffsetRotate,
                    StyleValue::OffsetRotate(OffsetRotateValue::Angle(number(45.0))),
                ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            fixed.computed().paint().transform.offset_path,
            ComputedOffsetPathValue::None
        );
        assert_eq!(
            fixed.computed().paint().transform.offset_distance,
            number(0.5)
        );
        assert_eq!(
            fixed.computed().paint().transform.offset_rotate,
            OffsetRotateValue::Angle(number(45.0))
        );

        let circle = crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::OffsetPath,
                StyleValue::OffsetPath(OffsetPathValue::Circle {
                    radius: percentage(25.0),
                    center_x: px_length(10.0),
                    center_y: percentage(75.0),
                }),
            ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            circle.computed().paint().transform.offset_path,
            ComputedOffsetPathValue::Circle {
                radius: ComputedLengthPercentage::new(0.0, 0.25),
                center_x: ComputedLengthPercentage::new(10.0, 0.0),
                center_y: ComputedLengthPercentage::new(0.0, 0.75),
            }
        );

        let ellipse = crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::OffsetPath,
                StyleValue::OffsetPath(OffsetPathValue::Ellipse {
                    radius_x: px_length(10.0),
                    radius_y: percentage(25.0),
                    center_x: percentage(50.0),
                    center_y: percentage(50.0),
                }),
            ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            ellipse.computed().paint().transform.offset_path,
            ComputedOffsetPathValue::Ellipse {
                radius_x: ComputedLengthPercentage::new(10.0, 0.0),
                radius_y: ComputedLengthPercentage::new(0.0, 0.25),
                center_x: ComputedLengthPercentage::new(0.0, 0.5),
                center_y: ComputedLengthPercentage::new(0.0, 0.5),
            }
        );

        let inset_radius = |horizontal, vertical| BorderRadiusValue {
            horizontal,
            vertical,
        };
        let inset = crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::OffsetPath,
                StyleValue::OffsetPath(OffsetPathValue::Inset(Box::new(crate::InsetPathValue {
                    offsets: [
                        percentage(10.0),
                        px_length(20.0),
                        percentage(25.0),
                        px_length(5.0),
                    ],
                    radii: Some([
                        inset_radius(px_length(2.0), percentage(5.0)),
                        inset_radius(px_length(3.0), percentage(6.0)),
                        inset_radius(px_length(4.0), percentage(7.0)),
                        inset_radius(px_length(5.0), percentage(8.0)),
                    ]),
                }))),
            ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            inset.computed().paint().transform.offset_path,
            ComputedOffsetPathValue::Inset(Box::new(ComputedInsetPathValue {
                offsets: Edges {
                    top: ComputedLengthPercentage::new(0.0, 0.1),
                    right: ComputedLengthPercentage::new(20.0, 0.0),
                    bottom: ComputedLengthPercentage::new(0.0, 0.25),
                    left: ComputedLengthPercentage::new(5.0, 0.0),
                },
                radii: Some(Corners {
                    top_left: ComputedCornerRadius {
                        horizontal: ComputedLengthPercentage::new(2.0, 0.0),
                        vertical: ComputedLengthPercentage::new(0.0, 0.05),
                    },
                    top_right: ComputedCornerRadius {
                        horizontal: ComputedLengthPercentage::new(3.0, 0.0),
                        vertical: ComputedLengthPercentage::new(0.0, 0.06),
                    },
                    bottom_right: ComputedCornerRadius {
                        horizontal: ComputedLengthPercentage::new(4.0, 0.0),
                        vertical: ComputedLengthPercentage::new(0.0, 0.07),
                    },
                    bottom_left: ComputedCornerRadius {
                        horizontal: ComputedLengthPercentage::new(5.0, 0.0),
                        vertical: ComputedLengthPercentage::new(0.0, 0.08),
                    },
                }),
            }))
        );

        let invalid_path = |path| {
            crate::resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::OffsetPath,
                    StyleValue::OffsetPath(OffsetPathValue::Path(path)),
                ),
                None,
                StyleEnvironment::default(),
            )
        };
        for commands in [
            Vec::new(),
            vec![MotionPathCommandValue::LineTo(point(1.0, 1.0))],
            vec![MotionPathCommandValue::QuadraticTo {
                control: point(1.0, 1.0),
                to: point(2.0, 2.0),
            }],
            vec![MotionPathCommandValue::CubicTo {
                control1: point(1.0, 1.0),
                control2: point(2.0, 2.0),
                to: point(3.0, 3.0),
            }],
            vec![MotionPathCommandValue::ArcTo {
                radius_x: number(1.0),
                radius_y: number(1.0),
                x_axis_rotation: number(0.0),
                large_arc: false,
                sweep: true,
                to: point(3.0, 3.0),
            }],
            vec![MotionPathCommandValue::Close],
            vec![MotionPathCommandValue::MoveTo(point(f32::NAN, 0.0))],
            vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::LineTo(point(f32::NAN, 0.0)),
            ],
            vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::QuadraticTo {
                    control: point(f32::NAN, 0.0),
                    to: point(1.0, 1.0),
                },
            ],
            vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::CubicTo {
                    control1: point(f32::NAN, 0.0),
                    control2: point(1.0, 1.0),
                    to: point(2.0, 2.0),
                },
            ],
            vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::ArcTo {
                    radius_x: number(f32::NAN),
                    radius_y: number(1.0),
                    x_axis_rotation: number(0.0),
                    large_arc: false,
                    sweep: true,
                    to: point(2.0, 2.0),
                },
            ],
            vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::ArcTo {
                    radius_x: number(1.0),
                    radius_y: number(f32::NAN),
                    x_axis_rotation: number(0.0),
                    large_arc: false,
                    sweep: true,
                    to: point(2.0, 2.0),
                },
            ],
            vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::ArcTo {
                    radius_x: number(1.0),
                    radius_y: number(1.0),
                    x_axis_rotation: number(f32::NAN),
                    large_arc: false,
                    sweep: true,
                    to: point(2.0, 2.0),
                },
            ],
            vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::ArcTo {
                    radius_x: number(1.0),
                    radius_y: number(1.0),
                    x_axis_rotation: number(0.0),
                    large_arc: false,
                    sweep: true,
                    to: point(f32::NAN, 2.0),
                },
            ],
            vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::ArcTo {
                    radius_x: number(1.0),
                    radius_y: number(1.0),
                    x_axis_rotation: number(0.0),
                    large_arc: false,
                    sweep: true,
                    to: point(2.0, f32::NAN),
                },
            ],
            vec![
                MotionPathCommandValue::MoveTo(point(-f32::MAX, 0.0)),
                MotionPathCommandValue::LineTo(point(f32::MAX, 0.0)),
            ],
            vec![
                MotionPathCommandValue::MoveTo(point(1.0, 1.0)),
                MotionPathCommandValue::LineTo(point(1.0, 1.0)),
            ],
        ] {
            assert_eq!(
                invalid_path(commands),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::OffsetPath
                ))
            );
        }

        let invalid_declaration = |property, value| {
            crate::resolve_style(
                &SpecifiedStyle::new().push(property, value),
                None,
                StyleEnvironment::default(),
            )
        };
        let invalid_length = px_length(f32::NAN);
        let valid_length = percentage(50.0);
        for path in [
            OffsetPathValue::Circle {
                radius: invalid_length.clone(),
                center_x: valid_length.clone(),
                center_y: valid_length.clone(),
            },
            OffsetPathValue::Circle {
                radius: valid_length.clone(),
                center_x: invalid_length.clone(),
                center_y: valid_length.clone(),
            },
            OffsetPathValue::Circle {
                radius: valid_length.clone(),
                center_x: valid_length.clone(),
                center_y: invalid_length.clone(),
            },
            OffsetPathValue::Ellipse {
                radius_x: invalid_length.clone(),
                radius_y: valid_length.clone(),
                center_x: valid_length.clone(),
                center_y: valid_length.clone(),
            },
            OffsetPathValue::Ellipse {
                radius_x: valid_length.clone(),
                radius_y: invalid_length.clone(),
                center_x: valid_length.clone(),
                center_y: valid_length.clone(),
            },
            OffsetPathValue::Ellipse {
                radius_x: valid_length.clone(),
                radius_y: valid_length.clone(),
                center_x: invalid_length.clone(),
                center_y: valid_length.clone(),
            },
            OffsetPathValue::Ellipse {
                radius_x: valid_length.clone(),
                radius_y: valid_length.clone(),
                center_x: valid_length.clone(),
                center_y: invalid_length.clone(),
            },
        ] {
            assert_eq!(
                invalid_declaration(StyleProperty::OffsetPath, StyleValue::OffsetPath(path),),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::OffsetPath
                ))
            );
        }
        for index in 0..4 {
            let mut offsets = std::array::from_fn(|_| valid_length.clone());
            offsets[index] = invalid_length.clone();
            assert_eq!(
                invalid_declaration(
                    StyleProperty::OffsetPath,
                    StyleValue::OffsetPath(OffsetPathValue::Inset(Box::new(
                        crate::InsetPathValue {
                            offsets,
                            radii: None,
                        },
                    ))),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::OffsetPath
                ))
            );
        }
        for index in 0..4 {
            let mut radii = std::array::from_fn(|_| BorderRadiusValue {
                horizontal: valid_length.clone(),
                vertical: valid_length.clone(),
            });
            radii[index].horizontal = invalid_length.clone();
            assert_eq!(
                invalid_declaration(
                    StyleProperty::OffsetPath,
                    StyleValue::OffsetPath(OffsetPathValue::Inset(Box::new(
                        crate::InsetPathValue {
                            offsets: std::array::from_fn(|_| valid_length.clone()),
                            radii: Some(radii),
                        },
                    ))),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::OffsetPath
                ))
            );
        }
        for value in [
            StyleValue::Text("invalid".into()),
            StyleValue::Number(number(f32::NAN)),
            StyleValue::Number(number(-0.1)),
            StyleValue::Number(number(1.1)),
            StyleValue::LengthPercentage(percentage(-1.0)),
            StyleValue::LengthPercentage(percentage(101.0)),
        ] {
            assert_eq!(
                invalid_declaration(StyleProperty::OffsetDistance, value),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::OffsetDistance
                ))
            );
        }
        for value in [
            StyleValue::Number(number(0.0)),
            StyleValue::OffsetRotate(OffsetRotateValue::Angle(number(f32::NAN))),
        ] {
            assert_eq!(
                invalid_declaration(StyleProperty::OffsetRotate, value),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::OffsetRotate
                ))
            );
        }
        assert_eq!(
            invalid_declaration(StyleProperty::OffsetPath, StyleValue::Number(number(0.0))),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::OffsetPath
            ))
        );

        assert!(
            invalid_path(vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::LineTo(point(10.0, 0.0)),
                MotionPathCommandValue::Close,
            ])
            .is_ok()
        );
    }

    #[test]
    fn logical_borders_resolve_to_physical_edges_and_corners() {
        let specified = |direction| {
            SpecifiedStyle::new()
                .push(StyleProperty::Direction, StyleValue::Direction(direction))
                .push(StyleProperty::BorderInlineStartWidth, px(2.0))
                .push(StyleProperty::BorderInlineEndWidth, px(3.0))
                .push(
                    StyleProperty::BorderInlineStartColor,
                    StyleValue::Color(ColorValue::Named("start".into())),
                )
                .push(
                    StyleProperty::BorderInlineEndColor,
                    StyleValue::Color(ColorValue::Named("end".into())),
                )
                .push(
                    StyleProperty::BorderInlineStartStyle,
                    StyleValue::BorderStyle(BorderStyleValue::Dotted),
                )
                .push(
                    StyleProperty::BorderInlineEndStyle,
                    StyleValue::BorderStyle(BorderStyleValue::Double),
                )
                .push(StyleProperty::BorderStartStartRadius, px(11.0))
                .push(StyleProperty::BorderStartEndRadius, px(12.0))
                .push(StyleProperty::BorderEndStartRadius, px(13.0))
                .push(StyleProperty::BorderEndEndRadius, px(14.0))
        };

        for (direction, start_is_left) in [
            (crate::DirectionValue::Ltr, true),
            (crate::DirectionValue::Rtl, false),
        ] {
            let resolved =
                crate::resolve_style(&specified(direction), None, StyleEnvironment::default())
                    .unwrap();
            let layout = resolved.computed().layout();
            let paint = resolved.computed().paint();
            let (start_width, end_width) = if start_is_left {
                (layout.border.left.length(), layout.border.right.length())
            } else {
                (layout.border.right.length(), layout.border.left.length())
            };
            assert_eq!((start_width, end_width), (2.0, 3.0));

            let (start_color, end_color, start_style, end_style) = if start_is_left {
                (
                    &paint.border_colors.left,
                    &paint.border_colors.right,
                    paint.border_styles.left,
                    paint.border_styles.right,
                )
            } else {
                (
                    &paint.border_colors.right,
                    &paint.border_colors.left,
                    paint.border_styles.right,
                    paint.border_styles.left,
                )
            };
            assert_eq!(start_color, &ColorValue::Named("start".into()));
            assert_eq!(end_color, &ColorValue::Named("end".into()));
            assert_eq!(start_style, BorderStyleValue::Dotted);
            assert_eq!(end_style, BorderStyleValue::Double);

            let corners = &paint.border_radii;
            let logical = if start_is_left {
                [
                    corners.top_left,
                    corners.top_right,
                    corners.bottom_left,
                    corners.bottom_right,
                ]
            } else {
                [
                    corners.top_right,
                    corners.top_left,
                    corners.bottom_right,
                    corners.bottom_left,
                ]
            };
            assert_eq!(
                logical.map(|corner| corner.horizontal.length()),
                [11.0, 12.0, 13.0, 14.0]
            );
        }
    }

    #[test]
    fn logical_and_physical_border_declarations_share_final_write_order() {
        let resolved = crate::resolve_style(
            &SpecifiedStyle::new()
                .push(StyleProperty::BorderInlineStartWidth, px(2.0))
                .push(StyleProperty::BorderLeftWidth, px(4.0))
                .push(
                    StyleProperty::BorderInlineStartColor,
                    StyleValue::Color(ColorValue::Named("logical".into())),
                )
                .push(
                    StyleProperty::BorderLeftColor,
                    StyleValue::Color(ColorValue::Named("physical".into())),
                ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(resolved.computed().layout().border.left.length(), 4.0);
        assert_eq!(
            resolved.computed().paint().border_colors.left,
            ColorValue::Named("physical".into())
        );

        let resolved = crate::resolve_style(
            &SpecifiedStyle::new()
                .push(StyleProperty::BorderLeftWidth, px(4.0))
                .push(StyleProperty::BorderInlineStartWidth, px(6.0))
                .push(
                    StyleProperty::BorderLeftColor,
                    StyleValue::Color(ColorValue::Named("physical".into())),
                )
                .push(
                    StyleProperty::BorderInlineStartColor,
                    StyleValue::Color(ColorValue::Named("logical".into())),
                ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(resolved.computed().layout().border.left.length(), 6.0);
        assert_eq!(
            resolved.computed().paint().border_colors.left,
            ColorValue::Named("logical".into())
        );
    }

    #[test]
    fn background_layer_initial_values_match_css() {
        let resolved =
            crate::resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default())
                .unwrap();
        assert_eq!(
            resolved.computed().paint().background_layers[0],
            ComputedBackgroundLayerStyle::default()
        );
    }

    #[test]
    fn background_shorthand_resolves_layer_lists_and_empty_initials() {
        let layer = |url: &str, x: f32, size, repeat_x, origin, clip| BackgroundLayerValue {
            image: BackgroundImageValue::Url(url.into()),
            position: BackgroundPositionValue {
                horizontal: percentage(x),
                vertical: px_length(4.0),
            },
            size,
            repeat: BackgroundRepeatValue {
                horizontal: repeat_x,
                vertical: BackgroundRepeatModeValue::NoRepeat,
            },
            origin,
            clip,
            attachment: BackgroundAttachmentValue::Scroll,
        };
        let color = ColorValue::Named("background".into());
        let specified = SpecifiedStyle::new()
            .push(
                StyleProperty::Background,
                StyleValue::Background(BackgroundValue {
                    layers: vec![
                        layer(
                            "front",
                            25.0,
                            BackgroundSizeValue::Cover,
                            BackgroundRepeatModeValue::Space,
                            BackgroundBoxValue::Content,
                            BackgroundBoxValue::Padding,
                        ),
                        layer(
                            "back",
                            75.0,
                            BackgroundSizeValue::Contain,
                            BackgroundRepeatModeValue::Round,
                            BackgroundBoxValue::Border,
                            BackgroundBoxValue::Content,
                        ),
                    ],
                    color: color.clone(),
                }),
            )
            .push(StyleProperty::BackgroundPositionY, px(9.0));
        let resolved = crate::resolve_style(&specified, None, StyleEnvironment::default()).unwrap();
        let paint = resolved.computed().paint();
        assert_eq!(paint.background_color, color);
        assert_eq!(
            paint.background_images,
            vec![
                ComputedBackgroundImage::Url("front".into()),
                ComputedBackgroundImage::Url("back".into()),
            ]
        );
        assert_eq!(paint.background_layers.len(), 2);
        assert_eq!(
            paint.background_layers[0].position.horizontal.fraction(),
            0.25
        );
        assert_eq!(
            paint.background_layers[1].position.horizontal.fraction(),
            0.75
        );
        assert!(
            paint
                .background_layers
                .iter()
                .all(|layer| layer.position.vertical.length() == 9.0)
        );
        assert_eq!(
            paint.background_layers[0].size,
            ComputedBackgroundSize::Cover
        );
        assert_eq!(
            paint.background_layers[1].size,
            ComputedBackgroundSize::Contain
        );

        let empty = crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::Background,
                StyleValue::Background(BackgroundValue {
                    layers: Vec::new(),
                    color: ColorValue::Named("empty".into()),
                }),
            ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert!(empty.computed().paint().background_images.is_empty());
        assert_eq!(empty.computed().paint().background_layers.len(), 1);

        let mut invalid_position = layer(
            "invalid-position",
            0.0,
            BackgroundSizeValue::Auto,
            BackgroundRepeatModeValue::Repeat,
            BackgroundBoxValue::Padding,
            BackgroundBoxValue::Border,
        );
        invalid_position.position.horizontal = px_length(f32::NAN);
        let mut invalid_size = layer(
            "invalid-size",
            0.0,
            BackgroundSizeValue::Auto,
            BackgroundRepeatModeValue::Repeat,
            BackgroundBoxValue::Padding,
            BackgroundBoxValue::Border,
        );
        invalid_size.size = BackgroundSizeValue::Explicit {
            width: Some(px_length(-1.0)),
            height: None,
        };
        let invalid_image = layer(
            "  ",
            0.0,
            BackgroundSizeValue::Auto,
            BackgroundRepeatModeValue::Repeat,
            BackgroundBoxValue::Padding,
            BackgroundBoxValue::Border,
        );
        for layer in [invalid_position, invalid_size, invalid_image] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(
                        StyleProperty::Background,
                        StyleValue::Background(BackgroundValue {
                            layers: vec![layer],
                            color: ColorValue::Named("invalid".into()),
                        }),
                    ),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::Background
                ))
            );
        }
    }

    #[test]
    fn background_gradients_resolve_all_shapes_and_reject_invalid_values() {
        let stop = |name: &str, position| GradientStopValue {
            color: ColorValue::Named(name.into()),
            position,
        };
        let stops = || {
            vec![
                stop("red", None),
                stop("gold", None),
                stop("blue", Some(percentage(100.0))),
            ]
        };
        let resolve = |image| {
            crate::resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::BackgroundImage,
                    StyleValue::BackgroundImages(vec![image]),
                ),
                None,
                StyleEnvironment::default(),
            )
        };

        assert_eq!(
            resolve(BackgroundImageValue::None)
                .unwrap()
                .computed()
                .paint()
                .background_images,
            vec![ComputedBackgroundImage::None]
        );
        let linear = resolve(BackgroundImageValue::Gradient(GradientValue::Linear {
            angle_degrees: number(90.0),
            stops: stops(),
        }))
        .unwrap();
        assert!(matches!(
            &linear.computed().paint().background_images[0],
            ComputedBackgroundImage::Gradient(ComputedGradient::Linear {
                angle_degrees,
                stops,
            }) if angle_degrees.get() == 90.0
                && stops[0].position.unwrap().fraction() == 0.0
                && stops[1].position.unwrap().fraction() == 0.5
                && stops[2].position.unwrap().fraction() == 1.0
        ));
        let implicit_endpoints = resolve(BackgroundImageValue::Gradient(GradientValue::Linear {
            angle_degrees: number(0.0),
            stops: vec![stop("start", None), stop("end", None)],
        }))
        .unwrap();
        assert!(matches!(
            &implicit_endpoints.computed().paint().background_images[0],
            ComputedBackgroundImage::Gradient(ComputedGradient::Linear { stops, .. })
                if stops[0].position.unwrap().fraction() == 0.0
                    && stops[1].position.unwrap().fraction() == 1.0
        ));

        for (shape, circle, explicit) in [
            (RadialGradientValue::Circle, true, false),
            (RadialGradientValue::Ellipse, false, false),
            (
                RadialGradientValue::CircleSized(px_length(20.0)),
                true,
                true,
            ),
            (
                RadialGradientValue::EllipseSized(px_length(30.0), percentage(40.0)),
                false,
                true,
            ),
        ] {
            let resolved = resolve(BackgroundImageValue::Gradient(GradientValue::Radial {
                shape,
                stops: stops(),
            }))
            .unwrap();
            assert!(matches!(
                &resolved.computed().paint().background_images[0],
                ComputedBackgroundImage::Gradient(ComputedGradient::Radial {
                    circle: actual_circle,
                    radii,
                    ..
                }) if *actual_circle == circle && radii.is_some() == explicit
            ));
        }

        let conic = resolve(BackgroundImageValue::Gradient(GradientValue::Conic {
            from_degrees: number(45.0),
            center: BackgroundPositionValue {
                horizontal: percentage(25.0),
                vertical: percentage(75.0),
            },
            stops: stops(),
        }))
        .unwrap();
        assert!(matches!(
            &conic.computed().paint().background_images[0],
            ComputedBackgroundImage::Gradient(ComputedGradient::Conic {
                from_degrees,
                center,
                ..
            }) if from_degrees.get() == 45.0
                && center.horizontal.fraction() == 0.25
                && center.vertical.fraction() == 0.75
        ));

        let invalid_color = GradientStopValue {
            color: ColorValue::Rgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: number(f32::NAN),
            },
            position: None,
        };
        let invalid = [
            BackgroundImageValue::Url("  ".into()),
            BackgroundImageValue::Gradient(GradientValue::Linear {
                angle_degrees: number(f32::NAN),
                stops: stops(),
            }),
            BackgroundImageValue::Gradient(GradientValue::Linear {
                angle_degrees: number(0.0),
                stops: vec![stop("only", None)],
            }),
            BackgroundImageValue::Gradient(GradientValue::Linear {
                angle_degrees: number(0.0),
                stops: vec![
                    stop("bad-position", Some(px_length(f32::NAN))),
                    stop("end", None),
                ],
            }),
            BackgroundImageValue::Gradient(GradientValue::Linear {
                angle_degrees: number(0.0),
                stops: vec![invalid_color, stop("end", None)],
            }),
            BackgroundImageValue::Gradient(GradientValue::Radial {
                shape: RadialGradientValue::CircleSized(px_length(f32::NAN)),
                stops: stops(),
            }),
            BackgroundImageValue::Gradient(GradientValue::Radial {
                shape: RadialGradientValue::EllipseSized(px_length(1.0), px_length(f32::NAN)),
                stops: stops(),
            }),
            BackgroundImageValue::Gradient(GradientValue::Radial {
                shape: RadialGradientValue::EllipseSized(px_length(f32::NAN), px_length(1.0)),
                stops: stops(),
            }),
            BackgroundImageValue::Gradient(GradientValue::Radial {
                shape: RadialGradientValue::Circle,
                stops: vec![stop("only", None)],
            }),
            BackgroundImageValue::Gradient(GradientValue::Conic {
                from_degrees: number(f32::NAN),
                center: BackgroundPositionValue {
                    horizontal: percentage(50.0),
                    vertical: percentage(50.0),
                },
                stops: stops(),
            }),
            BackgroundImageValue::Gradient(GradientValue::Conic {
                from_degrees: number(0.0),
                center: BackgroundPositionValue {
                    horizontal: px_length(f32::NAN),
                    vertical: percentage(50.0),
                },
                stops: stops(),
            }),
            BackgroundImageValue::Gradient(GradientValue::Conic {
                from_degrees: number(0.0),
                center: BackgroundPositionValue {
                    horizontal: percentage(50.0),
                    vertical: percentage(50.0),
                },
                stops: vec![stop("only", None)],
            }),
        ];
        for image in invalid {
            assert_eq!(
                resolve(image),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::BackgroundImage
                ))
            );
        }
    }

    #[test]
    fn paint_values_resolve_without_host_types() {
        let specified = SpecifiedStyle::new()
            .push(
                StyleProperty::Color,
                StyleValue::Color(ColorValue::Named("current".into())),
            )
            .push(
                StyleProperty::BackgroundColor,
                StyleValue::Color(ColorValue::Rgba {
                    red: 1,
                    green: 2,
                    blue: 3,
                    alpha: number(0.5),
                }),
            )
            .push(
                StyleProperty::BackgroundImage,
                StyleValue::BackgroundImages(vec![BackgroundImageValue::Url(
                    "https://example.com/image.png".into(),
                )]),
            )
            .push(
                StyleProperty::BackgroundPosition,
                StyleValue::BackgroundPosition(BackgroundPositionValue {
                    horizontal: percentage(25.0),
                    vertical: px_length(10.0),
                }),
            )
            .push(StyleProperty::BackgroundPositionX, px(5.0))
            .push(
                StyleProperty::BackgroundSize,
                StyleValue::BackgroundSize(BackgroundSizeValue::Explicit {
                    width: Some(percentage(50.0)),
                    height: Some(px_length(20.0)),
                }),
            )
            .push(
                StyleProperty::BackgroundRepeat,
                StyleValue::BackgroundRepeat(BackgroundRepeatValue {
                    horizontal: BackgroundRepeatModeValue::Space,
                    vertical: BackgroundRepeatModeValue::Round,
                }),
            )
            .push(
                StyleProperty::BackgroundOrigin,
                StyleValue::BackgroundBox(BackgroundBoxValue::Content),
            )
            .push(
                StyleProperty::BackgroundClip,
                StyleValue::BackgroundBox(BackgroundBoxValue::Padding),
            )
            .push(
                StyleProperty::BackgroundAttachment,
                StyleValue::BackgroundAttachment(BackgroundAttachmentValue::Scroll),
            )
            .push(
                StyleProperty::BorderTopStyle,
                StyleValue::BorderStyle(BorderStyleValue::Solid),
            )
            .push(
                StyleProperty::BorderTopColor,
                StyleValue::Color(ColorValue::Named("top".into())),
            )
            .push(
                StyleProperty::BorderRightColor,
                StyleValue::Color(ColorValue::Named("right".into())),
            )
            .push(
                StyleProperty::BorderBottomColor,
                StyleValue::Color(ColorValue::Named("bottom".into())),
            )
            .push(
                StyleProperty::BorderLeftColor,
                StyleValue::Color(ColorValue::Named("left".into())),
            )
            .push(
                StyleProperty::BorderRightStyle,
                StyleValue::BorderStyle(BorderStyleValue::Dashed),
            )
            .push(
                StyleProperty::BorderBottomStyle,
                StyleValue::BorderStyle(BorderStyleValue::Dotted),
            )
            .push(
                StyleProperty::BorderLeftStyle,
                StyleValue::BorderStyle(BorderStyleValue::Double),
            )
            .push(StyleProperty::BorderTopLeftRadius, px(8.0))
            .push(
                StyleProperty::BorderTopRightRadius,
                StyleValue::BorderRadius(BorderRadiusValue {
                    horizontal: px_length(9.0),
                    vertical: px_length(4.0),
                }),
            )
            .push(StyleProperty::BorderBottomRightRadius, px(10.0))
            .push(StyleProperty::BorderBottomLeftRadius, px(11.0))
            .push(StyleProperty::Opacity, StyleValue::Number(number(2.0)))
            .push(
                StyleProperty::OverflowX,
                StyleValue::Overflow(OverflowValue::Hidden),
            )
            .push(
                StyleProperty::OverflowY,
                StyleValue::Overflow(OverflowValue::Hidden),
            )
            .push(
                StyleProperty::Visibility,
                StyleValue::Visibility(VisibilityValue::Hidden),
            )
            .push(StyleProperty::ZIndex, StyleValue::Integer(-3));
        let resolved = crate::resolve_style(&specified, None, StyleEnvironment::default()).unwrap();
        let paint = resolved.computed().paint();
        assert_eq!(
            paint.background_color,
            ColorValue::Rgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: number(0.5),
            }
        );
        assert_eq!(
            paint.background_images,
            vec![ComputedBackgroundImage::Url(
                "https://example.com/image.png".into()
            )]
        );
        assert_eq!(paint.background_layers[0].position.horizontal.length(), 5.0);
        assert_eq!(
            paint.background_layers[0].position.horizontal.fraction(),
            0.0
        );
        assert_eq!(paint.background_layers[0].position.vertical.length(), 10.0);
        assert_eq!(
            paint.background_layers[0].size,
            ComputedBackgroundSize::Explicit {
                width: Some(ComputedLengthPercentage::new(0.0, 0.5)),
                height: Some(ComputedLengthPercentage::new(20.0, 0.0)),
            }
        );
        assert_eq!(
            paint.background_layers[0].repeat_x,
            BackgroundRepeatModeValue::Space
        );
        assert_eq!(
            paint.background_layers[0].repeat_y,
            BackgroundRepeatModeValue::Round
        );
        assert_eq!(
            paint.background_layers[0].origin,
            BackgroundBoxValue::Content
        );
        assert_eq!(paint.background_layers[0].clip, BackgroundBoxValue::Padding);
        assert_eq!(
            paint.background_layers[0].attachment,
            BackgroundAttachmentValue::Scroll
        );
        assert_eq!(paint.border_colors.top, ColorValue::Named("top".into()));
        assert_eq!(paint.border_colors.right, ColorValue::Named("right".into()));
        assert_eq!(
            paint.border_colors.bottom,
            ColorValue::Named("bottom".into())
        );
        assert_eq!(paint.border_colors.left, ColorValue::Named("left".into()));
        assert_eq!(paint.border_styles.top, BorderStyleValue::Solid);
        assert_eq!(paint.border_styles.right, BorderStyleValue::Dashed);
        assert_eq!(paint.border_styles.bottom, BorderStyleValue::Dotted);
        assert_eq!(paint.border_styles.left, BorderStyleValue::Double);
        assert_eq!(paint.border_radii.top_left.horizontal.length(), 8.0);
        assert_eq!(paint.border_radii.top_left.vertical.length(), 8.0);
        assert_eq!(paint.border_radii.top_right.horizontal.length(), 9.0);
        assert_eq!(paint.border_radii.top_right.vertical.length(), 4.0);
        assert_eq!(paint.border_radii.bottom_right.horizontal.length(), 10.0);
        assert_eq!(paint.border_radii.bottom_left.horizontal.length(), 11.0);
        assert_eq!(paint.opacity.get(), 1.0);
        assert_eq!(paint.overflow_x, OverflowValue::Hidden);
        assert_eq!(paint.overflow_y, OverflowValue::Hidden);
        assert_eq!(paint.visibility, VisibilityValue::Hidden);
        assert_eq!(paint.z_index, -3);

        assert!(paint.changes_from(paint).is_empty());
        let mut changed = paint.clone();
        changed.opacity = number(0.5);
        assert_eq!(changed.changes_from(paint), crate::PropertyImpactSet::PAINT);
    }

    #[test]
    fn background_geometry_resolves_keywords_auto_axes_and_axis_errors() {
        let resolve_size = |size| {
            crate::resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::BackgroundSize,
                    StyleValue::BackgroundSize(size),
                ),
                None,
                StyleEnvironment::default(),
            )
        };
        for (specified, computed) in [
            (BackgroundSizeValue::Auto, ComputedBackgroundSize::Auto),
            (BackgroundSizeValue::Cover, ComputedBackgroundSize::Cover),
            (
                BackgroundSizeValue::Contain,
                ComputedBackgroundSize::Contain,
            ),
            (
                BackgroundSizeValue::Explicit {
                    width: None,
                    height: None,
                },
                ComputedBackgroundSize::Auto,
            ),
            (
                BackgroundSizeValue::Explicit {
                    width: Some(px_length(12.0)),
                    height: None,
                },
                ComputedBackgroundSize::Explicit {
                    width: Some(ComputedLengthPercentage::new(12.0, 0.0)),
                    height: None,
                },
            ),
            (
                BackgroundSizeValue::Explicit {
                    width: None,
                    height: Some(percentage(25.0)),
                },
                ComputedBackgroundSize::Explicit {
                    width: None,
                    height: Some(ComputedLengthPercentage::new(0.0, 0.25)),
                },
            ),
        ] {
            assert_eq!(
                resolve_size(specified)
                    .unwrap()
                    .computed()
                    .paint()
                    .background_layers[0]
                    .size,
                computed
            );
        }

        for position in [
            BackgroundPositionValue {
                horizontal: px_length(f32::NAN),
                vertical: px_length(0.0),
            },
            BackgroundPositionValue {
                horizontal: px_length(0.0),
                vertical: px_length(f32::NAN),
            },
        ] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(
                        StyleProperty::BackgroundPosition,
                        StyleValue::BackgroundPosition(position),
                    ),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::BackgroundPosition
                ))
            );
        }

        for size in [
            BackgroundSizeValue::Explicit {
                width: Some(px_length(f32::NAN)),
                height: None,
            },
            BackgroundSizeValue::Explicit {
                width: None,
                height: Some(px_length(f32::NAN)),
            },
            BackgroundSizeValue::Explicit {
                width: None,
                height: Some(px_length(-1.0)),
            },
        ] {
            assert_eq!(
                resolve_size(size),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::BackgroundSize
                ))
            );
        }
    }

    #[test]
    fn invalid_paint_values_are_diagnostic() {
        for property in [
            StyleProperty::Background,
            StyleProperty::BackgroundColor,
            StyleProperty::BackgroundImage,
            StyleProperty::BackgroundRepeat,
            StyleProperty::BackgroundPosition,
            StyleProperty::BackgroundPositionX,
            StyleProperty::BackgroundPositionY,
            StyleProperty::BackgroundSize,
            StyleProperty::BackgroundOrigin,
            StyleProperty::BackgroundClip,
            StyleProperty::BackgroundAttachment,
            StyleProperty::BorderTopColor,
            StyleProperty::BorderRightColor,
            StyleProperty::BorderBottomColor,
            StyleProperty::BorderLeftColor,
            StyleProperty::BorderInlineStartColor,
            StyleProperty::BorderInlineEndColor,
            StyleProperty::BorderTopStyle,
            StyleProperty::BorderRightStyle,
            StyleProperty::BorderBottomStyle,
            StyleProperty::BorderLeftStyle,
            StyleProperty::BorderInlineStartStyle,
            StyleProperty::BorderInlineEndStyle,
            StyleProperty::BorderTopLeftRadius,
            StyleProperty::BorderTopRightRadius,
            StyleProperty::BorderBottomRightRadius,
            StyleProperty::BorderBottomLeftRadius,
            StyleProperty::BorderStartStartRadius,
            StyleProperty::BorderStartEndRadius,
            StyleProperty::BorderEndStartRadius,
            StyleProperty::BorderEndEndRadius,
            StyleProperty::Opacity,
            StyleProperty::Visibility,
            StyleProperty::ZIndex,
        ] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(property, StyleValue::Bool(true)),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(property))
            );
        }
        let inherited =
            crate::resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default())
                .unwrap()
                .computed()
                .inherited_text()
                .clone();
        for property in [StyleProperty::OverflowX, StyleProperty::OverflowY] {
            assert_eq!(
                resolve_paint_style(
                    &SpecifiedStyle::new().push(property, StyleValue::Bool(true)),
                    &inherited,
                    DirectionValue::Ltr,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(property))
            );
        }
        for property in [
            StyleProperty::BorderInlineStartColor,
            StyleProperty::BorderInlineEndColor,
            StyleProperty::BorderInlineStartStyle,
            StyleProperty::BorderInlineEndStyle,
            StyleProperty::BorderStartStartRadius,
            StyleProperty::BorderStartEndRadius,
            StyleProperty::BorderEndStartRadius,
            StyleProperty::BorderEndEndRadius,
        ] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new()
                        .push(
                            StyleProperty::Direction,
                            StyleValue::Direction(DirectionValue::Rtl),
                        )
                        .push(property, StyleValue::Bool(true)),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(property))
            );
        }
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(StyleProperty::BorderTopLeftRadius, px(-1.0)),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::BorderTopLeftRadius
            ))
        );
        for (property, value) in [
            (
                StyleProperty::BackgroundColor,
                StyleValue::Color(ColorValue::Named(String::new())),
            ),
            (
                StyleProperty::BackgroundSize,
                StyleValue::BackgroundSize(BackgroundSizeValue::Explicit {
                    width: Some(px_length(-1.0)),
                    height: Some(px_length(1.0)),
                }),
            ),
            (StyleProperty::Opacity, StyleValue::Number(number(f32::NAN))),
            (StyleProperty::ZIndex, StyleValue::Integer(i64::MAX)),
            (StyleProperty::BorderTopLeftRadius, px(f32::NAN)),
            (
                StyleProperty::BorderTopRightRadius,
                StyleValue::BorderRadius(BorderRadiusValue {
                    horizontal: px_length(1.0),
                    vertical: px_length(f32::NAN),
                }),
            ),
        ] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(property, value),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(property))
            );
        }
    }
}
