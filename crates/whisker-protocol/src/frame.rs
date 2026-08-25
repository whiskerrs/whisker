//! Owned semantic frame values.

use crate::{
    CommandId, ElementTypeId, MeasurementPayloadError, NodeId, PointerId, PreparedContentId,
    PropertyId, ResultId, SurfaceId, TextMeasurePayload, WhiskerValue,
};

/// Backend-independent color used by semantic paint operations.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintColor {
    /// A canonical named color retained from the typed authoring value.
    Named(String),
    /// An sRGB color with an alpha channel in `0.0..=1.0`.
    Srgba {
        /// Red channel.
        red: u8,
        /// Green channel.
        green: u8,
        /// Blue channel.
        blue: u8,
        /// Alpha channel.
        alpha: f32,
    },
    /// An HSL color with percentage saturation/lightness and alpha.
    Hsla {
        /// Hue in degrees. Values outside one turn are permitted.
        hue_degrees: f32,
        /// Saturation percentage in `0.0..=100.0`.
        saturation: f32,
        /// Lightness percentage in `0.0..=100.0`.
        lightness: f32,
        /// Alpha channel in `0.0..=1.0`.
        alpha: f32,
    },
}

impl PaintColor {
    pub(crate) fn is_valid(&self) -> bool {
        match self {
            Self::Named(name) => !name.trim().is_empty(),
            Self::Srgba { alpha, .. } => alpha.is_finite() && (0.0..=1.0).contains(alpha),
            Self::Hsla {
                hue_degrees,
                saturation,
                lightness,
                alpha,
            } => {
                hue_degrees.is_finite()
                    && saturation.is_finite()
                    && (0.0..=100.0).contains(saturation)
                    && lightness.is_finite()
                    && (0.0..=100.0).contains(lightness)
                    && alpha.is_finite()
                    && (0.0..=1.0).contains(alpha)
            }
        }
    }
}

impl Default for PaintColor {
    fn default() -> Self {
        Self::Srgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 1.0,
        }
    }
}

/// Resolved paint values for plain text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextPaint {
    /// Foreground glyph color.
    pub foreground: PaintColor,
    /// Resolved line decoration.
    pub decoration: crate::TextDecoration,
    /// Text shadows, ordered front to back.
    pub shadows: Vec<crate::TextShadow>,
}

impl TextPaint {
    /// Returns whether painting requires protocol-minor-1 decoration or shadow
    /// support beyond the original foreground-color path.
    pub fn uses_extended_features(&self) -> bool {
        self.decoration.lines.underline
            || self.decoration.lines.overline
            || self.decoration.lines.line_through
            || !self.shadows.is_empty()
    }
}

/// An affine logical length retaining a border-box-relative fraction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintLengthPercentage {
    /// Absolute logical-pixel component.
    pub length: f32,
    /// Fraction of the relevant border-box axis, where `1.0` is 100 percent.
    pub fraction: f32,
}

impl PaintLengthPercentage {
    pub(crate) fn is_valid(self) -> bool {
        self.length.is_finite()
            && self.length >= 0.0
            && self.fraction.is_finite()
            && self.fraction >= 0.0
    }
}

/// Four physical edges in top, right, bottom, left order.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintEdges<T> {
    /// Top edge.
    pub top: T,
    /// Right edge.
    pub right: T,
    /// Bottom edge.
    pub bottom: T,
    /// Left edge.
    pub left: T,
}

/// Four corners in top-left, top-right, bottom-right, bottom-left order.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintCorners<T> {
    /// Top-left corner.
    pub top_left: T,
    /// Top-right corner.
    pub top_right: T,
    /// Bottom-right corner.
    pub bottom_right: T,
    /// Bottom-left corner.
    pub bottom_left: T,
}

/// Horizontal and vertical radius of one rounded corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintCornerRadius {
    /// Horizontal radius, resolved against the border-box width.
    pub horizontal: PaintLengthPercentage,
    /// Vertical radius, resolved against the border-box height.
    pub vertical: PaintLengthPercentage,
}

impl PaintCornerRadius {
    /// Creates a circular radius from one CSS value.
    pub const fn circular(value: PaintLengthPercentage) -> Self {
        Self {
            horizontal: value,
            vertical: value,
        }
    }

