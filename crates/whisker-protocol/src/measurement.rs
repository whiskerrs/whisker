//! Semantic intrinsic-measurement values shared by Rust and Host renderers.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ElementTypeId, LayoutRect, MeasurementKey, MeasurementRequestId, NodeId, PreparedContentId,
    SurfaceId,
};

/// Content category whose intrinsic dimensions are supplied by a measurement provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeasurementKind {
    /// Shaped and wrapped text, including inline attachments.
    Text,
    /// Replaced content such as an auto-sized image or media poster.
    ReplacedContent,
    /// A platform control such as a switch, progress indicator, or date picker.
    NativeControl,
    /// A child surface whose content determines its containing box.
    EmbeddedSurface,
    /// An element-specific measurement implemented by a module.
    Custom {
        /// Module-defined payload schema version.
        version: u16,
    },
}

/// Font family selected for text shaping.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MeasureFontFamily {
    /// Platform default UI font.
    System,
    /// A named font family resolved by the Host font collection.
    Named(String),
}

/// Font face posture selected for text shaping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeasureFontStyle {
    /// Upright face.
    Normal,
    /// Italic face.
    Italic,
    /// Synthesized or font-provided oblique face.
    Oblique,
}

/// Four-byte OpenType feature or variation axis tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontTag([u8; 4]);

impl FontTag {
    /// Creates a tag from exactly four printable ASCII bytes.
    pub const fn new(value: [u8; 4]) -> Option<Self> {
        let mut index = 0;
        while index < value.len() {
            if value[index] < 0x20 || value[index] > 0x7e {
                return None;
            }
            index += 1;
        }
        Some(Self(value))
    }

    /// Returns the OpenType tag bytes.
    pub const fn get(self) -> [u8; 4] {
        self.0
    }
}

/// One resolved OpenType feature selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontFeature {
    /// Four-byte feature tag, such as `kern` or `tnum`.
    pub tag: FontTag,
    /// Feature value after CSS `on`/`off` normalization.
    pub value: u32,
}

/// One resolved variable-font axis value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontVariation {
    /// Four-byte axis tag, such as `wght` or `opsz`.
    pub tag: FontTag,
    /// Finite axis value.
    pub value: f32,
}

/// Whether the shaper may select optical sizing from the computed font size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontOpticalSizing {
    /// Enable automatic optical sizing when the selected font provides it.
    #[default]
    Auto,
    /// Disable automatic optical sizing.
    None,
}

/// Computed line-height input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeasureLineHeight {
    /// Use the platform shaper's normal line metric.
    Normal,
    /// Exact line height in logical pixels.
    LogicalPixels(f32),
}

/// Base direction used by shaping and line breaking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeasureTextDirection {
    /// Resolve direction from Unicode content and the Host locale.
    Auto,
    /// Left to right.
    LeftToRight,
    /// Right to left.
    RightToLeft,
}

/// Whether text may create additional lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeasureTextWrap {
    /// Apply platform line breaking within the available width.
    Wrap,
    /// Keep content on one logical line.
    NoWrap,
}

/// Content treatment when text exceeds its line or size limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeasureTextOverflow {
    /// Clip overflowing glyphs.
    Clip,
    /// Shape an ellipsis on the last visible line.
    Ellipsis,
}

/// Resolved text properties that can affect shaping or intrinsic metrics.
#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasureStyle {
    /// Ordered font-family fallback list.
    pub font_families: Vec<MeasureFontFamily>,
    /// Computed font size in logical pixels.
    pub font_size: f32,
    /// CSS-compatible numeric weight in the inclusive range 1 through 1000.
    pub font_weight: u16,
    /// Font posture.
    pub font_style: MeasureFontStyle,
    /// Computed line height.
    pub line_height: MeasureLineHeight,
    /// Additional logical pixels between glyph advances.
    pub letter_spacing: f32,
    /// Resolved OpenType feature settings, sorted by tag.
    pub features: Vec<FontFeature>,
    /// Resolved variable-font axis settings, sorted by tag.
    pub variations: Vec<FontVariation>,
    /// Optical sizing behavior.
    pub optical_sizing: FontOpticalSizing,
}

impl Default for TextMeasureStyle {
    fn default() -> Self {
        Self {
            font_families: vec![MeasureFontFamily::System],
            font_size: 14.0,
            font_weight: 400,
            font_style: MeasureFontStyle::Normal,
            line_height: MeasureLineHeight::Normal,
            letter_spacing: 0.0,
            features: Vec::new(),
            variations: Vec::new(),
            optical_sizing: FontOpticalSizing::Auto,
        }
    }
}

