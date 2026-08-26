//! Resolved visual values shared by every Host renderer.

use crate::{
    BorderLineStyle, PaintColor, PaintCornerRadius, PaintCorners, PaintEdges,
    PaintLengthPercentage, ResourceId,
};

/// Lines enabled by `text-decoration-line` after style resolution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextDecorationLines {
    /// Underline glyphs.
    pub underline: bool,
    /// Draw a line above glyphs.
    pub overline: bool,
    /// Strike through glyphs.
    pub line_through: bool,
}

/// Stroke pattern for text decoration lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextDecorationStyle {
    /// Solid line.
    #[default]
    Solid,
    /// Double line.
    Double,
    /// Dotted line.
    Dotted,
    /// Dashed line.
    Dashed,
    /// Wavy line.
    Wavy,
}

/// Resolved text-decoration thickness.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TextDecorationThickness {
    /// Host chooses the normal thickness from the font.
    #[default]
    Auto,
    /// Use the font's preferred underline thickness.
    FromFont,
    /// Explicit non-negative logical-pixel thickness.
    Length(f32),
}

/// Complete resolved text line-decoration paint.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextDecoration {
    /// Enabled line types.
    pub lines: TextDecorationLines,
    /// Line color.
    pub color: PaintColor,
    /// Line stroke pattern.
    pub style: TextDecorationStyle,
    /// Line thickness.
    pub thickness: TextDecorationThickness,
}

impl TextDecoration {
    /// Validates its color and explicit thickness.
    pub fn validate(&self) -> bool {
        self.color.is_valid()
            && match self.thickness {
                TextDecorationThickness::Auto | TextDecorationThickness::FromFont => true,
                TextDecorationThickness::Length(value) => value.is_finite() && value >= 0.0,
            }
    }
}

/// One resolved text shadow, ordered front to back.
#[derive(Clone, Debug, PartialEq)]
pub struct TextShadow {
    /// Horizontal offset in logical pixels.
    pub offset_x: f32,
    /// Vertical offset in logical pixels.
    pub offset_y: f32,
    /// Non-negative blur radius in logical pixels.
    pub blur_radius: f32,
    /// Shadow color.
    pub color: PaintColor,
}

impl TextShadow {
    /// Validates finite geometry, blur range, and color.
    pub fn validate(&self) -> bool {
        self.offset_x.is_finite()
            && self.offset_y.is_finite()
            && self.blur_radius.is_finite()
            && self.blur_radius >= 0.0
            && self.color.is_valid()
    }
}

/// A signed affine coordinate resolved to logical pixels plus an axis fraction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintCoordinate {
    /// Absolute logical-pixel component.
    pub length: f32,
    /// Fraction of the relevant box axis, where `1.0` is 100 percent.
    pub fraction: f32,
}

impl PaintCoordinate {
    fn is_valid(self) -> bool {
        self.length.is_finite() && self.fraction.is_finite()
    }
}

/// A two-dimensional resolved position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintPosition {
    /// Horizontal position.
    pub x: PaintCoordinate,
    /// Vertical position.
    pub y: PaintCoordinate,
}

impl PaintPosition {
    fn is_valid(self) -> bool {
        self.x.is_valid() && self.y.is_valid()
    }
}

/// One color stop in a resolved gradient.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientStop {
    /// Stop color.
    pub color: PaintColor,
    /// Optional position along the gradient line.
    pub position: Option<PaintCoordinate>,
}

impl GradientStop {
    fn is_valid(&self) -> bool {
        self.color.is_valid() && self.position.is_none_or(PaintCoordinate::is_valid)
    }
}

/// Radius selection for a radial gradient.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RadialGradientExtent {
    /// The nearest side of the gradient box.
    ClosestSide,
    /// The farthest side of the gradient box.
    FarthestSide,
    /// The nearest corner of the gradient box.
    ClosestCorner,
    /// The farthest corner of the gradient box.
    FarthestCorner,
    /// Explicit horizontal and vertical radii are used.
    Explicit,
}

/// Shape of a radial gradient.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RadialGradientShape {
    /// Circular gradient.
    Circle,
    /// Elliptical gradient.
    Ellipse,
}