    /// Returns whether both axes carry identical resolved values.
    pub fn is_circular(self) -> bool {
        self.horizontal == self.vertical
    }

    pub(crate) fn is_valid(self) -> bool {
        self.horizontal.is_valid() && self.vertical.is_valid()
    }
}

/// Renderer-independent border line style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BorderLineStyle {
    /// No line is painted.
    None,
    /// Hidden line, equivalent to none outside table conflict resolution.
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

/// Resolved background and border paint for one box.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxPaint {
    /// Background color painted behind content and borders.
    pub background_color: PaintColor,
    /// Border widths retaining their percentage components.
    pub border_widths: PaintEdges<PaintLengthPercentage>,
    /// Border colors.
    pub border_colors: PaintEdges<PaintColor>,
    /// Border line styles.
    pub border_styles: PaintEdges<BorderLineStyle>,
    /// Corner radii retaining their border-box percentage components.
    pub border_radii: PaintCorners<PaintCornerRadius>,
}

impl Default for BoxPaint {
    fn default() -> Self {
        let transparent = PaintColor::Srgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0.0,
        };
        Self {
            background_color: transparent.clone(),
            border_widths: PaintEdges {
                top: PaintLengthPercentage::default(),
                right: PaintLengthPercentage::default(),
                bottom: PaintLengthPercentage::default(),
                left: PaintLengthPercentage::default(),
            },
            border_colors: PaintEdges {
                top: transparent.clone(),
                right: transparent.clone(),
                bottom: transparent.clone(),
                left: transparent,
            },
            border_styles: PaintEdges {
                top: BorderLineStyle::None,
                right: BorderLineStyle::None,
                bottom: BorderLineStyle::None,
                left: BorderLineStyle::None,
            },
            border_radii: PaintCorners {
                top_left: PaintCornerRadius::default(),
                top_right: PaintCornerRadius::default(),
                bottom_right: PaintCornerRadius::default(),
                bottom_left: PaintCornerRadius::default(),
            },
        }
    }
}

impl BoxPaint {
    /// Validates every numeric and color component.
    pub fn validate(&self) -> bool {
        self.background_color.is_valid()
            && [
                self.border_widths.top,
                self.border_widths.right,
                self.border_widths.bottom,
                self.border_widths.left,
            ]
            .into_iter()
            .all(PaintLengthPercentage::is_valid)
            && [
                self.border_radii.top_left,
                self.border_radii.top_right,
                self.border_radii.bottom_right,
                self.border_radii.bottom_left,
            ]
            .into_iter()
            .all(PaintCornerRadius::is_valid)
            && [
                &self.border_colors.top,
                &self.border_colors.right,
                &self.border_colors.bottom,
                &self.border_colors.left,
            ]
            .into_iter()
            .all(PaintColor::is_valid)
    }
}

/// Descendant overflow behavior on one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverflowClip {
    /// Allow paint outside the border box.
    Visible,
    /// Clip paint to the border box and its corner radii.
    Hidden,
}

/// Semantic clip applied to a node's descendants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoxClip {
    /// Horizontal overflow behavior.
    pub horizontal: OverflowClip,
    /// Vertical overflow behavior.
    pub vertical: OverflowClip,
}

/// A malformed plain-text presentation payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextContentError {
    /// Text shaping or line-breaking input was invalid.
    InvalidMeasurement(MeasurementPayloadError),
    /// The resolved foreground color was invalid.
    InvalidPaint,
}

/// Protocol major version implemented by this semantic model.
pub const PROTOCOL_MAJOR: u16 = 1;

/// Protocol minor version implemented by this semantic model.
pub const PROTOCOL_MINOR: u16 = 1;

/// A negotiated frame protocol version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion {
    /// Breaking protocol generation.
    pub major: u16,
    /// Backward-compatible feature generation within one major version.
    pub minor: u16,
}

impl ProtocolVersion {
    /// Version supported by this crate.
    pub const CURRENT: Self = Self {
        major: PROTOCOL_MAJOR,
        minor: PROTOCOL_MINOR,
    };
}

/// Whether a packet replaces or advances the receiver projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameMode {
    /// Replaces all nodes and starts the declared scene epoch.
    Snapshot,
    /// Advances the existing scene from `base_revision`.
    Delta,
}