impl TextMeasureStyle {
    /// Returns whether this style needs the protocol-minor-1 typography path.
    pub fn uses_extended_typography(&self) -> bool {
        !self.features.is_empty()
            || !self.variations.is_empty()
            || self.optical_sizing != FontOpticalSizing::Auto
    }
}

/// Complete built-in Text input required by a Host shaper.
#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasurePayload {
    /// UTF-8 text after application-level text transforms.
    pub text: String,
    /// Resolved metric-affecting style.
    pub style: TextMeasureStyle,
    /// BCP-47 locale hint, or `None` to use the surface environment.
    pub locale: Option<String>,
    /// Base shaping direction.
    pub direction: MeasureTextDirection,
    /// Line-wrapping policy.
    pub wrap: MeasureTextWrap,
    /// Maximum visible line count, or `None` for no explicit limit.
    pub max_lines: Option<u32>,
    /// Overflow policy applied at the line limit.
    pub overflow: MeasureTextOverflow,
}

/// Built-in replaced-content metadata used when Rust cannot derive a size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReplacedContentMeasurePayload {
    /// Resource intrinsic dimensions already known to the module, if any.
    pub intrinsic_size: Option<MeasuredSize>,
    /// Width divided by height, if known independently of decoded dimensions.
    pub aspect_ratio: Option<f32>,
}

/// Versioned inputs for a built-in or module-registered native control.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeControlMeasurePayload {
    /// Stable control schema identifier negotiated during element registration.
    pub control_type: u32,
    /// Payload schema version.
    pub version: u16,
    /// Versioned schema-owned bytes affecting intrinsic size.
    pub state: Vec<u8>,
}

/// Inputs for a child surface whose content determines its containing box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmbeddedSurfaceMeasurePayload {
    /// Child surface queried by the Host.
    pub surface: SurfaceId,
    /// Last preferred child size retained by Rust, if one is available.
    pub preferred_size: Option<MeasuredSize>,
}

/// Versioned module-defined measurement inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomMeasurePayload {
    /// Module payload schema version.
    pub version: u16,
    /// Opaque bytes interpreted only by the negotiated module schema.
    pub data: Vec<u8>,
}

/// Typed provider input carried across the Rust-to-Host boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum MeasurementPayload {
    /// Built-in shaped and wrapped text.
    Text(TextMeasurePayload),
    /// Built-in replaced content.
    ReplacedContent(ReplacedContentMeasurePayload),
    /// A native control registered by an element provider.
    NativeControl(NativeControlMeasurePayload),
    /// A content-sized child surface.
    EmbeddedSurface(EmbeddedSurfaceMeasurePayload),
    /// A module-defined versioned payload.
    Custom(CustomMeasurePayload),
}

impl MeasurementPayload {
    /// Returns the semantic provider category and custom schema version.
    pub const fn kind(&self) -> MeasurementKind {
        match self {
            Self::Text(_) => MeasurementKind::Text,
            Self::ReplacedContent(_) => MeasurementKind::ReplacedContent,
            Self::NativeControl(_) => MeasurementKind::NativeControl,
            Self::EmbeddedSurface(_) => MeasurementKind::EmbeddedSurface,
            Self::Custom(payload) => MeasurementKind::Custom {
                version: payload.version,
            },
        }
    }

    /// Validates geometry and schema invariants before a payload reaches Host code.
    pub fn validate(&self) -> Result<(), MeasurementPayloadError> {
        match self {
            Self::Text(payload) => payload.validate(),
            Self::ReplacedContent(payload) => {
                if payload.intrinsic_size.is_some_and(|size| !size.is_valid()) {
                    return Err(MeasurementPayloadError::InvalidIntrinsicSize);
                }
                if payload
                    .aspect_ratio
                    .is_some_and(|ratio| !ratio.is_finite() || ratio <= 0.0)
                {
                    return Err(MeasurementPayloadError::InvalidAspectRatio);
                }
                Ok(())
            }
            Self::NativeControl(payload) => {
                if payload.control_type == 0 {
                    Err(MeasurementPayloadError::InvalidControlType)
                } else if payload.version == 0 {
                    Err(MeasurementPayloadError::InvalidPayloadVersion)
                } else {
                    Ok(())
                }
            }
            Self::EmbeddedSurface(payload) => {
                if payload.preferred_size.is_some_and(|size| !size.is_valid()) {
                    Err(MeasurementPayloadError::InvalidPreferredSize)
                } else {
                    Ok(())
                }
            }
            Self::Custom(payload) => {
                if payload.version == 0 {
                    Err(MeasurementPayloadError::InvalidPayloadVersion)
                } else {
                    Ok(())
                }
            }
        }
    }
}