/// A renderer-independent image source.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintImage {
    /// No image for this layer; retained to preserve parallel layer lists.
    None,
    /// A resource whose lifecycle is negotiated separately from the frame.
    Resource(ResourceId),
    /// A linear gradient.
    LinearGradient {
        /// Direction in clockwise degrees from the positive vertical axis.
        angle_degrees: f32,
        /// Whether the gradient repeats beyond its final stop.
        repeating: bool,
        /// Ordered color stops.
        stops: Vec<GradientStop>,
    },
    /// A radial gradient.
    RadialGradient {
        /// Circle or ellipse.
        shape: RadialGradientShape,
        /// Keyword or explicit sizing rule.
        extent: RadialGradientExtent,
        /// Gradient center.
        center: PaintPosition,
        /// Explicit radii, used only with [`RadialGradientExtent::Explicit`].
        radii: Option<(PaintLengthPercentage, PaintLengthPercentage)>,
        /// Whether the gradient repeats beyond its final stop.
        repeating: bool,
        /// Ordered color stops.
        stops: Vec<GradientStop>,
    },
    /// A conic gradient.
    ConicGradient {
        /// Starting angle in clockwise degrees.
        from_degrees: f32,
        /// Gradient center.
        center: PaintPosition,
        /// Whether the gradient repeats beyond its final stop.
        repeating: bool,
        /// Ordered color stops.
        stops: Vec<GradientStop>,
    },
}

impl PaintImage {
    fn is_valid(&self) -> bool {
        let valid_stops =
            |stops: &[GradientStop]| stops.len() >= 2 && stops.iter().all(GradientStop::is_valid);
        match self {
            Self::None | Self::Resource(_) => true,
            Self::LinearGradient {
                angle_degrees,
                stops,
                ..
            } => angle_degrees.is_finite() && valid_stops(stops),
            Self::RadialGradient {
                extent,
                center,
                radii,
                stops,
                ..
            } => {
                center.is_valid()
                    && valid_stops(stops)
                    && match (extent, radii) {
                        (RadialGradientExtent::Explicit, Some((x, y))) => {
                            x.is_valid() && y.is_valid()
                        }
                        (RadialGradientExtent::Explicit, None) => false,
                        (_, None) => true,
                        (_, Some(_)) => false,
                    }
            }
            Self::ConicGradient {
                from_degrees,
                center,
                stops,
                ..
            } => from_degrees.is_finite() && center.is_valid() && valid_stops(stops),
        }
    }
}

/// Box used to position or clip a background or mask layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintBox {
    /// Margin box.
    Margin,
    /// Border box.
    Border,
    /// Padding box.
    Padding,
    /// Content box.
    Content,
    /// Object bounding box for SVG-compatible content.
    Fill,
    /// Stroke bounding box for SVG-compatible content.
    Stroke,
    /// Nearest SVG viewport box.
    View,
    /// Glyph shapes, for `background-clip: text`.
    Text,
    /// Border-painted area, for `background-clip: border-area`.
    BorderArea,
}

/// Repetition rule on one image axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageRepeat {
    /// Tile and crop the final tile when necessary.
    Repeat,
    /// Paint once.
    NoRepeat,
    /// Add spacing between whole tiles.
    Space,
    /// Scale tiles so a whole number fits.
    Round,
}

/// Resolved background-size behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackgroundSize {
    /// Preserve intrinsic sizing.
    Auto,
    /// Cover the positioning area while preserving aspect ratio.
    Cover,
    /// Fit inside the positioning area while preserving aspect ratio.
    Contain,
    /// Explicit width and height; `None` retains the intrinsic axis.
    Explicit {
        /// Width.
        width: Option<PaintLengthPercentage>,
        /// Height.
        height: Option<PaintLengthPercentage>,
    },
}

impl BackgroundSize {
    fn is_valid(self) -> bool {
        match self {
            Self::Auto | Self::Cover | Self::Contain => true,
            Self::Explicit { width, height } => {
                width.is_none_or(PaintLengthPercentage::is_valid)
                    && height.is_none_or(PaintLengthPercentage::is_valid)
            }
        }
    }
}