/// Metadata that orders a frame within one surface and scene epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameHeader {
    /// Protocol version used by the packet.
    pub version: ProtocolVersion,
    /// Destination surface.
    pub surface: SurfaceId,
    /// Generation in which node identifiers remain unique.
    pub scene_epoch: u32,
    /// Monotonically increasing diagnostic frame identifier.
    pub frame_id: u64,
    /// Revision the receiver must currently have for a delta.
    pub base_revision: u64,
    /// Revision accepted after the complete packet succeeds.
    pub target_revision: u64,
    /// Viewport/environment generation used to compute geometry.
    pub viewport_epoch: u32,
    /// Snapshot or incremental transaction.
    pub mode: FrameMode,
}

/// One owned frame transaction before packed wire encoding.
#[derive(Clone, Debug, PartialEq)]
pub struct FramePacket {
    /// Transaction metadata.
    pub header: FrameHeader,
    /// Operations applied in order after complete validation succeeds.
    pub operations: Vec<Operation>,
}

/// Backend-independent rectangle in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutRect {
    /// Horizontal offset from the parent content origin.
    pub x: f32,
    /// Vertical offset from the parent content origin.
    pub y: f32,
    /// Border-box width.
    pub width: f32,
    /// Border-box height.
    pub height: f32,
}

/// Complete box geometry needed by a Host to paint content without
/// reconstructing layout-engine inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutGeometry {
    /// Border box relative to the parent border-box origin.
    pub border_box: LayoutRect,
    /// Content box relative to this node's border-box origin.
    pub content_box: LayoutRect,
}

impl LayoutGeometry {
    /// Returns whether every coordinate is finite and every extent is
    /// non-negative.
    pub fn is_valid(self) -> bool {
        [self.border_box, self.content_box].into_iter().all(|rect| {
            [rect.x, rect.y, rect.width, rect.height]
                .into_iter()
                .all(f32::is_finite)
                && rect.width >= 0.0
                && rect.height >= 0.0
        })
    }
}

impl From<LayoutRect> for LayoutGeometry {
    fn from(border_box: LayoutRect) -> Self {
        Self {
            content_box: LayoutRect {
                width: border_box.width.max(0.0),
                height: border_box.height.max(0.0),
                ..LayoutRect::default()
            },
            border_box,
        }
    }
}

/// A column-major 4-by-4 transform matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform(pub [f32; 16]);

impl Transform {
    /// Identity transform.
    pub const IDENTITY: Self = Self([
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
}

/// Whether a node participates in presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// The node is presented normally.
    Visible,
    /// The node retains scene state but is not presented.
    Hidden,
}

/// How Host hit testing treats a node and its descendants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HitTestBehavior {
    /// The node and descendants use their normal hit-test rules.
    Auto,
    /// Neither the node nor its descendants receives pointer input.
    None,
    /// The node may receive input but its descendants do not.
    BoxOnly,
    /// Descendants may receive input but the node itself does not.
    DescendantsOnly,
}

/// Plain-text presentation selected after intrinsic measurement.
///
/// `payload` repeats the semantic shaping inputs used for measurement so a
/// renderer can validate that presentation still matches those metrics.
/// `prepared_content` identifies the exact Host-shaped object when the
/// measurement provider retained one for painting.
#[derive(Clone, Debug, PartialEq)]
pub struct TextContent {
    /// UTF-8 content and resolved metric-affecting text inputs.
    pub payload: TextMeasurePayload,
    /// Resolved values that affect painting but not intrinsic measurement.
    pub paint: TextPaint,
    /// Host object produced by the accepted measurement, when available.
    pub prepared_content: Option<PreparedContentId>,
}

impl TextContent {
    /// Validates the contained shaping and paint inputs.
    pub fn validate(&self) -> Result<(), TextContentError> {
        self.payload
            .validate()
            .map_err(TextContentError::InvalidMeasurement)?;
        if !self.paint.foreground.is_valid()
            || !self.paint.decoration.validate()
            || !self.paint.shadows.iter().all(crate::TextShadow::validate)
        {
            return Err(TextContentError::InvalidPaint);
        }
        Ok(())
    }
}

