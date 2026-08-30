//! Renderer capability negotiation for optional semantic protocol groups.

use std::error::Error;
use std::fmt;

use crate::{FramePacket, Operation, ProtocolVersion};

/// An independently negotiable renderer feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RenderCapability {
    /// Independent horizontal and vertical border radii.
    EllipticalBorderRadius,
    /// Background image and gradient layers.
    BackgroundLayers,
    /// Outline, shadow, clip-path, mask, filter, and compositing effects.
    VisualEffects,
    /// Text decoration and text shadows.
    TextEffects,
    /// OpenType feature, variation, and optical-sizing controls.
    TextTypography,
    /// Replaced image content.
    ImageContent,
    /// Keyword or resource-backed pointing-device cursors.
    Cursor,
    /// Resource load, readiness, failure, and release messages.
    ResourceLifecycle,
    /// One resolved, non-repeating linear-gradient background image using the
    /// initial layer geometry and explicit color-stop positions.
    LinearGradients,
    /// One resolved, non-repeating explicit radial-gradient background image
    /// using the initial layer geometry and explicit color-stop positions.
    RadialGradients,
    /// One resolved, non-repeating conic-gradient background image using the
    /// initial layer geometry and explicit fractional color-stop positions.
    ConicGradients,
    /// Explicit two-axis geometry for otherwise supported background images.
    BackgroundGeometry,
    /// Ordered stacking of multiple otherwise independently supported
    /// background layers.
    BackgroundLayerStacking,
    /// Resource-backed images used by otherwise supported background layers.
    BackgroundImageResources,
    /// Blur applied to pixels already painted behind a node.
    BackdropBlur,
}

impl RenderCapability {
    /// Every optional capability in stable declaration order.
    pub const ALL: [Self; 15] = [
        Self::EllipticalBorderRadius,
        Self::BackgroundLayers,
        Self::VisualEffects,
        Self::TextEffects,
        Self::TextTypography,
        Self::ImageContent,
        Self::Cursor,
        Self::ResourceLifecycle,
        Self::LinearGradients,
        Self::RadialGradients,
        Self::ConicGradients,
        Self::BackgroundGeometry,
        Self::BackgroundLayerStacking,
        Self::BackgroundImageResources,
        Self::BackdropBlur,
    ];

    /// Stable diagnostic spelling shared by Host errors and checklists.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EllipticalBorderRadius => "elliptical-border-radius",
            Self::BackgroundLayers => "background-layers",
            Self::VisualEffects => "visual-effects",
            Self::TextEffects => "text-effects",
            Self::TextTypography => "text-typography",
            Self::ImageContent => "image-content",
            Self::Cursor => "cursor",
            Self::ResourceLifecycle => "resource-lifecycle",
            Self::LinearGradients => "linear-gradients",
            Self::RadialGradients => "radial-gradients",
            Self::ConicGradients => "conic-gradients",
            Self::BackgroundGeometry => "background-geometry",
            Self::BackgroundLayerStacking => "background-layer-stacking",
            Self::BackgroundImageResources => "background-image-resources",
            Self::BackdropBlur => "backdrop-blur",
        }
    }

    /// Stable bit used by compact Host capability profiles.
    pub const fn mask(self) -> u64 {
        1 << self as u8
    }
}

/// How a Host realizes one semantic capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CapabilitySupport {
    /// The Host implements the semantics directly with its platform renderer.
    Native,
    /// The Host produces conforming behavior through an explicit adaptation.
    Emulated,
    /// The Host rejects the semantics before mutating retained state.
    Unsupported,
}

/// One declared capability and its implementation mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityEntry {
    /// Semantic feature.
    pub capability: RenderCapability,
    /// Host implementation mode.
    pub support: CapabilitySupport,
}

/// Error returned when a Host declares an ambiguous capability profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuplicateCapability(pub RenderCapability);

/// A compact Host profile contained unknown or contradictory capability bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidCapabilityMasks {
    /// At least one bit is not assigned by this protocol implementation.
    Unknown {
        /// Unrecognized bits across both masks.
        bits: u64,
    },
    /// The same capability was declared both native and emulated.
    Overlapping {
        /// Bits present in both masks.
        bits: u64,
    },
}

impl fmt::Display for InvalidCapabilityMasks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { bits } => {
                write!(formatter, "unknown Host capability bits 0x{bits:016x}")
            }
            Self::Overlapping { bits } => write!(
                formatter,
                "Host capabilities are both native and emulated in bits 0x{bits:016x}"
            ),
        }
    }
}

impl Error for InvalidCapabilityMasks {}

/// Failure to select one protocol version shared by Rust and its Host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityNegotiationError {
    /// The two sides implement different breaking protocol generations.
    IncompatibleProtocol {
        /// Highest protocol version understood by the Rust runtime.
        runtime: ProtocolVersion,
        /// Highest protocol version advertised by the Host.
        host: ProtocolVersion,
    },
}

impl fmt::Display for CapabilityNegotiationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncompatibleProtocol { runtime, host } => write!(
                formatter,
                "incompatible frame protocol: Rust {}.{}, Host {}.{}",
                runtime.major, runtime.minor, host.major, host.minor
            ),
        }
    }
}