/// Scrolling behavior for a background layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundAttachment {
    /// Layer follows the element.
    Scroll,
    /// Layer remains fixed to the viewport.
    Fixed,
    /// Layer follows the element's local scrolling contents.
    Local,
}

/// Porter-Duff or blend operation shared by backgrounds and groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// Normal source-over blending.
    Normal,
    /// Multiply.
    Multiply,
    /// Screen.
    Screen,
    /// Overlay.
    Overlay,
    /// Darken.
    Darken,
    /// Lighten.
    Lighten,
    /// Color dodge.
    ColorDodge,
    /// Color burn.
    ColorBurn,
    /// Hard light.
    HardLight,
    /// Soft light.
    SoftLight,
    /// Difference.
    Difference,
    /// Exclusion.
    Exclusion,
    /// Hue.
    Hue,
    /// Saturation.
    Saturation,
    /// Color.
    Color,
    /// Luminosity.
    Luminosity,
}

/// One background image layer, ordered front to back.
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundLayer {
    /// Image or gradient.
    pub image: PaintImage,
    /// Position within the origin box.
    pub position: PaintPosition,
    /// Resolved sizing rule.
    pub size: BackgroundSize,
    /// Horizontal repetition.
    pub repeat_x: ImageRepeat,
    /// Vertical repetition.
    pub repeat_y: ImageRepeat,
    /// Positioning box.
    pub origin: PaintBox,
    /// Painting clip box.
    pub clip: PaintBox,
    /// Element/viewport attachment.
    pub attachment: BackgroundAttachment,
    /// Blend mode against layers behind this one.
    pub blend_mode: BlendMode,
}

impl BackgroundLayer {
    /// Validates image, position, and size components.
    pub fn validate(&self) -> bool {
        self.image.is_valid() && self.position.is_valid() && self.size.is_valid()
    }
}

/// Resolved outline paint.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlinePaint {
    /// Outline color.
    pub color: PaintColor,
    /// Outline style.
    pub style: OutlineLineStyle,
    /// Non-negative outline width.
    pub width: f32,
    /// Signed offset from the border edge.
    pub offset: f32,
}

/// Renderer-independent outline style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutlineLineStyle {
    /// Platform-defined focus outline.
    Auto,
    /// A concrete CSS border-line style.
    Line(BorderLineStyle),
}

impl OutlinePaint {
    fn is_valid(&self) -> bool {
        self.color.is_valid()
            && self.width.is_finite()
            && self.width >= 0.0
            && self.offset.is_finite()
    }
}

/// One resolved box shadow, ordered front to back.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxShadow {
    /// Horizontal offset.
    pub offset_x: f32,
    /// Vertical offset.
    pub offset_y: f32,
    /// Non-negative blur radius.
    pub blur_radius: f32,
    /// Signed spread radius.
    pub spread_radius: f32,
    /// Shadow color.
    pub color: PaintColor,
    /// Paint inside the border box when true.
    pub inset: bool,
}

impl BoxShadow {
    fn is_valid(&self) -> bool {
        self.offset_x.is_finite()
            && self.offset_y.is_finite()
            && self.blur_radius.is_finite()
            && self.blur_radius >= 0.0
            && self.spread_radius.is_finite()
            && self.color.is_valid()
    }
}

/// Fill rule used by polygon and path clips.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FillRule {
    /// Non-zero winding rule.
    NonZero,
    /// Even-odd rule.
    EvenOdd,
}

/// One normalized command in a clip path.
#[derive(Clone, Debug, PartialEq)]
pub enum PathCommand {
    /// Move the current point.
    MoveTo(PaintPosition),
    /// Add a straight line.
    LineTo(PaintPosition),
    /// Add a quadratic Bezier segment.
    QuadraticTo {
        /// Control point.
        control: PaintPosition,
        /// End point.
        end: PaintPosition,
    },
    /// Add a cubic Bezier segment.
    CubicTo {
        /// First control point.
        control_1: PaintPosition,
        /// Second control point.
        control_2: PaintPosition,
        /// End point.
        end: PaintPosition,
    },
    /// Close the current subpath.
    Close,
}

