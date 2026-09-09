use whisker_protocol::{InlinePlacement, MeasurementKey, NodeId};

use crate::LayoutSize;

/// Whether an intrinsic result can participate in a committed frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum MeasurementState {
    /// Final metrics for the current constraints.
    #[default]
    Ready,
    /// Explicit provider or schema fallback awaiting final metrics.
    Provisional,
    /// No permissible fallback exists.
    Blocked,
}

/// Intrinsic metrics and the exact Host result selected by layout.
#[derive(Clone, Debug, Default)]
pub struct IntrinsicResult {
    /// Content-box dimensions.
    pub size: LayoutSize,
    /// First baseline relative to the content-box top.
    pub first_baseline: Option<f32>,
    /// Readiness of this result and its measured dependencies.
    pub state: MeasurementState,
    /// Accepted result identity, independent of later intrinsic probes.
    pub selection: Option<MeasurementKey>,
    /// Inline child origins from the same accepted paragraph.
    pub inline_placements: Vec<InlinePlacement>,
}

impl From<LayoutSize> for IntrinsicResult {
    fn from(size: LayoutSize) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }
}

/// Dimensions of an atomic subtree supplied to a paragraph measurer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredInlineChild {
    /// Retained child identity.
    pub node: NodeId,
    /// Margin-box dimensions after intrinsic sizing.
    pub size: LayoutSize,
    /// Baseline relative to the margin-box top.
    pub baseline: f32,
}