impl TextMeasurePayload {
    pub(crate) fn validate(&self) -> Result<(), MeasurementPayloadError> {
        if self.style.font_families.is_empty()
            || self
                .style
                .font_families
                .iter()
                .any(|family| matches!(family, MeasureFontFamily::Named(name) if name.is_empty()))
        {
            return Err(MeasurementPayloadError::InvalidFontFamily);
        }
        if !self.style.font_size.is_finite() || self.style.font_size < 0.0 {
            return Err(MeasurementPayloadError::InvalidFontSize);
        }
        if !(1..=1000).contains(&self.style.font_weight) {
            return Err(MeasurementPayloadError::InvalidFontWeight);
        }
        if matches!(self.style.line_height, MeasureLineHeight::LogicalPixels(value) if !value.is_finite() || value < 0.0)
        {
            return Err(MeasurementPayloadError::InvalidLineHeight);
        }
        if !self.style.letter_spacing.is_finite() {
            return Err(MeasurementPayloadError::InvalidLetterSpacing);
        }
        if !strictly_sorted_tags(self.style.features.iter().map(|feature| feature.tag)) {
            return Err(MeasurementPayloadError::InvalidFontFeatures);
        }
        if !strictly_sorted_tags(self.style.variations.iter().map(|variation| variation.tag))
            || self
                .style
                .variations
                .iter()
                .any(|variation| !variation.value.is_finite())
        {
            return Err(MeasurementPayloadError::InvalidFontVariations);
        }
        if self.locale.as_ref().is_some_and(|locale| locale.is_empty()) {
            return Err(MeasurementPayloadError::InvalidLocale);
        }
        if self.max_lines == Some(0) {
            return Err(MeasurementPayloadError::InvalidMaxLines);
        }
        Ok(())
    }
}

/// Malformed typed measurement input rejected before calling a Host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasurementPayloadError {
    /// Text has no usable font fallback.
    InvalidFontFamily,
    /// Font size is negative or non-finite.
    InvalidFontSize,
    /// Font weight is outside 1 through 1000.
    InvalidFontWeight,
    /// Explicit line height is negative or non-finite.
    InvalidLineHeight,
    /// Letter spacing is non-finite.
    InvalidLetterSpacing,
    /// Feature tags are duplicated or not in canonical sorted order.
    InvalidFontFeatures,
    /// Variation tags are duplicated, unsorted, or paired with non-finite values.
    InvalidFontVariations,
    /// Locale is present but empty.
    InvalidLocale,
    /// A present line limit is zero.
    InvalidMaxLines,
    /// Replaced-content intrinsic dimensions are invalid.
    InvalidIntrinsicSize,
    /// Replaced-content aspect ratio is non-positive or non-finite.
    InvalidAspectRatio,
    /// A native-control schema identifier is zero.
    InvalidControlType,
    /// A native-control or custom payload version is zero.
    InvalidPayloadVersion,
    /// An embedded surface preferred size is invalid.
    InvalidPreferredSize,
}

fn strictly_sorted_tags(tags: impl Iterator<Item = FontTag>) -> bool {
    let mut previous = None;
    for tag in tags {
        if previous.is_some_and(|previous| previous >= tag) {
            return false;
        }
        previous = Some(tag);
    }
    true
}

/// Space made available by the box-layout algorithm on one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AvailableSpace {
    /// A finite logical-pixel constraint.
    Definite(f32),
    /// The smallest size that avoids avoidable content overflow.
    MinContent,
    /// The unconstrained content size.
    MaxContent,
}

/// Width and height constraints supplied by Rust layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasureConstraints {
    /// Dimensions already fixed by the layout algorithm.
    pub known_dimensions: [Option<f32>; 2],
    /// Remaining width and height availability.
    pub available_space: [AvailableSpace; 2],
}

/// A width and height in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeasuredSize {
    /// Horizontal extent.
    pub width: f32,
    /// Vertical extent.
    pub height: f32,
}