impl PathCommand {
    fn is_valid(&self) -> bool {
        match self {
            Self::MoveTo(point) | Self::LineTo(point) => point.is_valid(),
            Self::QuadraticTo { control, end } => control.is_valid() && end.is_valid(),
            Self::CubicTo {
                control_1,
                control_2,
                end,
            } => control_1.is_valid() && control_2.is_valid() && end.is_valid(),
            Self::Close => true,
        }
    }
}

/// A resolved CSS basic shape used for clipping.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipShape {
    /// Inset rectangle with independent corner radii.
    Inset {
        /// Insets from the selected reference box.
        edges: PaintEdges<PaintCoordinate>,
        /// Corner radii.
        radii: PaintCorners<PaintCornerRadius>,
    },
    /// Circle.
    Circle {
        /// Radius.
        radius: PaintLengthPercentage,
        /// Center.
        center: PaintPosition,
    },
    /// Ellipse.
    Ellipse {
        /// Horizontal radius.
        radius_x: PaintLengthPercentage,
        /// Vertical radius.
        radius_y: PaintLengthPercentage,
        /// Center.
        center: PaintPosition,
    },
    /// Polygon.
    Polygon {
        /// Fill rule.
        fill_rule: FillRule,
        /// Polygon vertices.
        points: Vec<PaintPosition>,
    },
    /// Normalized path commands.
    Path {
        /// Fill rule.
        fill_rule: FillRule,
        /// Path command stream.
        commands: Vec<PathCommand>,
    },
}

impl ClipShape {
    fn is_valid(&self) -> bool {
        let length = PaintLengthPercentage::is_valid;
        match self {
            Self::Inset { edges, radii } => {
                [edges.top, edges.right, edges.bottom, edges.left]
                    .into_iter()
                    .all(PaintCoordinate::is_valid)
                    && [
                        radii.top_left,
                        radii.top_right,
                        radii.bottom_right,
                        radii.bottom_left,
                    ]
                    .into_iter()
                    .all(PaintCornerRadius::is_valid)
            }
            Self::Circle { radius, center } => length(*radius) && center.is_valid(),
            Self::Ellipse {
                radius_x,
                radius_y,
                center,
            } => length(*radius_x) && length(*radius_y) && center.is_valid(),
            Self::Polygon { points, .. } => {
                points.len() >= 3 && points.iter().copied().all(PaintPosition::is_valid)
            }
            Self::Path { commands, .. } => {
                !commands.is_empty() && commands.iter().all(PathCommand::is_valid)
            }
        }
    }
}

/// Alpha/luminance interpretation for a mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaskMode {
    /// Select mode from the source.
    MatchSource,
    /// Use alpha.
    Alpha,
    /// Use luminance.
    Luminance,
}

/// Compositing operator between adjacent mask layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaskComposite {
    /// Add source coverage.
    Add,
    /// Subtract source coverage.
    Subtract,
    /// Intersect coverage.
    Intersect,
    /// Exclude overlapping coverage.
    Exclude,
}

/// One resolved mask layer.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskLayer {
    /// Mask image or gradient.
    pub image: PaintImage,
    /// Position within the origin box.
    pub position: PaintPosition,
    /// Resolved sizing rule.
    pub size: BackgroundSize,
    /// Horizontal repetition.
    pub repeat_x: ImageRepeat,
    /// Vertical repetition.
    pub repeat_y: ImageRepeat,
    /// Positioning box.
    pub origin: PaintBox,
    /// Painting clip box.
    pub clip: PaintBox,
    /// Alpha/luminance interpretation.
    pub mode: MaskMode,
    /// Composition with the layer below.
    pub composite: MaskComposite,
}

impl MaskLayer {
    fn is_valid(&self) -> bool {
        self.image.is_valid() && self.position.is_valid() && self.size.is_valid()
    }
}

/// Whether a node establishes an isolated blending group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Isolation {
    /// Use normal stacking-context rules.
    Auto,
    /// Force an isolated group.
    Isolate,
}

/// Whether the reverse side of a transformed plane is visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackfaceVisibility {
    /// Paint the reverse side.
    Visible,
    /// Cull the reverse side.
    Hidden,
}

