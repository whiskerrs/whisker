//! Rust-only renderer used by engine and protocol tests.

use whisker_protocol::{ApplyResult, FramePacket, SceneProjection, SurfaceId, ValidationError};

/// Semantic frame receiver used by a [`Scene`](crate::Scene).
///
/// Platform renderer modules will implement the same information flow through
/// generated bindings. This small Rust trait exists for engine tests and does
/// not define the final cross-language ABI.
pub trait FrameSink {
    /// Receiver-specific presentation failure.
    type Error;

    /// Validates and presents one complete frame transaction.
    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error>;
}

/// One attempted presentation and its recorded result.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedFrame {
    /// Packet submitted to the renderer.
    pub packet: FramePacket,
    /// Reference validation outcome.
    pub result: Result<ApplyResult, ValidationError>,
}

/// Rust-only frame receiver backed by [`SceneProjection`].
///
/// Every attempted packet is retained, including malformed packets. Tests can
/// therefore assert both the producer output and the receiver state after
/// acceptance, recovery requests, or validation failures.
#[derive(Debug)]
pub struct RecordingRenderer {
    projection: SceneProjection,
    frames: Vec<RecordedFrame>,
}

impl RecordingRenderer {
    /// Creates an empty receiver for one surface.
    pub fn new(surface: SurfaceId) -> Self {
        Self {
            projection: SceneProjection::new(surface),
            frames: Vec::new(),
        }
    }

    /// Returns the accepted reference projection.
    pub const fn projection(&self) -> &SceneProjection {
        &self.projection
    }

    /// Returns every attempted frame in submission order.
    pub fn frames(&self) -> &[RecordedFrame] {
        &self.frames
    }
}

impl FrameSink for RecordingRenderer {
    type Error = ValidationError;

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        let result = self.projection.apply(packet);
        self.frames.push(RecordedFrame {
            packet: packet.clone(),
            result: result.clone(),
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_protocol::{FrameHeader, FrameMode, ProtocolVersion, SurfaceId, ValidationError};

    fn surface(value: u64) -> SurfaceId {
        SurfaceId::new(value).expect("test surface")
    }

    #[test]
    fn records_accepted_and_rejected_packets() {
        let mut renderer = RecordingRenderer::new(surface(1));
        let packet = FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: surface(1),
                scene_epoch: 1,
                frame_id: 1,
                base_revision: 0,
                target_revision: 1,
                viewport_epoch: 1,
                mode: FrameMode::Snapshot,
            },
            operations: Vec::new(),
        };
        assert_eq!(
            renderer.present(&packet),
            Ok(ApplyResult::Accepted { revision: 1 })
        );

        let mut malformed = packet;
        malformed.header.scene_epoch = 2;
        malformed.header.surface = surface(2);
        assert_eq!(
            renderer.present(&malformed),
            Err(ValidationError::SurfaceMismatch {
                expected: surface(1),
                received: surface(2),
            })
        );

        assert_eq!(renderer.projection().revision(), 1);
        assert_eq!(renderer.frames().len(), 2);
        assert_eq!(
            renderer.frames()[0].result,
            Ok(ApplyResult::Accepted { revision: 1 })
        );
        assert!(renderer.frames()[1].result.is_err());
    }
}
