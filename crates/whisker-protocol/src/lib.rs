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
mod input;
mod measurement;
mod validation;

pub use frame::{
    BorderLineStyle, BoxClip, BoxPaint, FrameHeader, FrameMode, FramePacket, HitTestBehavior,
    LayoutRect, Operation, OverflowClip, PROTOCOL_MAJOR, PROTOCOL_MINOR, PaintColor, PaintCorners,
    PaintEdges, PaintLengthPercentage, ProtocolValue, ProtocolVersion, TextContent,
    TextContentError, TextPaint, Transform, Visibility,
};
pub use id::{
    CommandId, ElementTypeId, MeasurementKey, MeasurementRequestId, NodeId, PointerId,
    PreparedContentId, PropertyId, ResultId, SurfaceId,
};
pub use input::{
    InputEvent, InputEventError, InputEventKind, InputPoint, PointerInput, PointerKind,
};
pub use measurement::{
    AvailableSpace, CustomMeasurePayload, EmbeddedSurfaceMeasurePayload, MeasureConstraints,
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection,
    MeasureTextOverflow, MeasureTextWrap, MeasuredSize, MeasurementBatchError, MeasurementKind,
    MeasurementMetrics, MeasurementPayload, MeasurementPayloadError, MeasurementReady,
    MeasurementRequest, MeasurementResponse, MeasurementSpec, NativeControlMeasurePayload,
    PendingMeasurePolicy, ReplacedContentMeasurePayload, TextMeasurePayload, TextMeasureStyle,
    UnsupportedMeasurementReason, validate_measurement_batch,
};
pub use validation::{ApplyResult, NodeProjection, SceneProjection, ValidationError};
