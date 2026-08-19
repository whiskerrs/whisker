//! Host-independent frame protocol model for Whisker
//!
//! This crate defines the owned, semantic representation of a scene frame and
//! validates its revision and tree invariants. It deliberately does not define
//! the packed wire encoding or a platform renderer ABI yet: those layers can
//! evolve without coupling the Rust scene engine to Android, UIKit, or DOM
//! types.
//!
//! [`SceneProjection`] is a small reference receiver used by tests, recording
//! renderers, and future Host conformance fixtures. Applying a [`FramePacket`]
//! is transactional: malformed operations leave the accepted projection and
//! revision unchanged.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod frame;
mod id;
mod measurement;
mod validation;

pub use frame::{
    FrameHeader, FrameMode, FramePacket, HitTestBehavior, LayoutRect, Operation, PROTOCOL_MAJOR,
    PROTOCOL_MINOR, ProtocolValue, ProtocolVersion, Transform, Visibility,
};
pub use id::{
    CommandId, ElementTypeId, MeasurementKey, MeasurementRequestId, NodeId, PointerId,
    PreparedContentId, PropertyId, ResultId, SurfaceId,
};
pub use measurement::{
    AvailableSpace, MeasureConstraints, MeasuredSize, MeasurementKind, MeasurementMetrics,
    MeasurementReady, MeasurementRequest, MeasurementResponse, MeasurementSpec,
    PendingMeasurePolicy, UnsupportedMeasurementReason,
};
pub use validation::{ApplyResult, NodeProjection, SceneProjection, ValidationError};