impl MeasuredSize {
    /// Creates a measured logical-pixel size.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Returns whether both dimensions are finite and non-negative.
    pub fn is_valid(self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width >= 0.0 && self.height >= 0.0
    }
}

/// Layout behavior while a provider cannot return final intrinsic metrics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PendingMeasurePolicy {
    /// Withhold new geometry until final metrics arrive.
    Block,
    /// Lay out using a schema-declared placeholder.
    Placeholder(MeasuredSize),
    /// Reuse the node's last final measurement, blocking when none exists.
    RetainPrevious,
}

/// Stable content inputs registered for one intrinsically sized node.
///
/// `content_hash` and `style_hash` must cover every payload field that can
/// affect measurement. The engine combines them with constraints, element
/// type, measurement kind, and the environment epoch to form its cache key.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasurementSpec {
    /// Hash of content or resource identity.
    pub content_hash: u64,
    /// Hash of measurement-affecting resolved style and provider options.
    pub style_hash: u64,
    /// Typed inputs interpreted by the selected provider.
    pub payload: MeasurementPayload,
    /// Explicit behavior for deferred results.
    pub pending_policy: PendingMeasurePolicy,
}

/// One cache-missing measurement sent from Rust to a renderer provider.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasurementRequest {
    /// Opaque immediate-response correlation key.
    pub key: MeasurementKey,
    /// Representative scene node for diagnostics and element lookup.
    pub node: NodeId,
    /// Negotiated element schema.
    pub element_type: ElementTypeId,
    /// Host environment generation used by this request.
    pub environment_epoch: u64,
    /// Layout constraints under which content must be measured.
    pub constraints: MeasureConstraints,
    /// Typed provider inputs.
    pub payload: MeasurementPayload,
}

/// Final or provisional intrinsic content metrics.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasurementMetrics {
    /// Measured content size.
    pub size: MeasuredSize,
    /// First line or provider-defined alignment baseline from the content top.
    pub first_baseline: Option<f32>,
    /// Last line or provider-defined alignment baseline from the content top.
    pub last_baseline: Option<f32>,
    /// Content overflow bounds relative to the measured content origin.
    pub overflow: Option<LayoutRect>,
    /// Host object that must also be used to paint these exact metrics.
    pub prepared_content: Option<PreparedContentId>,
}

impl MeasurementMetrics {
    /// Creates metrics containing only a size.
    pub const fn from_size(size: MeasuredSize) -> Self {
        Self {
            size,
            first_baseline: None,
            last_baseline: None,
            overflow: None,
            prepared_content: None,
        }
    }

    /// Returns whether all geometric values are finite and sizes non-negative.
    pub fn is_valid(&self) -> bool {
        self.size.is_valid()
            && [self.first_baseline, self.last_baseline]
                .into_iter()
                .flatten()
                .all(f32::is_finite)
            && self.overflow.is_none_or(|rect| {
                [rect.x, rect.y, rect.width, rect.height]
                    .into_iter()
                    .all(f32::is_finite)
                    && rect.width >= 0.0
                    && rect.height >= 0.0
            })
    }
}

/// Why a renderer cannot implement a requested measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnsupportedMeasurementReason {
    /// The backend does not implement this measurement category.
    Kind,
    /// The negotiated element has no compatible Host implementation.
    Element,
    /// The provider does not understand the payload schema version.
    PayloadVersion,
    /// Required platform facilities are unavailable in this environment.
    Environment,
    /// The backend does not implement one of the requested semantic features.
    Feature,
}

/// Immediate response to one entry in a measurement batch.
#[derive(Clone, Debug, PartialEq)]
pub enum MeasurementResponse {
    /// Final metrics are available synchronously.
    Ready {
        /// Request correlation key.
        key: MeasurementKey,
        /// Environment generation copied from the request.
        environment_epoch: u64,
        /// Final intrinsic metrics.
        metrics: MeasurementMetrics,
    },
    /// Final metrics will arrive through a later Host-to-Rust event.
    Pending {
        /// Request correlation key.
        key: MeasurementKey,
        /// Environment generation copied from the request.
        environment_epoch: u64,
        /// Deferred-result correlation identifier allocated by the Host.
        request_id: MeasurementRequestId,
        /// Optional provider-supplied provisional metrics.
        provisional: Option<MeasurementMetrics>,
    },
    /// The selected provider cannot measure this request.
    Unsupported {
        /// Request correlation key.
        key: MeasurementKey,
        /// Environment generation copied from the request.
        environment_epoch: u64,
        /// Stable diagnostic category.
        reason: UnsupportedMeasurementReason,
    },
}