impl Error for CapabilityNegotiationError {}

/// Protocol version and optional semantic features supported by one Host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderCapabilities {
    protocol: ProtocolVersion,
    native: u64,
    emulated: u64,
}

impl RenderCapabilities {
    /// Builds a profile, rejecting duplicate feature declarations.
    pub fn new(
        protocol: ProtocolVersion,
        entries: impl IntoIterator<Item = CapabilityEntry>,
    ) -> Result<Self, DuplicateCapability> {
        let mut seen = 0;
        let mut native = 0;
        let mut emulated = 0;
        for entry in entries {
            let bit = entry.capability.mask();
            if seen & bit != 0 {
                return Err(DuplicateCapability(entry.capability));
            }
            seen |= bit;
            match entry.support {
                CapabilitySupport::Native => native |= bit,
                CapabilitySupport::Emulated => emulated |= bit,
                CapabilitySupport::Unsupported => {}
            }
        }
        Ok(Self {
            protocol,
            native,
            emulated,
        })
    }

    /// Decodes a compact Host profile crossing an FFI or WASM seam.
    pub fn from_masks(
        protocol: ProtocolVersion,
        native: u64,
        emulated: u64,
    ) -> Result<Self, InvalidCapabilityMasks> {
        let known = RenderCapability::ALL
            .into_iter()
            .fold(0, |mask, capability| mask | capability.mask());
        let unknown = (native | emulated) & !known;
        if unknown != 0 {
            return Err(InvalidCapabilityMasks::Unknown { bits: unknown });
        }
        let overlapping = native & emulated;
        if overlapping != 0 {
            return Err(InvalidCapabilityMasks::Overlapping { bits: overlapping });
        }
        Ok(Self {
            protocol,
            native,
            emulated,
        })
    }

    /// Builds a profile that implements the base protocol but no optional feature.
    pub fn base() -> Self {
        Self {
            protocol: ProtocolVersion::CURRENT,
            native: 0,
            emulated: 0,
        }
    }

    /// Builds a reference frame receiver profile for every frame-carried feature.
    ///
    /// Resource lifecycle is deliberately omitted because it uses a separate
    /// service boundary rather than [`crate::FramePacket`].
    pub fn all_frame_native() -> Self {
        let native = RenderCapability::ALL
            .into_iter()
            .filter(|capability| *capability != RenderCapability::ResourceLifecycle)
            .fold(0, |bits, capability| bits | capability.mask());
        Self {
            protocol: ProtocolVersion::CURRENT,
            native,
            emulated: 0,
        }
    }

    /// Returns the highest protocol version understood by the Host.
    pub const fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    /// Compact mask of capabilities implemented directly by the Host.
    pub const fn native_mask(&self) -> u64 {
        self.native
    }

    /// Compact mask of capabilities implemented through a conforming adaptation.
    pub const fn emulated_mask(&self) -> u64 {
        self.emulated
    }

    /// Selects the highest protocol minor understood by both sides.
    ///
    /// Host capability support is retained because it describes the concrete
    /// renderer attached to the surface rather than the Rust implementation.
    pub const fn negotiate(
        self,
        runtime: ProtocolVersion,
    ) -> Result<Self, CapabilityNegotiationError> {
        if runtime.major != self.protocol.major {
            return Err(CapabilityNegotiationError::IncompatibleProtocol {
                runtime,
                host: self.protocol,
            });
        }
        Ok(Self {
            protocol: ProtocolVersion {
                major: runtime.major,
                minor: if runtime.minor < self.protocol.minor {
                    runtime.minor
                } else {
                    self.protocol.minor
                },
            },
            native: self.native,
            emulated: self.emulated,
        })
    }

    /// Returns whether the Host understands this protocol version.
    pub const fn supports_protocol(&self, version: ProtocolVersion) -> bool {
        version.major == self.protocol.major && version.minor <= self.protocol.minor
    }

    /// Returns how the Host implements a feature; omitted features are unsupported.
    pub fn support(&self, capability: RenderCapability) -> CapabilitySupport {
        let bit = capability.mask();
        if self.native & bit != 0 {
            CapabilitySupport::Native
        } else if self.emulated & bit != 0 {
            CapabilitySupport::Emulated
        } else {
            CapabilitySupport::Unsupported
        }
    }

    /// Returns whether the Host can preserve the requested semantics.
    pub fn supports(&self, capability: RenderCapability) -> bool {
        self.support(capability) != CapabilitySupport::Unsupported
    }

    /// Returns the first capability required by a packet that this Host rejects.
    pub fn first_unsupported(&self, packet: &FramePacket) -> Option<RenderCapability> {
        for operation in &packet.operations {
            for capability in operation_capabilities(operation).into_iter().flatten() {
                if !self.supports(capability) {
                    return Some(capability);
                }
            }
        }
        None
    }
}

impl FramePacket {
    /// Returns the deduplicated optional semantic capabilities used by this frame.
    pub fn required_capabilities(&self) -> Vec<RenderCapability> {
        let mut result = Vec::new();
        for operation in &self.operations {
            for capability in operation_capabilities(operation).into_iter().flatten() {
                if !result.contains(&capability) {
                    result.push(capability);
                }
            }
        }
        result
    }
}