/// Whether descendants share the current three-dimensional transform space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransformStyle {
    /// Flatten descendants into this node's plane.
    Flat,
    /// Preserve descendant 3-D positions.
    Preserve3d,
}

/// Raster-image scaling algorithm requested by `image-rendering`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageRendering {
    /// Host default quality selection.
    #[default]
    Auto,
    /// Prefer smooth interpolation.
    Smooth,
    /// Prefer speed over smooth interpolation.
    HighQuality,
    /// Preserve hard pixel edges.
    Pixelated,
    /// Use nearest-neighbor or an equivalent crisp-edge algorithm.
    CrispEdges,
}

/// Complete less-common visual effects for one node.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualEffects {
    /// Blur radius applied to pixels already painted behind the node.
    pub backdrop_blur: Option<f32>,
    /// Optional outline; absence means no outline.
    pub outline: Option<OutlinePaint>,
    /// Box shadows, ordered front to back.
    pub box_shadows: Vec<BoxShadow>,
    /// Optional shape clip in addition to overflow clipping.
    pub clip_path: Option<(PaintBox, ClipShape)>,
    /// Mask layers, ordered front to back.
    pub masks: Vec<MaskLayer>,
    /// Blend mode for the completed node group.
    pub blend_mode: BlendMode,
    /// Stacking-context isolation.
    pub isolation: Isolation,
    /// Back-face painting behavior.
    pub backface_visibility: BackfaceVisibility,
    /// Descendant 3-D flattening behavior.
    pub transform_style: TransformStyle,
    /// Raster-image sampling behavior for images painted by this node.
    pub image_rendering: ImageRendering,
}

impl Default for VisualEffects {
    fn default() -> Self {
        Self {
            backdrop_blur: None,
            outline: None,
            box_shadows: Vec::new(),
            clip_path: None,
            masks: Vec::new(),
            blend_mode: BlendMode::Normal,
            isolation: Isolation::Auto,
            backface_visibility: BackfaceVisibility::Visible,
            transform_style: TransformStyle::Flat,
            image_rendering: ImageRendering::Auto,
        }
    }
}

impl VisualEffects {
    /// Validates all colors, geometry, and scalar ranges.
    pub fn validate(&self) -> bool {
        self.backdrop_blur
            .is_none_or(|radius| radius.is_finite() && radius >= 0.0)
            && self.outline.as_ref().is_none_or(OutlinePaint::is_valid)
            && self.box_shadows.iter().all(BoxShadow::is_valid)
            && self
                .clip_path
                .as_ref()
                .is_none_or(|(_, shape)| shape.is_valid())
            && self.masks.iter().all(MaskLayer::is_valid)
    }
}

/// A platform cursor keyword selected after style resolution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorKeyword {
    /// Host default cursor.
    #[default]
    Auto,
    /// Platform default arrow or equivalent.
    Default,
    /// No cursor.
    None,
    /// Context menu affordance.
    ContextMenu,
    /// Help affordance.
    Help,
    /// Pointing/link affordance.
    Pointer,
    /// Progress without blocking interaction.
    Progress,
    /// Busy/wait affordance.
    Wait,
    /// Cell selection.
    Cell,
    /// Crosshair.
    Crosshair,
    /// Text selection.
    Text,
    /// Vertical text selection.
    VerticalText,
    /// Alias creation.
    Alias,
    /// Copy affordance.
    Copy,
    /// Move affordance.
    Move,
    /// Drop is prohibited.
    NoDrop,
    /// Interaction is prohibited.
    NotAllowed,
    /// Grab affordance.
    Grab,
    /// Active grab affordance.
    Grabbing,
    /// Horizontal resize.
    ColResize,
    /// Vertical resize.
    RowResize,
    /// North resize.
    NResize,
    /// East resize.
    EResize,
    /// South resize.
    SResize,
    /// West resize.
    WResize,
    /// North-east resize.
    NeResize,
    /// North-west resize.
    NwResize,
    /// South-east resize.
    SeResize,
    /// South-west resize.
    SwResize,
    /// Bidirectional east-west resize.
    EwResize,
    /// Bidirectional north-south resize.
    NsResize,
    /// Bidirectional north-east/south-west resize.
    NeswResize,
    /// Bidirectional north-west/south-east resize.
    NwseResize,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
}