impl MeasurementResponse {
    /// Returns the immediate request correlation key.
    pub const fn key(&self) -> MeasurementKey {
        match self {
            Self::Ready { key, .. } | Self::Pending { key, .. } | Self::Unsupported { key, .. } => {
                *key
            }
        }
    }

    /// Returns the environment generation copied from the request.
    pub const fn environment_epoch(&self) -> u64 {
        match self {
            Self::Ready {
                environment_epoch, ..
            }
            | Self::Pending {
                environment_epoch, ..
            }
            | Self::Unsupported {
                environment_epoch, ..
            } => *environment_epoch,
        }
    }
}

/// Structural error in one immediate Host measurement batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasurementBatchError {
    /// Rust supplied the same request key more than once.
    DuplicateRequestKey {
        /// Duplicated key.
        key: MeasurementKey,
    },
    /// Host returned the same response key more than once.
    DuplicateResponseKey {
        /// Duplicated key.
        key: MeasurementKey,
    },
    /// Host returned a key absent from the submitted batch.
    UnexpectedResponseKey {
        /// Unknown key.
        key: MeasurementKey,
    },
    /// Host omitted a response for a submitted request.
    MissingResponseKey {
        /// Missing key.
        key: MeasurementKey,
    },
    /// Host did not echo the request environment generation.
    EnvironmentEpochMismatch {
        /// Correlation key.
        key: MeasurementKey,
        /// Request generation.
        expected: u64,
        /// Host-returned generation.
        received: u64,
    },
}

/// Validates that a Host returned exactly one correlated result per request.
///
/// Response order is deliberately irrelevant so platform bindings may fill a
/// preallocated buffer in their most efficient order.
pub fn validate_measurement_batch(
    requests: &[MeasurementRequest],
    responses: &[MeasurementResponse],
) -> Result<(), MeasurementBatchError> {
    let mut expected = BTreeMap::new();
    for request in requests {
        if expected
            .insert(request.key, request.environment_epoch)
            .is_some()
        {
            return Err(MeasurementBatchError::DuplicateRequestKey { key: request.key });
        }
    }

    let mut received = BTreeSet::new();
    for response in responses {
        let key = response.key();
        let Some(expected_epoch) = expected.get(&key).copied() else {
            return Err(MeasurementBatchError::UnexpectedResponseKey { key });
        };
        if !received.insert(key) {
            return Err(MeasurementBatchError::DuplicateResponseKey { key });
        }
        let received_epoch = response.environment_epoch();
        if expected_epoch != received_epoch {
            return Err(MeasurementBatchError::EnvironmentEpochMismatch {
                key,
                expected: expected_epoch,
                received: received_epoch,
            });
        }
    }

    if let Some(key) = expected.keys().find(|key| !received.contains(key)) {
        return Err(MeasurementBatchError::MissingResponseKey { key: *key });
    }
    Ok(())
}

/// Deferred Host-to-Rust completion for a prior [`MeasurementResponse::Pending`].
#[derive(Clone, Debug, PartialEq)]
pub struct MeasurementReady {
    /// Immediate request key that originally produced the pending response.
    pub key: MeasurementKey,
    /// Deferred-result correlation identifier.
    pub request_id: MeasurementRequestId,
    /// Environment generation under which the content was measured.
    pub environment_epoch: u64,
    /// Final intrinsic metrics.
    pub metrics: MeasurementMetrics,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> MeasurementKey {
        MeasurementKey::new(1).expect("test key")
    }

    fn text_payload() -> TextMeasurePayload {
        TextMeasurePayload {
            text: "Hello".into(),
            style: TextMeasureStyle {
                font_families: vec![
                    MeasureFontFamily::System,
                    MeasureFontFamily::Named("Inter".into()),
                ],
                font_size: 14.0,
                font_weight: 400,
                font_style: MeasureFontStyle::Normal,
                line_height: MeasureLineHeight::Normal,
                letter_spacing: 0.0,
                ..TextMeasureStyle::default()
            },
            locale: Some("en-US".into()),
            direction: MeasureTextDirection::Auto,
            wrap: MeasureTextWrap::Wrap,
            max_lines: None,
            overflow: MeasureTextOverflow::Clip,
        }
    }