fn operation_capabilities(operation: &Operation) -> [Option<RenderCapability>; 6] {
    if let Operation::SetBackgroundLayers { layers, .. } = operation {
        return background_capabilities(layers);
    }
    if let Operation::SetVisualEffects { effects, .. } = operation {
        return visual_effect_capabilities(effects);
    }
    let first = match operation {
        Operation::SetBoxPaint { paint, .. }
            if [
                paint.border_radii.top_left,
                paint.border_radii.top_right,
                paint.border_radii.bottom_right,
                paint.border_radii.bottom_left,
            ]
            .into_iter()
            .any(|radius| !radius.is_circular()) =>
        {
            Some(RenderCapability::EllipticalBorderRadius)
        }
        Operation::SetText { content, .. } if content.paint.uses_extended_features() => {
            Some(RenderCapability::TextEffects)
        }
        Operation::SetTextStyle { style, .. } if style.paint.uses_extended_features() => {
            Some(RenderCapability::TextEffects)
        }
        Operation::SetImage { .. } => Some(RenderCapability::ImageContent),
        Operation::SetCursor { .. } => Some(RenderCapability::Cursor),
        _ => None,
    };
    let second = match operation {
        Operation::SetText { content, .. } if content.payload.style.uses_extended_typography() => {
            Some(RenderCapability::TextTypography)
        }
        Operation::SetTextStyle { style, .. } if style.style.uses_extended_typography() => {
            Some(RenderCapability::TextTypography)
        }
        _ => None,
    };
    [first, second, None, None, None, None]
}

fn visual_effect_capabilities(effects: &crate::VisualEffects) -> [Option<RenderCapability>; 6] {
    let backdrop = effects.backdrop_blur.is_some_and(|radius| radius > 0.0);
    let mut remainder = effects.clone();
    remainder.backdrop_blur = None;
    [
        (remainder != crate::VisualEffects::default()).then_some(RenderCapability::VisualEffects),
        backdrop.then_some(RenderCapability::BackdropBlur),
        None,
        None,
        None,
        None,
    ]
}

fn background_capabilities(layers: &[crate::BackgroundLayer]) -> [Option<RenderCapability>; 6] {
    if layers.is_empty() {
        return [None; 6];
    }
    let mut linear = false;
    let mut radial = false;
    let mut conic = false;
    let mut resource = false;
    let mut geometry = false;
    for layer in layers {
        match background_image_capability(layer) {
            Some(RenderCapability::LinearGradients) => linear = true,
            Some(RenderCapability::RadialGradients) => radial = true,
            Some(RenderCapability::ConicGradients) => conic = true,
            Some(RenderCapability::BackgroundImageResources) => resource = true,
            _ => {
                return [
                    Some(RenderCapability::BackgroundLayers),
                    None,
                    None,
                    None,
                    None,
                    None,
                ];
            }
        }
        if !has_initial_background_geometry(layer) {
            if !has_explicit_background_geometry(layer) {
                return [
                    Some(RenderCapability::BackgroundLayers),
                    None,
                    None,
                    None,
                    None,
                    None,
                ];
            }
            geometry = true;
        }
    }
    [
        linear.then_some(RenderCapability::LinearGradients),
        radial.then_some(RenderCapability::RadialGradients),
        conic.then_some(RenderCapability::ConicGradients),
        resource.then_some(RenderCapability::BackgroundImageResources),
        geometry.then_some(RenderCapability::BackgroundGeometry),
        (layers.len() > 1).then_some(RenderCapability::BackgroundLayerStacking),
    ]
}

fn background_image_capability(layer: &crate::BackgroundLayer) -> Option<RenderCapability> {
    if matches!(layer.image, crate::PaintImage::Resource(_)) {
        return Some(RenderCapability::BackgroundImageResources);
    }
    if matches!(
        &layer.image,
        crate::PaintImage::LinearGradient {
            repeating: false,
            stops,
            ..
        } if stops.iter().all(|stop| stop.position.is_some())
    ) {
        return Some(RenderCapability::LinearGradients);
    }
    if matches!(
        &layer.image,
        crate::PaintImage::RadialGradient {
            shape: crate::RadialGradientShape::Ellipse,
            extent: crate::RadialGradientExtent::Explicit,
            radii: Some(_),
            repeating: false,
            stops,
            ..
        } if stops.iter().all(|stop| {
            stop.position.is_some_and(|position| position.length == 0.0)
        })
    ) {
        return Some(RenderCapability::RadialGradients);
    }
    if matches!(
        &layer.image,
        crate::PaintImage::ConicGradient {
            repeating: false,
            stops,
            ..
        } if stops.iter().all(|stop| {
            stop.position.is_some_and(|position| position.length == 0.0)
        })
    ) {
        return Some(RenderCapability::ConicGradients);
    }
    None
}

fn has_initial_background_geometry(layer: &crate::BackgroundLayer) -> bool {
    layer.position == Default::default()
        && layer.size == crate::BackgroundSize::Auto
        && layer.repeat_x == crate::ImageRepeat::Repeat
        && layer.repeat_y == crate::ImageRepeat::Repeat
        && has_initial_background_environment(layer)
}