/// A semantic mutation within a [`FramePacket`].
#[derive(Clone, Debug, PartialEq)]
pub enum Operation {
    /// Creates an unattached node.
    CreateNode {
        /// New node identifier.
        node: NodeId,
        /// Negotiated element type.
        element_type: ElementTypeId,
    },
    /// Deletes a node and its complete attached subtree.
    DeleteNode {
        /// Subtree root to delete.
        node: NodeId,
    },
    /// Attaches an unattached child at an index.
    InsertChild {
        /// Destination parent.
        parent: NodeId,
        /// Unattached child.
        child: NodeId,
        /// Position in the resulting child list.
        index: u32,
    },
    /// Detaches a direct child without deleting it.
    RemoveChild {
        /// Current parent.
        parent: NodeId,
        /// Direct child to detach.
        child: NodeId,
    },
    /// Reorders a direct child within its current parent.
    MoveChild {
        /// Current parent.
        parent: NodeId,
        /// Direct child to move.
        child: NodeId,
        /// Position after removing the child from its old position.
        index: u32,
    },
    /// Sets resolved box geometry.
    SetLayout {
        /// Target node.
        node: NodeId,
        /// Resolved border-box and content-box geometry.
        geometry: LayoutGeometry,
    },
    /// Sets resolved background and border paint.
    SetBoxPaint {
        /// Target node.
        node: NodeId,
        /// Resolved paint values.
        paint: BoxPaint,
    },
    /// Replaces resolved background image layers, ordered front to back.
    SetBackgroundLayers {
        /// Target node.
        node: NodeId,
        /// Complete background layer list; an empty list clears all images.
        layers: Vec<crate::BackgroundLayer>,
    },
    /// Replaces resolved outline, shadow, mask, filter, and group effects.
    SetVisualEffects {
        /// Target node.
        node: NodeId,
        /// Complete visual-effect state.
        effects: crate::VisualEffects,
    },
    /// Sets descendant overflow clipping.
    SetClip {
        /// Target node.
        node: NodeId,
        /// Resolved clip behavior.
        clip: BoxClip,
    },
    /// Sets the resolved transform matrix.
    SetTransform {
        /// Target node.
        node: NodeId,
        /// Resolved transform.
        transform: Transform,
    },
    /// Sets resolved opacity.
    SetOpacity {
        /// Target node.
        node: NodeId,
        /// Opacity in the inclusive range `0.0..=1.0`.
        opacity: f32,
    },
    /// Sets whether a node is presented.
    SetVisibility {
        /// Target node.
        node: NodeId,
        /// Resolved visibility.
        visibility: Visibility,
    },
    /// Sets resolved sibling stacking order.
    SetZOrder {
        /// Target node.
        node: NodeId,
        /// Backend-independent stacking order.
        z_order: i32,
    },
    /// Sets plain UTF-8 text presentation and its optional prepared Host object.
    SetText {
        /// Target node.
        node: NodeId,
        /// Resolved text inputs used for both measurement and painting.
        content: TextContent,
    },
    /// Sets replaced image content for an image-capable element.
    SetImage {
        /// Target node.
        node: NodeId,
        /// Resolved image resource and fitting behavior.
        content: crate::ImageContent,
    },
    /// Sets a typed common or element-specific property.
    SetProperty {
        /// Target node.
        node: NodeId,
        /// Negotiated property identifier.
        property: PropertyId,
        /// Typed property payload.
        value: WhiskerValue,
    },
    /// Restores a property to its schema-defined absence/default state.
    ClearProperty {
        /// Target node.
        node: NodeId,
        /// Negotiated property identifier.
        property: PropertyId,
    },
    /// Replaces the event subscription bitset for a node.
    SetEventMask {
        /// Target node.
        node: NodeId,
        /// Negotiated event bits.
        event_mask: u64,
    },
    /// Sets Host hit-test participation.
    SetHitTest {
        /// Target node.
        node: NodeId,
        /// Resolved behavior.
        behavior: HitTestBehavior,
    },
    /// Sets the resolved pointing-device cursor.
    SetCursor {
        /// Target node.
        node: NodeId,
        /// Cursor selected after style resolution.
        cursor: crate::Cursor,
    },
    /// Captures a pointer stream for a node.
    SetPointerCapture {
        /// Target node.
        node: NodeId,
        /// Pointer to capture.
        pointer: PointerId,
    },
    /// Releases a pointer stream previously captured by a node.
    ReleasePointerCapture {
        /// Target node.
        node: NodeId,
        /// Pointer to release.
        pointer: PointerId,
    },
    /// Invokes an element command after preceding mutations.
    InvokeCommand {
        /// Target node.
        node: NodeId,
        /// Negotiated command identifier.
        command: CommandId,
        /// Typed command arguments.
        arguments: WhiskerValue,
        /// Optional asynchronous result correlation.
        result: Option<ResultId>,
    },
}