    fn request(key_value: u64, epoch: u64) -> MeasurementRequest {
        MeasurementRequest {
            key: MeasurementKey::new(key_value).expect("request key"),
            node: NodeId::new(key_value).expect("node"),
            element_type: ElementTypeId::new(1).expect("element"),
            environment_epoch: epoch,
            constraints: MeasureConstraints {
                known_dimensions: [None, None],
                available_space: [AvailableSpace::MaxContent, AvailableSpace::MinContent],
            },
            payload: MeasurementPayload::Text(text_payload()),
        }
    }

    fn ready(request: &MeasurementRequest) -> MeasurementResponse {
        MeasurementResponse::Ready {
            key: request.key,
            environment_epoch: request.environment_epoch,
            metrics: MeasurementMetrics::from_size(MeasuredSize::default()),
        }
    }

    #[test]
    fn metric_validation_covers_sizes_baselines_and_overflow() {
        let size = MeasuredSize::new(12.0, 8.0);
        assert!(size.is_valid());
        assert!(!MeasuredSize::new(-1.0, 0.0).is_valid());
        assert!(!MeasuredSize::new(f32::NAN, 0.0).is_valid());

        let mut metrics = MeasurementMetrics::from_size(size);
        assert!(metrics.is_valid());
        metrics.first_baseline = Some(6.0);
        metrics.last_baseline = Some(7.0);
        metrics.overflow = Some(LayoutRect {
            x: -1.0,
            y: -2.0,
            width: 14.0,
            height: 11.0,
        });
        assert!(metrics.is_valid());
        metrics.first_baseline = Some(f32::INFINITY);
        assert!(!metrics.is_valid());
        metrics.first_baseline = None;
        metrics.overflow.as_mut().expect("overflow").width = -1.0;
        assert!(!metrics.is_valid());
        metrics.overflow.as_mut().expect("overflow").width = f32::NAN;
        assert!(!metrics.is_valid());
    }

    #[test]
    fn responses_expose_common_correlation_fields() {
        let environment_epoch = 9;
        let responses = [
            MeasurementResponse::Ready {
                key: key(),
                environment_epoch,
                metrics: MeasurementMetrics::from_size(MeasuredSize::default()),
            },
            MeasurementResponse::Pending {
                key: key(),
                environment_epoch,
                request_id: MeasurementRequestId::new(2).expect("request"),
                provisional: None,
            },
            MeasurementResponse::Unsupported {
                key: key(),
                environment_epoch,
                reason: UnsupportedMeasurementReason::Kind,
            },
        ];
        for response in responses {
            assert_eq!(response.key(), key());
            assert_eq!(response.environment_epoch(), environment_epoch);
        }
    }

    #[test]
    fn semantic_variants_remain_distinct() {
        assert_ne!(MeasurementKind::Text, MeasurementKind::ReplacedContent);
        assert_ne!(
            MeasurementKind::NativeControl,
            MeasurementKind::EmbeddedSurface
        );
        assert_ne!(
            MeasurementKind::Custom { version: 1 },
            MeasurementKind::Custom { version: 2 }
        );
        assert_ne!(
            PendingMeasurePolicy::Block,
            PendingMeasurePolicy::RetainPrevious
        );
        assert_ne!(AvailableSpace::MinContent, AvailableSpace::MaxContent);
        assert_ne!(
            UnsupportedMeasurementReason::Element,
            UnsupportedMeasurementReason::PayloadVersion
        );
        assert_ne!(
            UnsupportedMeasurementReason::Environment,
            UnsupportedMeasurementReason::Kind
        );
    }

    #[test]
    fn font_tags_and_extended_typography_cover_each_path() {
        let kern = FontTag::new(*b"kern").expect("printable tag");
        assert_eq!(kern.get(), *b"kern");
        assert_eq!(FontTag::new([0x1f, b'e', b'r', b'n']), None);
        assert_eq!(FontTag::new([b'k', b'e', b'r', 0x7f]), None);

        let mut style = TextMeasureStyle::default();
        assert!(!style.uses_extended_typography());
        style.features.push(FontFeature {
            tag: kern,
            value: 1,
        });
        assert!(style.uses_extended_typography());
        style.features.clear();
        style.variations.push(FontVariation {
            tag: FontTag::new(*b"wght").unwrap(),
            value: 500.0,
        });
        assert!(style.uses_extended_typography());
        style.variations.clear();
        style.optical_sizing = FontOpticalSizing::None;
        assert!(style.uses_extended_typography());
    }

