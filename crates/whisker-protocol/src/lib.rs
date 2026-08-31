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

mod accessibility;
mod capability;
mod element;
mod frame;
mod id;
mod input;
mod measurement;
mod resource;
mod validation;
mod visual;

pub use accessibility::{
    Accessibility, AccessibilityChecked, AccessibilityRole, AccessibilityState,
};
pub use capability::{
    CapabilityEntry, CapabilityNegotiationError, CapabilitySupport, DuplicateCapability,
    InvalidCapabilityMasks, RenderCapabilities, RenderCapability,
};
pub use element::{
    ChildPolicy, ElementCommandSchema, ElementEventSchema, ElementMeasurement,
    ElementPropertySchema, ElementRegistration, ElementRegistrationError, ElementSchema,
    ElementValueKind,
};
pub use frame::{
    BorderLineStyle, BoxClip, BoxPaint, FrameHeader, FrameMode, FramePacket, HitTestBehavior,
    LayoutGeometry, LayoutRect, Operation, OverflowClip, PROTOCOL_MAJOR, PROTOCOL_MINOR,
    PaintColor, PaintCornerRadius, PaintCorners, PaintEdges, PaintLengthPercentage,
    ProtocolVersion, TextContent, TextContentError, TextPaint, TextStyleSnapshot, Transform,
    Visibility,
};
pub use id::{
    CommandId, ElementTypeId, EventId, MeasurementKey, MeasurementRequestId, NodeId, PointerId,
    PreparedContentId, PropertyId, ResourceId, SurfaceId,
};
pub use input::{
    InputEvent, InputEventError, InputEventKind, InputPoint, PointerInput, PointerKind,
};
pub use measurement::{
    AvailableSpace, CustomMeasurePayload, EmbeddedSurfaceMeasurePayload, FontFeature,
    FontOpticalSizing, FontTag, FontVariation, MeasureConstraints, MeasureFontFamily,
    MeasureFontStyle, MeasureLineHeight, MeasureTextAlignment, MeasureTextDirection,
    MeasureTextIndent, MeasureTextOverflow, MeasureTextWordBreak, MeasureTextWrap, MeasuredSize,
    MeasurementBatchError, MeasurementKind, MeasurementMetrics, MeasurementPayload,
    MeasurementPayloadError, MeasurementReady, MeasurementRequest, MeasurementResponse,
    MeasurementSpec, ModuleMeasureRequest, NativeControlMeasurePayload, PendingMeasurePolicy,
    ReplacedContentMeasurePayload, TextMeasurePayload, TextMeasureStyle,
    UnsupportedMeasurementReason, validate_measurement_batch,
};
pub use resource::{
    ResourceCommand, ResourceDimensions, ResourceEvent, ResourceFailureCode, ResourceKind,
    ResourceMessageError, ResourceRequest, ResourceSource,
};
pub use validation::{ApplyResult, NodeProjection, SceneProjection, ValidationError};
pub use visual::{
    BackfaceVisibility, BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode,
    BoxShadow, ClipShape, Cursor, CursorKeyword, CursorResource, FillRule, GradientStop,
    ImageRendering, ImageRepeat, Isolation, MaskComposite, MaskLayer, MaskMode, OutlineLineStyle,
    OutlinePaint, PaintBox, PaintCoordinate, PaintImage, PaintPosition, PathCommand,
    RadialGradientExtent, RadialGradientShape, TextDecoration, TextDecorationLines,
    TextDecorationStyle, TextDecorationThickness, TextShadow, TransformStyle, VisualEffects,
};
pub use whisker_value::WhiskerValue;