/// One custom cursor candidate in author preference order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CursorResource {
    /// Cursor image resource.
    pub resource: ResourceId,
    /// Optional hotspot in resource pixels.
    pub hotspot: Option<(u32, u32)>,
}

/// Complete resolved cursor including resource fallbacks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    /// Custom cursor candidates in author preference order.
    pub resources: Vec<CursorResource>,
    /// Required keyword fallback.
    pub fallback: CursorKeyword,
}

/// Fit mode for replaced image content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectFit {
    /// Distort to fill the content box.
    Fill,
    /// Preserve aspect ratio and fit inside.
    Contain,
    /// Preserve aspect ratio and cover.
    Cover,
    /// Preserve intrinsic dimensions.
    None,
    /// Behave as `none` or `contain`, whichever is smaller.
    ScaleDown,
}

/// Resolved content for an image element.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageContent {
    /// Image resource.
    pub resource: ResourceId,
    /// Fit within the content box.
    pub fit: ObjectFit,
    /// Alignment of the fitted image.
    pub position: PaintPosition,
}

impl ImageContent {
    /// Validates the resolved position.
    pub fn validate(&self) -> bool {
        self.position.is_valid()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color() -> PaintColor {
        PaintColor::Srgba {
            red: 1,
            green: 2,
            blue: 3,
            alpha: 1.0,
        }
    }

    fn stop(position: f32) -> GradientStop {
        GradientStop {
            color: color(),
            position: Some(PaintCoordinate {
                length: 0.0,
                fraction: position,
            }),
        }
    }

    #[test]
    fn gradients_require_finite_geometry_and_two_valid_stops() {
        let valid = PaintImage::LinearGradient {
            angle_degrees: 45.0,
            repeating: false,
            stops: vec![stop(0.0), stop(1.0)],
        };
        assert!(valid.is_valid());
        assert!(
            !PaintImage::LinearGradient {
                angle_degrees: f32::NAN,
                repeating: false,
                stops: vec![stop(0.0), stop(1.0)],
            }
            .is_valid()
        );
        assert!(
            !PaintImage::LinearGradient {
                angle_degrees: 0.0,
                repeating: false,
                stops: vec![stop(0.0)],
            }
            .is_valid()
        );
    }

    #[test]
    fn visual_effect_validation_rejects_each_malformed_numeric_family() {
        let mut effects = VisualEffects::default();
        effects.box_shadows.push(BoxShadow {
            offset_x: 1.0,
            offset_y: 2.0,
            blur_radius: 3.0,
            spread_radius: 4.0,
            color: color(),
            inset: false,
        });
        effects.backdrop_blur = Some(4.0);
        assert!(effects.validate());
        effects.backdrop_blur = Some(-1.0);
        assert!(!effects.validate());
    }

    #[test]
    fn text_shadow_reaches_color_validation_after_valid_geometry() {
        let shadow = TextShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur_radius: 0.0,
            color: PaintColor::Named(String::new()),
        };
        assert!(!shadow.validate());
    }