fn has_explicit_background_geometry(layer: &crate::BackgroundLayer) -> bool {
    let resource_backed = matches!(layer.image, crate::PaintImage::Resource(_));
    let supported_size = match layer.size {
        crate::BackgroundSize::Auto
        | crate::BackgroundSize::Cover
        | crate::BackgroundSize::Contain => resource_backed,
        crate::BackgroundSize::Explicit { width, height } => match (width, height) {
            (Some(width), Some(height)) => width.is_valid() && height.is_valid(),
            (Some(value), None) | (None, Some(value)) => resource_backed && value.is_valid(),
            (None, None) => false,
        },
    };
    [
        layer.position.x.length,
        layer.position.x.fraction,
        layer.position.y.length,
        layer.position.y.fraction,
    ]
    .into_iter()
    .all(f32::is_finite)
        && supported_size
        && matches!(
            layer.origin,
            crate::PaintBox::Border | crate::PaintBox::Padding | crate::PaintBox::Content
        )
        && matches!(
            layer.clip,
            crate::PaintBox::Border
                | crate::PaintBox::Padding
                | crate::PaintBox::Content
                | crate::PaintBox::BorderArea
        )
        && layer.attachment == crate::BackgroundAttachment::Scroll
        && layer.blend_mode == crate::BlendMode::Normal
}