impl Operation {
    /// Returns the node that must exist when this operation executes.
    pub fn target_node(&self) -> Option<NodeId> {
        match self {
            Self::CreateNode { .. } => None,
            Self::DeleteNode { node }
            | Self::SetLayout { node, .. }
            | Self::SetBoxPaint { node, .. }
            | Self::SetBackgroundLayers { node, .. }
            | Self::SetVisualEffects { node, .. }
            | Self::SetClip { node, .. }
            | Self::SetTransform { node, .. }
            | Self::SetOpacity { node, .. }
            | Self::SetVisibility { node, .. }
            | Self::SetZOrder { node, .. }
            | Self::SetText { node, .. }
            | Self::SetImage { node, .. }
            | Self::SetProperty { node, .. }
            | Self::ClearProperty { node, .. }
            | Self::SetEventMask { node, .. }
            | Self::SetHitTest { node, .. }
            | Self::SetCursor { node, .. }
            | Self::SetPointerCapture { node, .. }
            | Self::ReleasePointerCapture { node, .. }
            | Self::InvokeCommand { node, .. } => Some(*node),
            Self::InsertChild { parent, .. }
            | Self::RemoveChild { parent, .. }
            | Self::MoveChild { parent, .. } => Some(*parent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(value: u64) -> NodeId {
        NodeId::new(value).expect("test node")
    }

    fn length(length: f32, fraction: f32) -> PaintLengthPercentage {
        PaintLengthPercentage { length, fraction }
    }

    fn radius(length_value: f32, fraction: f32) -> PaintCornerRadius {
        PaintCornerRadius::circular(length(length_value, fraction))
    }

    fn box_paint() -> BoxPaint {
        BoxPaint {
            background_color: PaintColor::Named("transparent".into()),
            border_widths: PaintEdges {
                top: length(0.0, 0.0),
                right: length(1.0, 0.0),
                bottom: length(0.0, 0.5),
                left: length(1.0, 0.5),
            },
            border_colors: PaintEdges {
                top: PaintColor::default(),
                right: PaintColor::Named("red".into()),
                bottom: PaintColor::default(),
                left: PaintColor::Named("blue".into()),
            },
            border_styles: PaintEdges {
                top: BorderLineStyle::None,
                right: BorderLineStyle::Solid,
                bottom: BorderLineStyle::Dashed,
                left: BorderLineStyle::Dotted,
            },
            border_radii: PaintCorners {
                top_left: radius(0.0, 0.0),
                top_right: radius(2.0, 0.0),
                bottom_right: radius(0.0, 0.25),
                bottom_left: radius(2.0, 0.25),
            },
        }
    }

    #[test]
    fn paint_colors_validate_every_semantic_form_and_range() {
        assert!(PaintColor::Named("red".into()).is_valid());
        assert!(!PaintColor::Named("  ".into()).is_valid());

        for alpha in [f32::NAN, -0.1, 1.1] {
            assert!(
                !PaintColor::Srgba {
                    red: 1,
                    green: 2,
                    blue: 3,
                    alpha,
                }
                .is_valid()
            );
        }
        assert!(
            PaintColor::Srgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: 0.5,
            }
            .is_valid()
        );

        let hsla = |hue_degrees, saturation, lightness, alpha| PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        };
        assert!(hsla(720.0, 50.0, 25.0, 1.0).is_valid());
        for color in [
            hsla(f32::NAN, 50.0, 25.0, 1.0),
            hsla(0.0, f32::NAN, 25.0, 1.0),
            hsla(0.0, -0.1, 25.0, 1.0),
            hsla(0.0, 100.1, 25.0, 1.0),
            hsla(0.0, 50.0, f32::NAN, 1.0),
            hsla(0.0, 50.0, -0.1, 1.0),
            hsla(0.0, 50.0, 100.1, 1.0),
            hsla(0.0, 50.0, 25.0, f32::NAN),
            hsla(0.0, 50.0, 25.0, -0.1),
            hsla(0.0, 50.0, 25.0, 1.1),
        ] {
            assert!(!color.is_valid());
        }

        let mut content = TextContent {
            payload: crate::TextMeasurePayload {
                text: "paint".into(),
                style: crate::TextMeasureStyle {
                    font_families: vec![crate::MeasureFontFamily::System],
                    font_size: 14.0,
                    font_weight: 400,
                    font_style: crate::MeasureFontStyle::Normal,
                    line_height: crate::MeasureLineHeight::Normal,
                    letter_spacing: 0.0,
                    ..crate::TextMeasureStyle::default()
                },
                locale: None,
                direction: crate::MeasureTextDirection::Auto,
                alignment: crate::MeasureTextAlignment::Start,
                wrap: crate::MeasureTextWrap::Wrap,
                max_lines: None,
                overflow: crate::MeasureTextOverflow::Clip,
            },
            paint: TextPaint::default(),
            prepared_content: None,
        };
        assert_eq!(content.validate(), Ok(()));
        content.paint.foreground = PaintColor::Named(String::new());
        assert_eq!(content.validate(), Err(TextContentError::InvalidPaint));
        content.paint.foreground = PaintColor::default();
        content.paint.shadows.push(crate::TextShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur_radius: -1.0,
            color: PaintColor::default(),
        });
        assert_eq!(content.validate(), Err(TextContentError::InvalidPaint));
    }