    #[test]
    fn image_and_clip_validation_cover_remaining_shape_families() {
        let resource = ResourceId::new(1).unwrap();
        assert!(PaintImage::None.is_valid());
        assert!(PaintImage::Resource(resource).is_valid());

        let position = PaintPosition::default();
        let stops = vec![stop(0.0), stop(1.0)];
        let radial = |extent, radii| PaintImage::RadialGradient {
            shape: RadialGradientShape::Circle,
            extent,
            center: position,
            radii,
            repeating: false,
            stops: stops.clone(),
        };
        assert!(!radial(RadialGradientExtent::Explicit, None).is_valid());
        assert!(radial(RadialGradientExtent::ClosestSide, None).is_valid());
        assert!(
            !radial(
                RadialGradientExtent::FarthestSide,
                Some((
                    PaintLengthPercentage::default(),
                    PaintLengthPercentage::default()
                ))
            )
            .is_valid()
        );

        let valid_length = PaintLengthPercentage::default();
        let valid_radius = PaintCornerRadius::default();
        let edges = PaintEdges {
            top: PaintCoordinate::default(),
            right: PaintCoordinate::default(),
            bottom: PaintCoordinate::default(),
            left: PaintCoordinate::default(),
        };
        let radii = PaintCorners {
            top_left: valid_radius,
            top_right: valid_radius,
            bottom_right: valid_radius,
            bottom_left: valid_radius,
        };
        assert!(
            ClipShape::Inset {
                edges: edges.clone(),
                radii: radii.clone(),
            }
            .is_valid()
        );
        let mut invalid_edges = edges;
        invalid_edges.left.length = f32::NAN;
        assert!(
            !ClipShape::Inset {
                edges: invalid_edges,
                radii: radii.clone(),
            }
            .is_valid()
        );
        let mut invalid_radii = radii;
        invalid_radii.bottom_left.horizontal.length = -1.0;
        assert!(
            !ClipShape::Inset {
                edges: PaintEdges {
                    top: PaintCoordinate::default(),
                    right: PaintCoordinate::default(),
                    bottom: PaintCoordinate::default(),
                    left: PaintCoordinate::default(),
                },
                radii: invalid_radii,
            }
            .is_valid()
        );

        assert!(
            ClipShape::Circle {
                radius: valid_length,
                center: position,
            }
            .is_valid()
        );
        assert!(
            !ClipShape::Circle {
                radius: PaintLengthPercentage {
                    length: -1.0,
                    fraction: 0.0,
                },
                center: position,
            }
            .is_valid()
        );
        let invalid_position = PaintPosition {
            x: PaintCoordinate {
                length: f32::NAN,
                fraction: 0.0,
            },
            y: PaintCoordinate::default(),
        };
        assert!(
            !ClipShape::Circle {
                radius: valid_length,
                center: invalid_position,
            }
            .is_valid()
        );
        assert!(
            ClipShape::Ellipse {
                radius_x: valid_length,
                radius_y: valid_length,
                center: position,
            }
            .is_valid()
        );
        for shape in [
            ClipShape::Ellipse {
                radius_x: PaintLengthPercentage {
                    length: -1.0,
                    fraction: 0.0,
                },
                radius_y: valid_length,
                center: position,
            },
            ClipShape::Ellipse {
                radius_x: valid_length,
                radius_y: PaintLengthPercentage {
                    length: -1.0,
                    fraction: 0.0,
                },
                center: position,
            },
            ClipShape::Ellipse {
                radius_x: valid_length,
                radius_y: valid_length,
                center: invalid_position,
            },
        ] {
            assert!(!shape.is_valid());
        }

        let valid_quadratic = PathCommand::QuadraticTo {
            control: position,
            end: position,
        };
        assert!(valid_quadratic.is_valid());
        assert!(
            !PathCommand::QuadraticTo {
                control: invalid_position,
                end: position,
            }
            .is_valid()
        );
        assert!(
            !PathCommand::QuadraticTo {
                control: position,
                end: invalid_position,
            }
            .is_valid()
        );
        let valid_cubic = PathCommand::CubicTo {
            control_1: position,
            control_2: position,
            end: position,
        };
        assert!(valid_cubic.is_valid());
        for command in [
            PathCommand::CubicTo {
                control_1: invalid_position,
                control_2: position,
                end: position,
            },
            PathCommand::CubicTo {
                control_1: position,
                control_2: invalid_position,
                end: position,
            },
            PathCommand::CubicTo {
                control_1: position,
                control_2: position,
                end: invalid_position,
            },
        ] {
            assert!(!command.is_valid());
        }
        assert!(PathCommand::LineTo(position).is_valid());
        assert!(!PathCommand::LineTo(invalid_position).is_valid());
    }

    #[test]
    fn image_content_accepts_signed_finite_positions() {
        let image = ImageContent {
            resource: ResourceId::new(1).unwrap(),
            fit: ObjectFit::Cover,
            position: PaintPosition {
                x: PaintCoordinate {
                    length: -3.0,
                    fraction: 0.5,
                },
                y: PaintCoordinate {
                    length: 2.0,
                    fraction: 1.0,
                },
            },
        };
        assert!(image.validate());
    }