    #[test]
    fn typed_payloads_report_kinds_and_reject_every_invalid_field() {
        let text = MeasurementPayload::Text(text_payload());
        assert_eq!(text.kind(), MeasurementKind::Text);
        assert_eq!(text.validate(), Ok(()));

        let mut invalid = text_payload();
        invalid.style.font_families.clear();
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidFontFamily)
        );
        let mut invalid = text_payload();
        invalid.style.font_families = vec![MeasureFontFamily::Named(String::new())];
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidFontFamily)
        );
        let mut invalid = text_payload();
        invalid.style.font_size = f32::NAN;
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidFontSize)
        );
        let mut invalid = text_payload();
        invalid.style.font_size = -1.0;
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidFontSize)
        );
        for weight in [0, 1001] {
            let mut invalid = text_payload();
            invalid.style.font_weight = weight;
            assert_eq!(
                MeasurementPayload::Text(invalid).validate(),
                Err(MeasurementPayloadError::InvalidFontWeight)
            );
        }
        for height in [f32::NAN, -1.0] {
            let mut invalid = text_payload();
            invalid.style.line_height = MeasureLineHeight::LogicalPixels(height);
            assert_eq!(
                MeasurementPayload::Text(invalid).validate(),
                Err(MeasurementPayloadError::InvalidLineHeight)
            );
        }
        let mut invalid = text_payload();
        invalid.style.letter_spacing = f32::INFINITY;
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidLetterSpacing)
        );
        let kern = FontTag::new(*b"kern").unwrap();
        let liga = FontTag::new(*b"liga").unwrap();
        let mut invalid = text_payload();
        invalid.style.features = vec![
            FontFeature {
                tag: liga,
                value: 1,
            },
            FontFeature {
                tag: kern,
                value: 1,
            },
        ];
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidFontFeatures)
        );
        let mut invalid = text_payload();
        invalid.style.variations = vec![
            FontVariation {
                tag: FontTag::new(*b"wght").unwrap(),
                value: 500.0,
            },
            FontVariation {
                tag: FontTag::new(*b"opsz").unwrap(),
                value: 14.0,
            },
        ];
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidFontVariations)
        );
        let mut invalid = text_payload();
        invalid.style.variations = vec![FontVariation {
            tag: FontTag::new(*b"wght").unwrap(),
            value: f32::NAN,
        }];
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidFontVariations)
        );
        let mut invalid = text_payload();
        invalid.locale = Some(String::new());
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidLocale)
        );
        let mut invalid = text_payload();
        invalid.max_lines = Some(0);
        assert_eq!(
            MeasurementPayload::Text(invalid).validate(),
            Err(MeasurementPayloadError::InvalidMaxLines)
        );

        let replaced = MeasurementPayload::ReplacedContent(ReplacedContentMeasurePayload {
            intrinsic_size: Some(MeasuredSize::new(10.0, 20.0)),
            aspect_ratio: Some(0.5),
        });
        assert_eq!(replaced.kind(), MeasurementKind::ReplacedContent);
        assert_eq!(replaced.validate(), Ok(()));
        assert_eq!(
            MeasurementPayload::ReplacedContent(ReplacedContentMeasurePayload {
                intrinsic_size: Some(MeasuredSize::new(-1.0, 1.0)),
                aspect_ratio: None,
            })
            .validate(),
            Err(MeasurementPayloadError::InvalidIntrinsicSize)
        );
        for ratio in [0.0, f32::NAN] {
            assert_eq!(
                MeasurementPayload::ReplacedContent(ReplacedContentMeasurePayload {
                    intrinsic_size: None,
                    aspect_ratio: Some(ratio),
                })
                .validate(),
                Err(MeasurementPayloadError::InvalidAspectRatio)
            );
        }

        let control = MeasurementPayload::NativeControl(NativeControlMeasurePayload {
            control_type: 1,
            version: 1,
            state: vec![1],
        });
        assert_eq!(control.kind(), MeasurementKind::NativeControl);
        assert_eq!(control.validate(), Ok(()));
        assert_eq!(
            MeasurementPayload::NativeControl(NativeControlMeasurePayload {
                control_type: 0,
                version: 1,
                state: Vec::new(),
            })
            .validate(),
            Err(MeasurementPayloadError::InvalidControlType)
        );
        assert_eq!(
            MeasurementPayload::NativeControl(NativeControlMeasurePayload {
                control_type: 1,
                version: 0,
                state: Vec::new(),
            })
            .validate(),
            Err(MeasurementPayloadError::InvalidPayloadVersion)
        );

        let child_surface = SurfaceId::new(2).expect("child surface");
        let embedded = MeasurementPayload::EmbeddedSurface(EmbeddedSurfaceMeasurePayload {
            surface: child_surface,
            preferred_size: Some(MeasuredSize::new(4.0, 5.0)),
        });
        assert_eq!(embedded.kind(), MeasurementKind::EmbeddedSurface);
        assert_eq!(embedded.validate(), Ok(()));
        assert_eq!(
            MeasurementPayload::EmbeddedSurface(EmbeddedSurfaceMeasurePayload {
                surface: child_surface,
                preferred_size: Some(MeasuredSize::new(1.0, -1.0)),
            })
            .validate(),
            Err(MeasurementPayloadError::InvalidPreferredSize)
        );

        let custom_data = CustomMeasurePayload {
            version: 3,
            data: vec![1, 2],
        };
        let cloned = <CustomMeasurePayload as Clone>::clone(std::hint::black_box(&custom_data));
        assert_eq!(std::hint::black_box(cloned), custom_data);
        assert!(format!("{custom_data:?}").contains("CustomMeasurePayload"));
        let custom = MeasurementPayload::Custom(custom_data);
        assert_eq!(custom.kind(), MeasurementKind::Custom { version: 3 });
        assert_eq!(custom.validate(), Ok(()));
        assert_eq!(
            MeasurementPayload::Custom(CustomMeasurePayload {
                version: 0,
                data: Vec::new(),
            })
            .validate(),
            Err(MeasurementPayloadError::InvalidPayloadVersion)
        );

        let mut alternate_text = text_payload();
        alternate_text.style.font_style = MeasureFontStyle::Italic;
        alternate_text.style.line_height = MeasureLineHeight::LogicalPixels(18.0);
        alternate_text.direction = MeasureTextDirection::RightToLeft;
        alternate_text.wrap = MeasureTextWrap::NoWrap;
        alternate_text.overflow = MeasureTextOverflow::Ellipsis;
        assert_eq!(MeasurementPayload::Text(alternate_text).validate(), Ok(()));
        assert_ne!(MeasureFontStyle::Oblique, MeasureFontStyle::Italic);
        assert_ne!(
            MeasureTextDirection::LeftToRight,
            MeasureTextDirection::Auto
        );
    }

    #[test]
    fn batch_validation_requires_one_epoch_matched_response_per_key() {
        assert_eq!(validate_measurement_batch(&[], &[]), Ok(()));
        let first = request(1, 7);
        let second = request(2, 7);
        assert_eq!(
            validate_measurement_batch(
                &[first.clone(), second.clone()],
                &[ready(&second), ready(&first)]
            ),
            Ok(())
        );
        assert_eq!(
            validate_measurement_batch(&[first.clone(), first.clone()], &[]),
            Err(MeasurementBatchError::DuplicateRequestKey { key: first.key })
        );

        let unexpected = request(3, 7);
        assert_eq!(
            validate_measurement_batch(std::slice::from_ref(&first), &[ready(&unexpected)]),
            Err(MeasurementBatchError::UnexpectedResponseKey {
                key: unexpected.key
            })
        );
        assert_eq!(
            validate_measurement_batch(
                std::slice::from_ref(&first),
                &[ready(&first), ready(&first)]
            ),
            Err(MeasurementBatchError::DuplicateResponseKey { key: first.key })
        );

        let wrong_epoch = MeasurementResponse::Ready {
            key: first.key,
            environment_epoch: 8,
            metrics: MeasurementMetrics::from_size(MeasuredSize::default()),
        };
        assert_eq!(
            validate_measurement_batch(std::slice::from_ref(&first), &[wrong_epoch]),
            Err(MeasurementBatchError::EnvironmentEpochMismatch {
                key: first.key,
                expected: 7,
                received: 8,
            })
        );
        assert_eq!(
            validate_measurement_batch(&[first.clone(), second], &[ready(&first)]),
            Err(MeasurementBatchError::MissingResponseKey {
                key: MeasurementKey::new(2).expect("second key")
            })
        );
    }
}