    #[test]
    fn text_paint_detects_each_extended_feature_independently() {
        let mut paint = TextPaint::default();
        assert!(!paint.uses_extended_features());
        paint.decoration.lines.underline = true;
        assert!(paint.uses_extended_features());
        paint.decoration.lines.underline = false;
        paint.decoration.lines.overline = true;
        assert!(paint.uses_extended_features());
        paint.decoration.lines.overline = false;
        paint.decoration.lines.line_through = true;
        assert!(paint.uses_extended_features());
        paint.decoration.lines.line_through = false;
        paint.shadows.push(crate::TextShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur_radius: 0.0,
            color: PaintColor::default(),
        });
        assert!(paint.uses_extended_features());
    }

    #[test]
    fn box_paint_validates_lengths_colors_and_short_circuit_paths() {
        assert!(box_paint().validate());

        for invalid in [
            length(f32::NAN, 0.0),
            length(-1.0, 0.0),
            length(0.0, f32::NAN),
            length(0.0, -1.0),
        ] {
            assert!(!invalid.is_valid());
            let mut paint = box_paint();
            paint.border_widths.top = invalid;
            assert!(!paint.validate());
        }

        let mut paint = box_paint();
        paint.background_color = PaintColor::Named(String::new());
        assert!(!paint.validate());
        let mut paint = box_paint();
        paint.border_colors.left = PaintColor::Named(String::new());
        assert!(!paint.validate());
    }

    #[test]
    fn layout_geometry_validates_border_and_content_boxes() {
        let border = LayoutRect {
            x: -2.0,
            y: 3.0,
            width: 20.0,
            height: 10.0,
        };
        let geometry = LayoutGeometry::from(border);
        assert_eq!(geometry.border_box, border);
        assert_eq!(geometry.content_box.width, 20.0);
        assert_eq!(geometry.content_box.height, 10.0);
        assert!(geometry.is_valid());

        let negative_border = LayoutGeometry::from(LayoutRect {
            width: -1.0,
            height: -2.0,
            ..LayoutRect::default()
        });
        assert_eq!(negative_border.content_box.width, 0.0);
        assert_eq!(negative_border.content_box.height, 0.0);
        assert!(!negative_border.is_valid());

        let mut invalid_content = geometry;
        invalid_content.content_box.height = f32::NAN;
        assert!(!invalid_content.is_valid());
        invalid_content.content_box.height = -1.0;
        assert!(!invalid_content.is_valid());
    }

    #[test]
    fn target_node_covers_every_operation_group() {
        let target = node(1);
        let child = node(2);
        let element_type = ElementTypeId::new(1).expect("test element type");
        let property = PropertyId::new(1).expect("test property");
        let pointer = PointerId::new(1).expect("test pointer");
        let command = CommandId::new(1).expect("test command");

        let operations = [
            Operation::CreateNode {
                node: target,
                element_type,
            },
            Operation::DeleteNode { node: target },
            Operation::InsertChild {
                parent: target,
                child,
                index: 0,
            },
            Operation::RemoveChild {
                parent: target,
                child,
            },
            Operation::MoveChild {
                parent: target,
                child,
                index: 0,
            },
            Operation::SetLayout {
                node: target,
                geometry: LayoutGeometry::default(),
            },
            Operation::SetBoxPaint {
                node: target,
                paint: box_paint(),
            },
            Operation::SetBackgroundLayers {
                node: target,
                layers: Vec::new(),
            },
            Operation::SetVisualEffects {
                node: target,
                effects: crate::VisualEffects::default(),
            },
            Operation::SetClip {
                node: target,
                clip: BoxClip {
                    horizontal: OverflowClip::Visible,
                    vertical: OverflowClip::Hidden,
                },
            },
            Operation::SetTransform {
                node: target,
                transform: Transform::IDENTITY,
            },
            Operation::SetOpacity {
                node: target,
                opacity: 1.0,
            },
            Operation::SetVisibility {
                node: target,
                visibility: Visibility::Visible,
            },
            Operation::SetZOrder {
                node: target,
                z_order: 0,
            },
            Operation::SetText {
                node: target,
                content: TextContent {
                    payload: crate::TextMeasurePayload {
                        text: "hello".into(),
                        style: crate::TextMeasureStyle {
                            font_families: vec![crate::MeasureFontFamily::System],
                            font_size: 14.0,
                            font_weight: 400,
                            font_style: crate::MeasureFontStyle::Normal,
                            line_height: crate::MeasureLineHeight::Normal,
                            letter_spacing: 0.0,
                            ..crate::TextMeasureStyle::default()
                        },
                        locale: None,
                        direction: crate::MeasureTextDirection::Auto,
                        alignment: crate::MeasureTextAlignment::Start,
                        wrap: crate::MeasureTextWrap::Wrap,
                        max_lines: None,
                        overflow: crate::MeasureTextOverflow::Clip,
                    },
                    paint: TextPaint::default(),
                    prepared_content: None,
                },
            },
            Operation::SetImage {
                node: target,
                content: crate::ImageContent {
                    resource: crate::ResourceId::new(1).unwrap(),
                    fit: crate::ObjectFit::Contain,
                    position: crate::PaintPosition::default(),
                },
            },
            Operation::SetProperty {
                node: target,
                property,
                value: WhiskerValue::Null,
            },
            Operation::ClearProperty {
                node: target,
                property,
            },
            Operation::SetEventMask {
                node: target,
                event_mask: 0,
            },
            Operation::SetHitTest {
                node: target,
                behavior: HitTestBehavior::Auto,
            },
            Operation::SetCursor {
                node: target,
                cursor: crate::Cursor {
                    resources: Vec::new(),
                    fallback: crate::CursorKeyword::Pointer,
                },
            },
            Operation::SetPointerCapture {
                node: target,
                pointer,
            },
            Operation::ReleasePointerCapture {
                node: target,
                pointer,
            },
            Operation::InvokeCommand {
                node: target,
                command,
                arguments: WhiskerValue::Null,
                result: None,
            },
        ];

        assert_eq!(operations[0].target_node(), None);
        for operation in &operations[1..] {
            assert_eq!(operation.target_node(), Some(target));
        }
    }

    #[test]
    fn semantic_values_represent_every_owned_value_shape() {
        let values = [
            WhiskerValue::Null,
            WhiskerValue::Bool(true),
            WhiskerValue::Int(-1),
            WhiskerValue::Float(0.5),
            WhiskerValue::String("value".into()),
            WhiskerValue::Bytes(vec![1, 2]),
            WhiskerValue::Array(vec![WhiskerValue::Null]),
            WhiskerValue::map([("key", WhiskerValue::Bool(false))]),
        ];

        assert_eq!(values.len(), 8);
        assert_ne!(Visibility::Visible, Visibility::Hidden);
        assert_ne!(HitTestBehavior::None, HitTestBehavior::BoxOnly);
        assert_ne!(HitTestBehavior::Auto, HitTestBehavior::DescendantsOnly);
    }
}