    #[test]
    fn semantic_values_cover_background_clip_mask_filter_text_and_cursor_families() {
        let position = PaintPosition {
            x: PaintCoordinate {
                length: -1.0,
                fraction: 0.5,
            },
            y: PaintCoordinate {
                length: 2.0,
                fraction: 0.25,
            },
        };
        let layer = BackgroundLayer {
            image: PaintImage::RadialGradient {
                shape: RadialGradientShape::Ellipse,
                extent: RadialGradientExtent::Explicit,
                center: position,
                radii: Some((
                    PaintLengthPercentage {
                        length: 10.0,
                        fraction: 0.0,
                    },
                    PaintLengthPercentage {
                        length: 0.0,
                        fraction: 0.5,
                    },
                )),
                repeating: true,
                stops: vec![stop(0.0), stop(1.0)],
            },
            position,
            size: BackgroundSize::Explicit {
                width: None,
                height: Some(PaintLengthPercentage {
                    length: 20.0,
                    fraction: 0.0,
                }),
            },
            repeat_x: ImageRepeat::Round,
            repeat_y: ImageRepeat::Space,
            origin: PaintBox::Content,
            clip: PaintBox::Text,
            attachment: BackgroundAttachment::Local,
            blend_mode: BlendMode::Multiply,
        };
        assert!(layer.validate());

        let shadow = BoxShadow {
            offset_x: 1.0,
            offset_y: -2.0,
            blur_radius: 3.0,
            spread_radius: 0.0,
            color: color(),
            inset: false,
        };
        let mut effects = VisualEffects {
            backdrop_blur: Some(12.0),
            outline: Some(OutlinePaint {
                color: color(),
                style: OutlineLineStyle::Auto,
                width: 2.0,
                offset: -1.0,
            }),
            box_shadows: vec![shadow.clone()],
            clip_path: Some((
                PaintBox::Border,
                ClipShape::Polygon {
                    fill_rule: FillRule::EvenOdd,
                    points: vec![
                        position,
                        PaintPosition::default(),
                        PaintPosition {
                            x: PaintCoordinate {
                                length: 4.0,
                                fraction: 1.0,
                            },
                            y: PaintCoordinate {
                                length: 5.0,
                                fraction: 1.0,
                            },
                        },
                    ],
                },
            )),
            masks: vec![MaskLayer {
                image: PaintImage::ConicGradient {
                    from_degrees: 90.0,
                    center: position,
                    repeating: false,
                    stops: vec![stop(0.0), stop(1.0)],
                },
                position,
                size: BackgroundSize::Contain,
                repeat_x: ImageRepeat::NoRepeat,
                repeat_y: ImageRepeat::NoRepeat,
                origin: PaintBox::Border,
                clip: PaintBox::Padding,
                mode: MaskMode::Alpha,
                composite: MaskComposite::Intersect,
            }],
            blend_mode: BlendMode::Overlay,
            isolation: Isolation::Isolate,
            backface_visibility: BackfaceVisibility::Hidden,
            transform_style: TransformStyle::Preserve3d,
            image_rendering: ImageRendering::Pixelated,
        };
        assert!(effects.validate());
        effects.clip_path = Some((
            PaintBox::Border,
            ClipShape::Path {
                fill_rule: FillRule::NonZero,
                commands: vec![PathCommand::MoveTo(position), PathCommand::Close],
            },
        ));
        assert!(effects.validate());

        let text = TextDecoration {
            lines: TextDecorationLines {
                underline: true,
                overline: false,
                line_through: true,
            },
            color: color(),
            style: TextDecorationStyle::Wavy,
            thickness: TextDecorationThickness::Length(1.5),
        };
        assert!(text.validate());

        let cursor = Cursor {
            resources: vec![CursorResource {
                resource: ResourceId::new(3).unwrap(),
                hotspot: Some((4, 5)),
            }],
            fallback: CursorKeyword::Pointer,
        };
        assert_eq!(cursor.resources.len(), 1);
    }
}
