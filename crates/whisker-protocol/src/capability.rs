//! Renderer capability negotiation for optional semantic protocol groups.

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
}

impl RenderCapability {
    /// Every optional capability in stable declaration order.
    pub const ALL: [Self; 8] = [
        Self::EllipticalBorderRadius,
        Self::BackgroundLayers,
        Self::VisualEffects,
        Self::TextEffects,
        Self::TextTypography,
        Self::ImageContent,
        Self::Cursor,
        Self::ResourceLifecycle,
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
        }
    }

    const fn bit(self) -> u16 {
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

/// Protocol version and optional semantic features supported by one Host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderCapabilities {
    protocol: ProtocolVersion,
    native: u16,
    emulated: u16,
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
            let bit = entry.capability.bit();
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
        Self {
            protocol: ProtocolVersion::CURRENT,
            native: RenderCapability::ResourceLifecycle.bit() - 1,
            emulated: 0,
        }
    }

    /// Returns the highest protocol version understood by the Host.
    pub const fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    /// Returns whether the Host understands this protocol version.
    pub const fn supports_protocol(&self, version: ProtocolVersion) -> bool {
        version.major == self.protocol.major && version.minor <= self.protocol.minor
    }

    /// Returns how the Host implements a feature; omitted features are unsupported.
    pub fn support(&self, capability: RenderCapability) -> CapabilitySupport {
        let bit = capability.bit();
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

fn operation_capabilities(operation: &Operation) -> [Option<RenderCapability>; 2] {
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
        Operation::SetBackgroundLayers { .. } => Some(RenderCapability::BackgroundLayers),
        Operation::SetVisualEffects { .. } => Some(RenderCapability::VisualEffects),
        Operation::SetText { content, .. } if content.paint.uses_extended_features() => {
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
        _ => None,
    };
    [first, second]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BoxPaint, ElementTypeId, FrameHeader, FrameMode, NodeId, PaintCornerRadius,
        PaintLengthPercentage, SurfaceId,
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
            RenderCapabilities::new(ProtocolVersion::CURRENT, [duplicate, duplicate]),
            Err(DuplicateCapability(RenderCapability::Cursor))
        );
    }
}