fn has_initial_background_environment(layer: &crate::BackgroundLayer) -> bool {
    layer.origin == crate::PaintBox::Padding
        && layer.clip == crate::PaintBox::Border
        && layer.attachment == crate::BackgroundAttachment::Scroll
        && layer.blend_mode == crate::BlendMode::Normal
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, BoxPaint, Cursor,
        ElementTypeId, FontFeature, FontOpticalSizing, FontTag, FrameHeader, FrameMode,
        GradientStop, ImageContent, ImageRepeat, MeasureTextDirection, MeasureTextOverflow,
        MeasureTextWrap, NodeId, ObjectFit, PaintBox, PaintColor, PaintCoordinate,
        PaintCornerRadius, PaintImage, PaintLengthPercentage, PaintPosition, ResourceId, SurfaceId,
        TextContent, TextDecorationLines, TextMeasurePayload, TextMeasureStyle, TextPaint,
        TextStyleSnapshot, VisualEffects,
    };

    fn packet(operations: Vec<Operation>) -> FramePacket {
        FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: SurfaceId::new(1).unwrap(),
                scene_epoch: 1,
                frame_id: 1,
                base_revision: 0,
                target_revision: 1,
                viewport_epoch: 1,
                mode: FrameMode::Snapshot,
            },
            operations,
        }
    }

    fn background_layer(image: PaintImage) -> BackgroundLayer {
        BackgroundLayer {
            image,
            position: PaintPosition::default(),
            size: BackgroundSize::Auto,
            repeat_x: ImageRepeat::Repeat,
            repeat_y: ImageRepeat::Repeat,
            origin: PaintBox::Padding,
            clip: PaintBox::Border,
            attachment: BackgroundAttachment::Scroll,
            blend_mode: BlendMode::Normal,
        }
    }

    fn basic_gradient_stops() -> Vec<GradientStop> {
        vec![
            GradientStop {
                color: PaintColor::Named("black".into()),
                position: Some(PaintCoordinate::default()),
            },
            GradientStop {
                color: PaintColor::Named("white".into()),
                position: Some(PaintCoordinate {
                    length: 0.0,
                    fraction: 1.0,
                }),
            },
        ]
    }

    fn basic_linear_layer() -> BackgroundLayer {
        background_layer(PaintImage::LinearGradient {
            angle_degrees: 90.0,
            repeating: false,
            stops: basic_gradient_stops(),
        })
    }

    fn basic_radial_layer() -> BackgroundLayer {
        background_layer(PaintImage::RadialGradient {
            shape: crate::RadialGradientShape::Ellipse,
            extent: crate::RadialGradientExtent::Explicit,
            center: PaintPosition::default(),
            radii: Some((
                PaintLengthPercentage {
                    length: 10.0,
                    fraction: 0.0,
                },
                PaintLengthPercentage {
                    length: 20.0,
                    fraction: 0.0,
                },
            )),
            repeating: false,
            stops: basic_gradient_stops(),
        })
    }

    fn conic_layer(repeating: bool, first_stop_length: f32) -> BackgroundLayer {
        let mut stops = basic_gradient_stops();
        stops[0].position.as_mut().unwrap().length = first_stop_length;
        background_layer(PaintImage::ConicGradient {
            from_degrees: 90.0,
            center: PaintPosition::default(),
            repeating,
            stops,
        })
    }

    fn basic_conic_layer() -> BackgroundLayer {
        conic_layer(false, 0.0)
    }

    fn explicit_no_repeat(mut layer: BackgroundLayer) -> BackgroundLayer {
        layer.size = BackgroundSize::Explicit {
            width: Some(PaintLengthPercentage {
                length: 45.0,
                fraction: 0.0,
            }),
            height: Some(PaintLengthPercentage {
                length: 45.0,
                fraction: 0.0,
            }),
        };
        layer.repeat_x = ImageRepeat::NoRepeat;
        layer.repeat_y = ImageRepeat::NoRepeat;
        layer
    }

    #[test]
    fn discovers_elliptical_radius_without_classifying_base_box_paint() {
        let node = NodeId::new(1).unwrap();
        let mut paint = BoxPaint::default();
        paint.border_radii.top_left = PaintCornerRadius {
            horizontal: PaintLengthPercentage {
                length: 4.0,
                fraction: 0.0,
            },
            vertical: PaintLengthPercentage {
                length: 8.0,
                fraction: 0.0,
            },
        };
        let packet = packet(vec![
            Operation::CreateNode {
                node,
                element_type: ElementTypeId::new(1).unwrap(),
            },
            Operation::SetBoxPaint { node, paint },
        ]);
        assert_eq!(
            packet.required_capabilities(),
            vec![RenderCapability::EllipticalBorderRadius]
        );
    }

    #[test]
    fn omitted_capabilities_are_rejected_and_duplicates_are_invalid() {
        let profile = RenderCapabilities::base();
        assert!(!profile.supports(RenderCapability::ImageContent));
        let duplicate = CapabilityEntry {
            capability: RenderCapability::Cursor,
            support: CapabilitySupport::Native,
        };
        assert_eq!(
            RenderCapabilities::new(ProtocolVersion::CURRENT, vec![duplicate, duplicate]),
            Err(DuplicateCapability(RenderCapability::Cursor))
        );
    }

    #[test]
    fn negotiation_selects_the_highest_shared_minor_and_preserves_host_support() {
        let host = RenderCapabilities::new(
            ProtocolVersion { major: 1, minor: 3 },
            [CapabilityEntry {
                capability: RenderCapability::Cursor,
                support: CapabilitySupport::Emulated,
            }],
        )
        .unwrap();

        let negotiated = host
            .negotiate(ProtocolVersion { major: 1, minor: 4 })
            .expect("matching protocol majors negotiate");

        assert_eq!(
            negotiated.protocol(),
            ProtocolVersion { major: 1, minor: 3 }
        );
        assert_eq!(
            negotiated.support(RenderCapability::Cursor),
            CapabilitySupport::Emulated
        );

        let newer_host = RenderCapabilities::base();
        assert_eq!(
            newer_host
                .negotiate(ProtocolVersion { major: 1, minor: 2 })
                .unwrap()
                .protocol(),
            ProtocolVersion { major: 1, minor: 2 }
        );
    }

    #[test]
    fn negotiation_rejects_a_different_protocol_major() {
        let host = RenderCapabilities::base();

        let error = CapabilityNegotiationError::IncompatibleProtocol {
            runtime: ProtocolVersion { major: 2, minor: 0 },
            host: ProtocolVersion::CURRENT,
        };
        assert_eq!(
            host.negotiate(ProtocolVersion { major: 2, minor: 0 }),
            Err(error)
        );
        assert_eq!(
            error.to_string(),
            format!(
                "incompatible frame protocol: Rust 2.0, Host {}.{}",
                ProtocolVersion::CURRENT.major,
                ProtocolVersion::CURRENT.minor
            )
        );
    }

    #[test]
    fn wire_masks_round_trip_and_reject_ambiguous_profiles() {
        let native = RenderCapability::Cursor.mask();
        let emulated = RenderCapability::BackdropBlur.mask();
        let profile = RenderCapabilities::from_masks(ProtocolVersion::CURRENT, native, emulated)
            .expect("known disjoint masks are valid");

        assert_eq!(profile.native_mask(), native);
        assert_eq!(profile.emulated_mask(), emulated);
        assert_eq!(
            profile.support(RenderCapability::BackdropBlur),
            CapabilitySupport::Emulated
        );
        let overlapping = InvalidCapabilityMasks::Overlapping { bits: native };
        assert_eq!(
            RenderCapabilities::from_masks(ProtocolVersion::CURRENT, native, native),
            Err(overlapping)
        );
        assert_eq!(
            overlapping.to_string(),
            format!("Host capabilities are both native and emulated in bits 0x{native:016x}")
        );
        let unknown = InvalidCapabilityMasks::Unknown { bits: 1_u64 << 63 };
        assert_eq!(
            RenderCapabilities::from_masks(ProtocolVersion::CURRENT, 1_u64 << 63, 0),
            Err(unknown)
        );
        assert_eq!(
            unknown.to_string(),
            "unknown Host capability bits 0x8000000000000000"
        );
    }

    #[test]
    fn backdrop_blur_is_negotiated_independently_from_other_visual_effects() {
        let node = NodeId::new(1).unwrap();
        let effects = VisualEffects {
            backdrop_blur: Some(8.0),
            ..VisualEffects::default()
        };
        let packet = packet(vec![Operation::SetVisualEffects { node, effects }]);

        assert_eq!(
            packet.required_capabilities(),
            vec![RenderCapability::BackdropBlur]
        );
        assert_eq!(
            RenderCapabilities::new(
                ProtocolVersion::CURRENT,
                [CapabilityEntry {
                    capability: RenderCapability::VisualEffects,
                    support: CapabilitySupport::Native,
                }]
            )
            .unwrap()
            .first_unsupported(&packet),
            Some(RenderCapability::BackdropBlur)
        );
    }

    #[test]
    fn linear_gradient_capability_does_not_claim_the_complete_layer_protocol() {
        let node = NodeId::new(1).unwrap();
        assert_eq!(
            packet(vec![
                Operation::SetBackgroundLayers {
                    node,
                    layers: vec![basic_linear_layer()],
                },
                Operation::SetBackgroundLayers {
                    node,
                    layers: vec![basic_linear_layer()],
                },
            ])
            .required_capabilities(),
            vec![RenderCapability::LinearGradients]
        );
        assert_eq!(
            packet(vec![Operation::SetBackgroundLayers {
                node,
                layers: vec![basic_linear_layer(), basic_linear_layer()],
            }])
            .required_capabilities(),
            vec![
                RenderCapability::LinearGradients,
                RenderCapability::BackgroundLayerStacking,
            ]
        );
        assert_eq!(
            packet(vec![Operation::SetBackgroundLayers {
                node,
                layers: vec![basic_radial_layer()],
            }])
            .required_capabilities(),
            vec![RenderCapability::RadialGradients]
        );
        assert!(
            packet(vec![Operation::SetBackgroundLayers {
                node,
                layers: Vec::new(),
            }])
            .required_capabilities()
            .is_empty()
        );
    }

    #[test]
    fn conic_gradient_capability_is_limited_to_the_resolved_host_subset() {
        let node = NodeId::new(1).unwrap();
        assert_eq!(
            packet(vec![Operation::SetBackgroundLayers {
                node,
                layers: vec![basic_conic_layer()],
            }])
            .required_capabilities(),
            vec![RenderCapability::ConicGradients]
        );

        assert_eq!(
            packet(vec![Operation::SetBackgroundLayers {
                node,
                layers: vec![conic_layer(true, 0.0)],
            }])
            .required_capabilities(),
            vec![RenderCapability::BackgroundLayers]
        );

        assert_eq!(
            packet(vec![Operation::SetBackgroundLayers {
                node,
                layers: vec![conic_layer(false, 1.0)],
            }])
            .required_capabilities(),
            vec![RenderCapability::BackgroundLayers]
        );

        assert_eq!(
            packet(vec![
                Operation::SetBackgroundLayers {
                    node,
                    layers: vec![basic_conic_layer()],
                },
                Operation::SetBackgroundLayers {
                    node,
                    layers: vec![basic_conic_layer()],
                },
            ])
            .required_capabilities(),
            vec![RenderCapability::ConicGradients]
        );
    }

    #[test]
    fn background_geometry_is_additive_to_the_supported_image_capability() {
        let node = NodeId::new(1).unwrap();
        let operation = |layer| Operation::SetBackgroundLayers {
            node,
            layers: vec![layer],
        };
        assert_eq!(
            packet(vec![operation(explicit_no_repeat(basic_linear_layer()))])
                .required_capabilities(),
            vec![
                RenderCapability::LinearGradients,
                RenderCapability::BackgroundGeometry,
            ]
        );
        assert_eq!(
            packet(vec![operation(explicit_no_repeat(basic_radial_layer()))])
                .required_capabilities(),
            vec![
                RenderCapability::RadialGradients,
                RenderCapability::BackgroundGeometry,
            ]
        );
        assert_eq!(
            packet(vec![operation(explicit_no_repeat(basic_conic_layer()))])
                .required_capabilities(),
            vec![
                RenderCapability::ConicGradients,
                RenderCapability::BackgroundGeometry,
            ]
        );

        for (repeat_x, repeat_y) in [
            (ImageRepeat::Repeat, ImageRepeat::NoRepeat),
            (ImageRepeat::NoRepeat, ImageRepeat::Repeat),
            (ImageRepeat::Repeat, ImageRepeat::Repeat),
            (ImageRepeat::Space, ImageRepeat::NoRepeat),
            (ImageRepeat::NoRepeat, ImageRepeat::Space),
            (ImageRepeat::Space, ImageRepeat::Space),
            (ImageRepeat::Round, ImageRepeat::NoRepeat),
            (ImageRepeat::NoRepeat, ImageRepeat::Round),
            (ImageRepeat::Round, ImageRepeat::Round),
        ] {
            let mut layer = explicit_no_repeat(basic_linear_layer());
            layer.repeat_x = repeat_x;
            layer.repeat_y = repeat_y;
            assert_eq!(
                packet(vec![operation(layer)]).required_capabilities(),
                vec![
                    RenderCapability::LinearGradients,
                    RenderCapability::BackgroundGeometry,
                ]
            );
        }

        let mut positioned = explicit_no_repeat(basic_linear_layer());
        positioned.position = PaintPosition {
            x: PaintCoordinate {
                length: 50.0,
                fraction: 0.0,
            },
            y: PaintCoordinate {
                length: 0.0,
                fraction: 0.5,
            },
        };
        assert_eq!(
            packet(vec![operation(positioned)]).required_capabilities(),
            vec![
                RenderCapability::LinearGradients,
                RenderCapability::BackgroundGeometry,
            ]
        );

        let mut non_finite_position = explicit_no_repeat(basic_linear_layer());
        non_finite_position.position.x.fraction = f32::NAN;
        assert_eq!(
            packet(vec![operation(non_finite_position)]).required_capabilities(),
            vec![RenderCapability::BackgroundLayers]
        );

        for (origin, clip) in [
            (PaintBox::Border, PaintBox::Border),
            (PaintBox::Border, PaintBox::Padding),
            (PaintBox::Padding, PaintBox::Padding),
            (PaintBox::Content, PaintBox::Border),
            (PaintBox::Border, PaintBox::Content),
            (PaintBox::Border, PaintBox::BorderArea),
            (PaintBox::Content, PaintBox::Content),
        ] {
            let mut layer = explicit_no_repeat(basic_linear_layer());
            layer.origin = origin;
            layer.clip = clip;
            assert_eq!(
                packet(vec![operation(layer)]).required_capabilities(),
                vec![
                    RenderCapability::LinearGradients,
                    RenderCapability::BackgroundGeometry,
                ]
            );
        }

        let mut unsupported_box = explicit_no_repeat(basic_linear_layer());
        unsupported_box.origin = PaintBox::Margin;
        assert_eq!(
            packet(vec![operation(unsupported_box)]).required_capabilities(),
            vec![RenderCapability::BackgroundLayers]
        );
        let mut unsupported_clip = explicit_no_repeat(basic_linear_layer());
        unsupported_clip.clip = PaintBox::Text;
        assert_eq!(
            packet(vec![operation(unsupported_clip)]).required_capabilities(),
            vec![RenderCapability::BackgroundLayers]
        );

        let mut incomplete_size = explicit_no_repeat(basic_linear_layer());
        incomplete_size.size = BackgroundSize::Explicit {
            width: Some(PaintLengthPercentage::default()),
            height: None,
        };
        assert_eq!(
            packet(vec![operation(incomplete_size)]).required_capabilities(),
            vec![RenderCapability::BackgroundLayers]
        );

        for size in [
            BackgroundSize::Auto,
            BackgroundSize::Cover,
            BackgroundSize::Contain,
            BackgroundSize::Explicit {
                width: Some(PaintLengthPercentage::default()),
                height: None,
            },
            BackgroundSize::Explicit {
                width: None,
                height: Some(PaintLengthPercentage::default()),
            },
        ] {
            let mut layer = explicit_no_repeat(background_layer(PaintImage::Resource(
                ResourceId::new(1).unwrap(),
            )));
            layer.size = size;
            assert_eq!(
                packet(vec![operation(layer)]).required_capabilities(),
                vec![
                    RenderCapability::BackgroundImageResources,
                    RenderCapability::BackgroundGeometry,
                ]
            );
        }

        let mut missing_resource_size = explicit_no_repeat(background_layer(PaintImage::Resource(
            ResourceId::new(1).unwrap(),
        )));
        missing_resource_size.size = BackgroundSize::Explicit {
            width: None,
            height: None,
        };
        assert_eq!(
            packet(vec![operation(missing_resource_size)]).required_capabilities(),
            vec![RenderCapability::BackgroundLayers]
        );

        let mut negative_size = explicit_no_repeat(basic_linear_layer());
        negative_size.size = BackgroundSize::Explicit {
            width: Some(PaintLengthPercentage {
                length: -1.0,
                fraction: 0.0,
            }),
            height: Some(PaintLengthPercentage::default()),
        };
        assert_eq!(
            packet(vec![operation(negative_size)]).required_capabilities(),
            vec![RenderCapability::BackgroundLayers]
        );

        let image_only = RenderCapabilities::new(
            ProtocolVersion::CURRENT,
            vec![CapabilityEntry {
                capability: RenderCapability::LinearGradients,
                support: CapabilitySupport::Native,
            }],
        )
        .unwrap();
        let packet = packet(vec![operation(explicit_no_repeat(basic_linear_layer()))]);
        assert_eq!(
            image_only.first_unsupported(&packet),
            Some(RenderCapability::BackgroundGeometry)
        );
    }

    #[test]
    fn profiles_and_packets_cover_every_optional_capability() {
        assert_eq!(
            RenderCapability::ALL.map(RenderCapability::as_str),
            [
                "elliptical-border-radius",
                "background-layers",
                "visual-effects",
                "text-effects",
                "text-typography",
                "image-content",
                "cursor",
                "resource-lifecycle",
                "linear-gradients",
                "radial-gradients",
                "conic-gradients",
                "background-geometry",
                "background-layer-stacking",
                "background-image-resources",
                "backdrop-blur",
            ]
        );

        let profile = RenderCapabilities::new(
            ProtocolVersion::CURRENT,
            vec![
                CapabilityEntry {
                    capability: RenderCapability::BackgroundLayers,
                    support: CapabilitySupport::Native,
                },
                CapabilityEntry {
                    capability: RenderCapability::VisualEffects,
                    support: CapabilitySupport::Emulated,
                },
                CapabilityEntry {
                    capability: RenderCapability::Cursor,
                    support: CapabilitySupport::Unsupported,
                },
            ],
        )
        .unwrap();
        assert_eq!(profile.protocol(), ProtocolVersion::CURRENT);
        assert!(profile.supports_protocol(ProtocolVersion { major: 1, minor: 0 }));
        assert!(!profile.supports_protocol(ProtocolVersion {
            major: 1,
            minor: crate::PROTOCOL_MINOR + 1,
        }));
        assert!(!profile.supports_protocol(ProtocolVersion { major: 2, minor: 0 }));
        assert_eq!(
            profile.support(RenderCapability::BackgroundLayers),
            CapabilitySupport::Native
        );
        assert_eq!(
            profile.support(RenderCapability::VisualEffects),
            CapabilitySupport::Emulated
        );
        assert_eq!(
            profile.support(RenderCapability::Cursor),
            CapabilitySupport::Unsupported
        );

        let node = NodeId::new(1).unwrap();
        let mut paint = BoxPaint::default();
        paint.border_radii.top_left = PaintCornerRadius {
            horizontal: PaintLengthPercentage {
                length: 1.0,
                fraction: 0.0,
            },
            vertical: PaintLengthPercentage {
                length: 2.0,
                fraction: 0.0,
            },
        };
        let mut style = TextMeasureStyle::default();
        style.features.push(FontFeature {
            tag: FontTag::new(*b"kern").unwrap(),
            value: 1,
        });
        style.optical_sizing = FontOpticalSizing::None;
        let mut text_paint = TextPaint::default();
        text_paint.decoration.lines = TextDecorationLines {
            underline: true,
            overline: false,
            line_through: false,
        };
        let text_style = TextStyleSnapshot {
            style: style.clone(),
            locale: None,
            direction: MeasureTextDirection::Auto,
            alignment: crate::MeasureTextAlignment::Start,
            paint: text_paint.clone(),
        };
        let operations = vec![
            Operation::SetBoxPaint { node, paint },
            Operation::SetBackgroundLayers {
                node,
                layers: vec![basic_linear_layer()],
            },
            Operation::SetBackgroundLayers {
                node,
                layers: vec![basic_linear_layer()],
            },
            Operation::SetBackgroundLayers {
                node,
                layers: vec![basic_conic_layer()],
            },
            Operation::SetBackgroundLayers {
                node,
                layers: vec![explicit_no_repeat(basic_linear_layer())],
            },
            Operation::SetVisualEffects {
                node,
                effects: VisualEffects {
                    backdrop_blur: Some(4.0),
                    image_rendering: crate::ImageRendering::Pixelated,
                    ..VisualEffects::default()
                },
            },
            Operation::SetText {
                node,
                content: TextContent {
                    payload: TextMeasurePayload {
                        text: "capabilities".into(),
                        style,
                        locale: None,
                        direction: MeasureTextDirection::Auto,
                        alignment: crate::MeasureTextAlignment::Start,
                        indent: Default::default(),
                        wrap: MeasureTextWrap::Wrap,
                        word_break: Default::default(),
                        max_lines: None,
                        overflow: MeasureTextOverflow::Clip,
                    },
                    paint: text_paint,
                    prepared_content: None,
                },
            },
            Operation::SetTextStyle {
                node,
                style: text_style,
            },
            Operation::SetImage {
                node,
                content: ImageContent {
                    resource: ResourceId::new(1).unwrap(),
                    fit: ObjectFit::Contain,
                    position: PaintPosition::default(),
                },
            },
            Operation::SetCursor {
                node,
                cursor: Cursor::default(),
            },
            Operation::SetBackgroundLayers {
                node,
                layers: vec![background_layer(PaintImage::Resource(
                    ResourceId::new(2).unwrap(),
                ))],
            },
        ];
        let packet = packet(operations);
        assert_eq!(
            packet.required_capabilities(),
            vec![
                RenderCapability::EllipticalBorderRadius,
                RenderCapability::LinearGradients,
                RenderCapability::ConicGradients,
                RenderCapability::BackgroundGeometry,
                RenderCapability::VisualEffects,
                RenderCapability::BackdropBlur,
                RenderCapability::TextEffects,
                RenderCapability::TextTypography,
                RenderCapability::ImageContent,
                RenderCapability::Cursor,
                RenderCapability::BackgroundImageResources,
            ]
        );
        assert_eq!(
            RenderCapabilities::base().first_unsupported(&packet),
            Some(RenderCapability::EllipticalBorderRadius)
        );
        let elliptical_only = RenderCapabilities::new(
            ProtocolVersion::CURRENT,
            vec![CapabilityEntry {
                capability: RenderCapability::EllipticalBorderRadius,
                support: CapabilitySupport::Native,
            }],
        )
        .unwrap();
        assert_eq!(
            elliptical_only.first_unsupported(&packet),
            Some(RenderCapability::LinearGradients)
        );
        assert_eq!(
            RenderCapabilities::all_frame_native().first_unsupported(&packet),
            None
        );
        assert!(
            !RenderCapabilities::all_frame_native().supports(RenderCapability::ResourceLifecycle)
        );
    }
}
