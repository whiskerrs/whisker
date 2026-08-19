//! Semantic intrinsic-measurement values shared by Rust and Host renderers.

use crate::{
    ElementTypeId, LayoutRect, MeasurementKey, MeasurementRequestId, NodeId, PreparedContentId,
    ProtocolValue,
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
    /// A versioned element-specific measurement implemented by a module.
    Custom {
        /// Module-defined payload schema version.
        version: u16,
    },
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
    /// Provider category.
    pub kind: MeasurementKind,
    /// Hash of content or resource identity.
    pub content_hash: u64,
    /// Hash of measurement-affecting resolved style and provider options.
    pub style_hash: u64,
    /// Versioned typed inputs interpreted by the selected provider.
    pub payload: ProtocolValue,
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
    /// Provider category.
    pub kind: MeasurementKind,
    /// Layout constraints under which content must be measured.
    pub constraints: MeasureConstraints,
    /// Versioned provider inputs.
    pub payload: ProtocolValue,
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
}
