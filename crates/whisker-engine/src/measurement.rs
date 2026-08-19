//! Retained intrinsic-measurement coordination for one surface.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    error::Error,
    fmt,
};

use whisker_layout::{IntrinsicMeasurer, LayoutSize, MeasureRequest as LayoutMeasureRequest};
use whisker_protocol::{
    AvailableSpace, ElementTypeId, MeasureConstraints, MeasuredSize, MeasurementKey,
    MeasurementKind, MeasurementMetrics, MeasurementReady, MeasurementRequest,
    MeasurementRequestId, MeasurementResponse, MeasurementSpec, NodeId, PendingMeasurePolicy,
    UnsupportedMeasurementReason,
};

use crate::LayoutUpdate;

/// State reached by one Host-backed layout attempt.
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutProgress {
    /// Every intrinsic measurement was final and geometry can be presented.
    Complete(LayoutUpdate),
    /// Geometry used an explicit provisional policy and may be presented.
    Provisional {
        /// Geometry projected into the retained scene.
        update: LayoutUpdate,
        /// Cache-missing requests that should be sent in one Host batch.
        requests: Vec<MeasurementRequest>,
        /// Number of deferred Host requests still awaiting completion.
        pending: usize,
    },
    /// Geometry was withheld because at least one measurement had no fallback.
    Blocked {
        /// Cache-missing requests that should be sent in one Host batch.
        requests: Vec<MeasurementRequest>,
        /// Number of deferred Host requests still awaiting completion.
        pending: usize,
    },
}

impl LayoutProgress {
    /// Returns requests that must be passed to `RendererV1::measure_batch`.
    pub fn requests(&self) -> &[MeasurementRequest] {
        match self {
            Self::Complete(_) => &[],
            Self::Provisional { requests, .. } | Self::Blocked { requests, .. } => requests,
        }
    }

    /// Returns whether geometry was committed to the retained scene.
    pub const fn has_layout(&self) -> bool {
        matches!(self, Self::Complete(_) | Self::Provisional { .. })
    }
}

/// Summary of applying immediate Host measurement responses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementApply {
    applied: usize,
    stale: usize,
    invalidated_nodes: Vec<NodeId>,
}

impl MeasurementApply {
    /// Returns the number of responses accepted into retained state.
    pub const fn applied(&self) -> usize {
        self.applied
    }

    /// Returns the number of unknown or wrong-epoch responses ignored.
    pub const fn stale(&self) -> usize {
        self.stale
    }

    pub(crate) fn invalidated_nodes(&self) -> &[NodeId] {
        &self.invalidated_nodes
    }
}

/// Result of applying one deferred Host measurement event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeferredMeasurementApply {
    /// Final metrics replaced a pending entry and dirtied current consumers.
    Applied {
        /// Nodes whose Taffy measurement cache must be invalidated.
        invalidated_nodes: Vec<NodeId>,
    },
    /// The request, key, or environment generation was no longer current.
    IgnoredStale,
}

impl DeferredMeasurementApply {
    pub(crate) fn invalidated_nodes(&self) -> &[NodeId] {
        match self {
            Self::Applied { invalidated_nodes } => invalidated_nodes,
            Self::IgnoredStale => &[],
        }
    }
}

/// Failure while validating or resolving intrinsic measurement.
#[derive(Clone, Debug, PartialEq)]
pub enum MeasurementError {
    /// A schema declared an invalid provisional placeholder.
    InvalidPlaceholder {
        /// Node whose schema was rejected.
        node: NodeId,
    },
    /// A Host response contained non-finite or negative geometry.
    InvalidMetrics {
        /// Immediate request correlation key.
        key: MeasurementKey,
    },
    /// Two live pending responses reused a deferred request identifier.
    DuplicateRequestId {
        /// Reused Host identifier.
        request_id: MeasurementRequestId,
    },
    /// A measurable Taffy leaf had no paired measurement schema.
    MissingSpec {
        /// Inconsistent retained node.
        node: NodeId,
    },
    /// A required provider explicitly rejected the request.
    Unsupported {
        /// Node that required measurement.
        node: NodeId,
        /// Provider diagnostic category.
        reason: UnsupportedMeasurementReason,
    },
    /// The surface exhausted its immediate correlation-key space.
    KeyExhausted,
}

impl fmt::Display for MeasurementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker measurement error: {self:?}")
    }
}

impl Error for MeasurementError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AvailableSpaceKey {
    Definite(u32),
    MinContent,
    MaxContent,
}

impl From<AvailableSpace> for AvailableSpaceKey {
    fn from(value: AvailableSpace) -> Self {
        match value {
            AvailableSpace::Definite(value) => Self::Definite(value.to_bits()),
            AvailableSpace::MinContent => Self::MinContent,
            AvailableSpace::MaxContent => Self::MaxContent,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ConstraintsKey {
    known_dimensions: [Option<u32>; 2],
    available_space: [AvailableSpaceKey; 2],
}

impl From<MeasureConstraints> for ConstraintsKey {
    fn from(value: MeasureConstraints) -> Self {
        Self {
            known_dimensions: value
                .known_dimensions
                .map(|dimension| dimension.map(f32::to_bits)),
            available_space: value.available_space.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    element_type: ElementTypeId,
    kind: MeasurementKind,
    content_hash: u64,
    style_hash: u64,
    environment_epoch: u64,
    constraints: ConstraintsKey,
}

#[derive(Clone, Debug)]
struct SpecState {
    element_type: ElementTypeId,
    spec: MeasurementSpec,
    last_ready: Option<MeasurementMetrics>,
}

#[derive(Clone, Debug)]
enum CachedState {
    Ready(MeasurementMetrics),
    Pending {
        key: MeasurementKey,
        request_id: MeasurementRequestId,
        provisional: Option<MeasurementMetrics>,
    },
    Unsupported(UnsupportedMeasurementReason),
}

#[derive(Clone, Debug)]
struct CacheEntry {
    state: CachedState,
    consumers: BTreeSet<NodeId>,
}

#[derive(Clone, Debug)]
struct OutstandingRequest {
    request: MeasurementRequest,
    cache_key: CacheKey,
    consumers: BTreeSet<NodeId>,
}

#[derive(Clone, Debug)]
struct PendingRequest {
    key: MeasurementKey,
    cache_key: CacheKey,
    environment_epoch: u64,
}

#[derive(Clone, Debug, Default)]
struct PassState {
    requests: BTreeMap<MeasurementKey, MeasurementRequest>,
    pending: BTreeSet<MeasurementRequestId>,
    blocking: bool,
    provisional: bool,
    error: Option<MeasurementError>,
}

#[derive(Clone, Debug)]
pub(crate) struct MeasurementCoordinator {
    specs: BTreeMap<NodeId, SpecState>,
    cache: HashMap<CacheKey, CacheEntry>,
    outstanding: BTreeMap<MeasurementKey, OutstandingRequest>,
    outstanding_by_cache: HashMap<CacheKey, MeasurementKey>,
    pending: BTreeMap<MeasurementRequestId, PendingRequest>,
    environment_epoch: Option<u64>,
    next_key: u64,
    pass: PassState,
}

impl Default for MeasurementCoordinator {
    fn default() -> Self {
        Self {
            specs: BTreeMap::new(),
            cache: HashMap::new(),
            outstanding: BTreeMap::new(),
            outstanding_by_cache: HashMap::new(),
            pending: BTreeMap::new(),
            environment_epoch: None,
            next_key: 1,
            pass: PassState::default(),
        }
    }
}

impl MeasurementCoordinator {
    pub(crate) fn set_environment(&mut self, epoch: u64) -> Vec<NodeId> {
        let changed = self
            .environment_epoch
            .is_some_and(|current| current != epoch);
        self.environment_epoch = Some(epoch);
        if !changed {
            return Vec::new();
        }
        self.cache.clear();
        self.outstanding.clear();
        self.outstanding_by_cache.clear();
        self.pending.clear();
        self.specs.keys().copied().collect()
    }

    pub(crate) fn set_spec(
        &mut self,
        node: NodeId,
        element_type: ElementTypeId,
        spec: Option<MeasurementSpec>,
    ) -> Result<bool, MeasurementError> {
        if let Some(MeasurementSpec {
            pending_policy: PendingMeasurePolicy::Placeholder(size),
            ..
        }) = &spec
            && !size.is_valid()
        {
            return Err(MeasurementError::InvalidPlaceholder { node });
        }
        if self
            .specs
            .get(&node)
            .map(|state| (&state.element_type, &state.spec))
            == spec.as_ref().map(|spec| (&element_type, spec))
        {
            return Ok(false);
        }

        self.remove_consumer(node);
        let previous = self.specs.remove(&node).and_then(|state| state.last_ready);
        if let Some(spec) = spec {
            self.specs.insert(
                node,
                SpecState {
                    element_type,
                    spec,
                    last_ready: previous,
                },
            );
        }
        Ok(true)
    }

    pub(crate) fn remove_node(&mut self, node: NodeId) {
        self.remove_consumer(node);
        self.specs.remove(&node);
    }

    pub(crate) fn last_ready(&self, node: NodeId) -> Option<&MeasurementMetrics> {
        self.specs.get(&node)?.last_ready.as_ref()
    }

    pub(crate) fn outstanding_requests(&self) -> Vec<MeasurementRequest> {
        self.outstanding
            .values()
            .filter(|request| !request.consumers.is_empty())
            .map(|request| request.request.clone())
            .collect()
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.cache
            .values()
            .filter(|entry| {
                !entry.consumers.is_empty() && matches!(entry.state, CachedState::Pending { .. })
            })
            .count()
    }

    pub(crate) fn unresolved_nodes(&self) -> Vec<NodeId> {
        let mut nodes = BTreeSet::new();
        for request in self.outstanding.values() {
            nodes.extend(&request.consumers);
        }
        for entry in self.cache.values() {
            if matches!(entry.state, CachedState::Pending { .. }) {
                nodes.extend(&entry.consumers);
            }
        }
        nodes.into_iter().collect()
    }

    pub(crate) fn begin_pass(&mut self) {
        self.pass = PassState::default();
    }

    pub(crate) fn finish_pass(&mut self) -> Result<PassSummary, MeasurementError> {
        if let Some(error) = self.pass.error.take() {
            return Err(error);
        }
        Ok(PassSummary {
            requests: std::mem::take(&mut self.pass.requests)
                .into_values()
                .collect(),
            pending: self.pass.pending.len(),
            blocking: self.pass.blocking,
            provisional: self.pass.provisional,
        })
    }

    pub(crate) fn apply_batch(
        &mut self,
        responses: &[MeasurementResponse],
    ) -> Result<MeasurementApply, MeasurementError> {
        let mut next = self.clone();
        let mut apply = MeasurementApply::default();
        let mut invalidated = BTreeSet::new();
        for response in responses {
            next.apply_one(response, &mut apply, &mut invalidated)?;
        }
        apply.invalidated_nodes = invalidated.into_iter().collect();
        *self = next;
        Ok(apply)
    }

    pub(crate) fn apply_ready(
        &mut self,
        ready: &MeasurementReady,
    ) -> Result<DeferredMeasurementApply, MeasurementError> {
        let Some(pending) = self.pending.get(&ready.request_id).cloned() else {
            return Ok(DeferredMeasurementApply::IgnoredStale);
        };
        if pending.key != ready.key
            || pending.environment_epoch != ready.environment_epoch
            || self.environment_epoch != Some(ready.environment_epoch)
        {
            return Ok(DeferredMeasurementApply::IgnoredStale);
        }
        if !ready.metrics.is_valid() {
            return Err(MeasurementError::InvalidMetrics { key: ready.key });
        }
        let Some(entry) = self.cache.get_mut(&pending.cache_key) else {
            return Ok(DeferredMeasurementApply::IgnoredStale);
        };
        if !matches!(
            entry.state,
            CachedState::Pending {
                key,
                request_id,
                ..
            } if key == ready.key && request_id == ready.request_id
        ) {
            return Ok(DeferredMeasurementApply::IgnoredStale);
        }

        let consumers = entry.consumers.clone();
        entry.state = CachedState::Ready(ready.metrics.clone());
        self.pending.remove(&ready.request_id);
        self.remember_ready(&consumers, &ready.metrics);
        Ok(DeferredMeasurementApply::Applied {
            invalidated_nodes: consumers.into_iter().collect(),
        })
    }

    fn apply_one(
        &mut self,
        response: &MeasurementResponse,
        apply: &mut MeasurementApply,
        invalidated: &mut BTreeSet<NodeId>,
    ) -> Result<(), MeasurementError> {
        let key = response.key();
        let Some(outstanding) = self.outstanding.get(&key).cloned() else {
            apply.stale += 1;
            return Ok(());
        };
        if outstanding.request.environment_epoch != response.environment_epoch()
            || self.environment_epoch != Some(response.environment_epoch())
        {
            apply.stale += 1;
            return Ok(());
        }

        match response {
            MeasurementResponse::Ready { metrics, .. } => {
                self.validate_metrics(key, metrics)?;
            }
            MeasurementResponse::Pending {
                request_id,
                provisional,
                ..
            } => {
                if self.pending.contains_key(request_id) {
                    return Err(MeasurementError::DuplicateRequestId {
                        request_id: *request_id,
                    });
                }
                if let Some(metrics) = provisional {
                    self.validate_metrics(key, metrics)?;
                }
            }
            MeasurementResponse::Unsupported { .. } => {}
        }

        self.outstanding.remove(&key);
        self.outstanding_by_cache.remove(&outstanding.cache_key);
        invalidated.extend(&outstanding.consumers);
        let state = match response {
            MeasurementResponse::Ready { metrics, .. } => {
                self.remember_ready(&outstanding.consumers, metrics);
                CachedState::Ready(metrics.clone())
            }
            MeasurementResponse::Pending {
                request_id,
                provisional,
                ..
            } => {
                self.pending.insert(
                    *request_id,
                    PendingRequest {
                        key,
                        cache_key: outstanding.cache_key.clone(),
                        environment_epoch: response.environment_epoch(),
                    },
                );
                CachedState::Pending {
                    key,
                    request_id: *request_id,
                    provisional: provisional.clone(),
                }
            }
            MeasurementResponse::Unsupported { reason, .. } => CachedState::Unsupported(*reason),
        };
        self.cache.insert(
            outstanding.cache_key,
            CacheEntry {
                state,
                consumers: outstanding.consumers,
            },
        );
        apply.applied += 1;
        Ok(())
    }

    fn validate_metrics(
        &self,
        key: MeasurementKey,
        metrics: &MeasurementMetrics,
    ) -> Result<(), MeasurementError> {
        if metrics.is_valid() {
            Ok(())
        } else {
            Err(MeasurementError::InvalidMetrics { key })
        }
    }

    fn remember_ready(&mut self, consumers: &BTreeSet<NodeId>, metrics: &MeasurementMetrics) {
        for node in consumers {
            self.specs
                .get_mut(node)
                .expect("measurement consumers retain their registered specs")
                .last_ready = Some(metrics.clone());
        }
    }

    fn remove_consumer(&mut self, node: NodeId) {
        for entry in self.cache.values_mut() {
            entry.consumers.remove(&node);
        }
        let mut empty = Vec::new();
        for (key, request) in &mut self.outstanding {
            request.consumers.remove(&node);
            if request.consumers.is_empty() {
                empty.push(*key);
            }
        }
        for key in empty {
            let request = self
                .outstanding
                .remove(&key)
                .expect("collected key remains outstanding");
            self.outstanding_by_cache.remove(&request.cache_key);
        }
    }

    fn cache_key(state: &SpecState, constraints: MeasureConstraints, epoch: u64) -> CacheKey {
        CacheKey {
            element_type: state.element_type,
            kind: state.spec.kind,
            content_hash: state.spec.content_hash,
            style_hash: state.spec.style_hash,
            environment_epoch: epoch,
            constraints: constraints.into(),
        }
    }

    fn use_fallback(
        &mut self,
        policy: PendingMeasurePolicy,
        last_ready: Option<&MeasurementMetrics>,
        provider_provisional: Option<&MeasurementMetrics>,
    ) -> LayoutSize {
        if let Some(metrics) = provider_provisional {
            self.pass.provisional = true;
            return to_layout_size(metrics.size);
        }
        match policy {
            PendingMeasurePolicy::Block => {
                self.pass.blocking = true;
                LayoutSize::default()
            }
            PendingMeasurePolicy::Placeholder(size) => {
                self.pass.provisional = true;
                to_layout_size(size)
            }
            PendingMeasurePolicy::RetainPrevious => {
                if let Some(metrics) = last_ready {
                    self.pass.provisional = true;
                    to_layout_size(metrics.size)
                } else {
                    self.pass.blocking = true;
                    LayoutSize::default()
                }
            }
        }
    }

    fn allocate_key(&mut self) -> Result<MeasurementKey, MeasurementError> {
        let key = MeasurementKey::new(self.next_key).ok_or(MeasurementError::KeyExhausted)?;
        self.next_key = self.next_key.checked_add(1).unwrap_or(0);
        Ok(key)
    }
}

impl IntrinsicMeasurer for MeasurementCoordinator {
    fn measure(&mut self, node: NodeId, constraints: LayoutMeasureRequest) -> LayoutSize {
        let Some(state) = self.specs.get(&node).cloned() else {
            self.pass
                .error
                .get_or_insert(MeasurementError::MissingSpec { node });
            return LayoutSize::default();
        };
        let epoch = self.environment_epoch.unwrap_or(0);
        let cache_key = Self::cache_key(&state, constraints, epoch);

        if let Some(entry) = self.cache.get_mut(&cache_key) {
            entry.consumers.insert(node);
            let cached = entry.state.clone();
            return match cached {
                CachedState::Ready(metrics) => to_layout_size(metrics.size),
                CachedState::Pending {
                    request_id,
                    provisional,
                    ..
                } => {
                    self.pass.pending.insert(request_id);
                    self.use_fallback(
                        state.spec.pending_policy,
                        state.last_ready.as_ref(),
                        provisional.as_ref(),
                    )
                }
                CachedState::Unsupported(reason) => {
                    self.pass
                        .error
                        .get_or_insert(MeasurementError::Unsupported { node, reason });
                    LayoutSize::default()
                }
            };
        }

        if let Some(key) = self.outstanding_by_cache.get(&cache_key).copied() {
            let request = self
                .outstanding
                .get_mut(&key)
                .expect("cache index points to an outstanding request");
            request.consumers.insert(node);
            self.pass.requests.insert(key, request.request.clone());
            return self.use_fallback(state.spec.pending_policy, state.last_ready.as_ref(), None);
        }

        let key = match self.allocate_key() {
            Ok(key) => key,
            Err(error) => {
                self.pass.error.get_or_insert(error);
                return LayoutSize::default();
            }
        };
        let request = MeasurementRequest {
            key,
            node,
            element_type: state.element_type,
            environment_epoch: epoch,
            kind: state.spec.kind,
            constraints,
            payload: state.spec.payload.clone(),
        };
        self.outstanding_by_cache.insert(cache_key.clone(), key);
        self.outstanding.insert(
            key,
            OutstandingRequest {
                request: request.clone(),
                cache_key,
                consumers: BTreeSet::from([node]),
            },
        );
        self.pass.requests.insert(key, request);
        self.use_fallback(state.spec.pending_policy, state.last_ready.as_ref(), None)
    }
}

fn to_layout_size(value: MeasuredSize) -> LayoutSize {
    LayoutSize::new(value.width, value.height)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PassSummary {
    pub(crate) requests: Vec<MeasurementRequest>,
    pub(crate) pending: usize,
    pub(crate) blocking: bool,
    pub(crate) provisional: bool,
}

#[cfg(test)]
mod tests {
    use whisker_protocol::{LayoutRect, PreparedContentId, ProtocolValue};

    use super::*;

    fn node(value: u64) -> NodeId {
        NodeId::new(value).expect("node")
    }

    fn element() -> ElementTypeId {
        ElementTypeId::new(1).expect("element")
    }

    fn constraints(width: f32) -> MeasureConstraints {
        MeasureConstraints {
            known_dimensions: [None, Some(10.0)],
            available_space: [AvailableSpace::Definite(width), AvailableSpace::MaxContent],
        }
    }

    fn spec(kind: MeasurementKind, policy: PendingMeasurePolicy) -> MeasurementSpec {
        MeasurementSpec {
            kind,
            content_hash: 7,
            style_hash: 9,
            payload: ProtocolValue::String("payload".into()),
            pending_policy: policy,
        }
    }

    fn metrics(width: f32, height: f32) -> MeasurementMetrics {
        MeasurementMetrics {
            size: MeasuredSize::new(width, height),
            first_baseline: Some(6.0),
            last_baseline: Some(7.0),
            overflow: Some(LayoutRect {
                width,
                height,
                ..LayoutRect::default()
            }),
            prepared_content: PreparedContentId::new(3),
        }
    }

    #[test]
    fn coordinator_deduplicates_caches_and_invalidates_environment() {
        let mut coordinator = MeasurementCoordinator::default();
        assert!(coordinator.set_environment(1).is_empty());
        assert!(coordinator.set_environment(1).is_empty());
        assert!(
            coordinator
                .set_spec(
                    node(1),
                    element(),
                    Some(spec(
                        MeasurementKind::Text,
                        PendingMeasurePolicy::Placeholder(MeasuredSize::new(2.0, 3.0)),
                    )),
                )
                .unwrap()
        );
        assert!(
            !coordinator
                .set_spec(
                    node(1),
                    element(),
                    Some(spec(
                        MeasurementKind::Text,
                        PendingMeasurePolicy::Placeholder(MeasuredSize::new(2.0, 3.0)),
                    )),
                )
                .unwrap()
        );
        coordinator
            .set_spec(
                node(2),
                element(),
                Some(spec(
                    MeasurementKind::Text,
                    PendingMeasurePolicy::Placeholder(MeasuredSize::new(2.0, 3.0)),
                )),
            )
            .unwrap();

        coordinator.begin_pass();
        assert_eq!(
            coordinator.measure(node(1), constraints(100.0)),
            LayoutSize::new(2.0, 3.0)
        );
        assert_eq!(
            coordinator.measure(node(2), constraints(100.0)),
            LayoutSize::new(2.0, 3.0)
        );
        let pass = coordinator.finish_pass().unwrap();
        assert_eq!(pass.requests.len(), 1);
        assert!(pass.provisional);
        assert!(!pass.blocking);

        let request = pass.requests[0].clone();
        let apply = coordinator
            .apply_batch(&[MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: 1,
                metrics: metrics(20.0, 10.0),
            }])
            .unwrap();
        assert_eq!(apply.applied(), 1);
        assert_eq!(apply.stale(), 0);
        assert_eq!(apply.invalidated_nodes(), [node(1), node(2)]);

        coordinator.begin_pass();
        assert_eq!(
            coordinator.measure(node(1), constraints(100.0)),
            LayoutSize::new(20.0, 10.0)
        );
        let pass = coordinator.finish_pass().unwrap();
        assert!(pass.requests.is_empty());
        assert!(!pass.provisional);
        assert_eq!(coordinator.last_ready(node(1)), Some(&metrics(20.0, 10.0)));
        assert_eq!(coordinator.pending_count(), 0);

        assert_eq!(coordinator.set_environment(2), [node(1), node(2)]);
        coordinator.begin_pass();
        coordinator.measure(node(1), constraints(100.0));
        assert_eq!(coordinator.finish_pass().unwrap().requests.len(), 1);
    }

    #[test]
    fn pending_completion_rejects_stale_and_validates_metrics() {
        let mut coordinator = MeasurementCoordinator::default();
        coordinator.set_environment(4);
        coordinator
            .set_spec(
                node(1),
                element(),
                Some(spec(
                    MeasurementKind::ReplacedContent,
                    PendingMeasurePolicy::Block,
                )),
            )
            .unwrap();
        coordinator.begin_pass();
        assert_eq!(
            coordinator.measure(node(1), constraints(80.0)),
            LayoutSize::default()
        );
        let pass = coordinator.finish_pass().unwrap();
        assert!(pass.blocking);
        let key = pass.requests[0].key;
        let request_id = MeasurementRequestId::new(11).expect("request");
        let apply = coordinator
            .apply_batch(&[MeasurementResponse::Pending {
                key,
                environment_epoch: 4,
                request_id,
                provisional: Some(metrics(8.0, 6.0)),
            }])
            .unwrap();
        assert_eq!(apply.applied(), 1);

        coordinator.begin_pass();
        assert_eq!(
            coordinator.measure(node(1), constraints(80.0)),
            LayoutSize::new(8.0, 6.0)
        );
        let pass = coordinator.finish_pass().unwrap();
        assert_eq!(pass.pending, 1);
        assert!(pass.provisional);

        let wrong = MeasurementReady {
            key,
            request_id,
            environment_epoch: 5,
            metrics: metrics(9.0, 7.0),
        };
        assert_eq!(
            coordinator.apply_ready(&wrong).unwrap(),
            DeferredMeasurementApply::IgnoredStale
        );
        let invalid = MeasurementReady {
            environment_epoch: 4,
            metrics: MeasurementMetrics::from_size(MeasuredSize::new(f32::NAN, 1.0)),
            ..wrong.clone()
        };
        assert_eq!(
            coordinator.apply_ready(&invalid),
            Err(MeasurementError::InvalidMetrics { key })
        );
        let ready = MeasurementReady {
            environment_epoch: 4,
            metrics: metrics(9.0, 7.0),
            ..wrong
        };
        assert_eq!(
            coordinator.apply_ready(&ready).unwrap(),
            DeferredMeasurementApply::Applied {
                invalidated_nodes: vec![node(1)]
            }
        );
        assert_eq!(
            coordinator.apply_ready(&ready).unwrap(),
            DeferredMeasurementApply::IgnoredStale
        );
    }

    #[test]
    fn policies_errors_and_stale_batches_are_explicit() {
        let mut coordinator = MeasurementCoordinator::default();
        coordinator.set_environment(1);
        assert_eq!(
            coordinator.set_spec(
                node(1),
                element(),
                Some(spec(
                    MeasurementKind::NativeControl,
                    PendingMeasurePolicy::Placeholder(MeasuredSize::new(-1.0, 0.0)),
                )),
            ),
            Err(MeasurementError::InvalidPlaceholder { node: node(1) })
        );

        coordinator.begin_pass();
        coordinator.measure(node(1), constraints(10.0));
        assert_eq!(
            coordinator.finish_pass(),
            Err(MeasurementError::MissingSpec { node: node(1) })
        );

        coordinator
            .set_spec(
                node(1),
                element(),
                Some(spec(
                    MeasurementKind::EmbeddedSurface,
                    PendingMeasurePolicy::RetainPrevious,
                )),
            )
            .unwrap();
        coordinator.begin_pass();
        assert_eq!(
            coordinator.measure(node(1), constraints(10.0)),
            LayoutSize::default()
        );
        let request = coordinator.finish_pass().unwrap().requests.remove(0);
        let stale_key = MeasurementKey::new(999).expect("stale");
        let apply = coordinator
            .apply_batch(&[
                MeasurementResponse::Ready {
                    key: stale_key,
                    environment_epoch: 1,
                    metrics: metrics(1.0, 1.0),
                },
                MeasurementResponse::Ready {
                    key: request.key,
                    environment_epoch: 2,
                    metrics: metrics(1.0, 1.0),
                },
            ])
            .unwrap();
        assert_eq!(apply.stale(), 2);

        assert_eq!(
            coordinator.apply_batch(&[MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: 1,
                metrics: MeasurementMetrics::from_size(MeasuredSize::new(-1.0, 1.0)),
            }]),
            Err(MeasurementError::InvalidMetrics { key: request.key })
        );
        assert!(coordinator.outstanding.contains_key(&request.key));

        coordinator
            .apply_batch(&[MeasurementResponse::Unsupported {
                key: request.key,
                environment_epoch: 1,
                reason: UnsupportedMeasurementReason::Element,
            }])
            .unwrap();
        coordinator.begin_pass();
        coordinator.measure(node(1), constraints(10.0));
        assert_eq!(
            coordinator.finish_pass(),
            Err(MeasurementError::Unsupported {
                node: node(1),
                reason: UnsupportedMeasurementReason::Element,
            })
        );
        assert!(coordinator.set_spec(node(1), element(), None).unwrap());
        assert!(!coordinator.set_spec(node(1), element(), None).unwrap());
        coordinator.remove_node(node(1));
    }

    #[test]
    fn duplicate_pending_ids_are_transactionally_rejected() {
        let mut coordinator = MeasurementCoordinator::default();
        coordinator.set_environment(1);
        for value in 1..=2 {
            let mut value_spec = spec(
                MeasurementKind::Custom { version: 1 },
                PendingMeasurePolicy::Block,
            );
            value_spec.content_hash = value;
            coordinator
                .set_spec(node(value), element(), Some(value_spec))
                .unwrap();
        }
        coordinator.begin_pass();
        coordinator.measure(node(1), constraints(10.0));
        coordinator.measure(node(2), constraints(10.0));
        let requests = coordinator.finish_pass().unwrap().requests;
        let request_id = MeasurementRequestId::new(7).expect("request");
        let responses = requests
            .iter()
            .map(|request| MeasurementResponse::Pending {
                key: request.key,
                environment_epoch: 1,
                request_id,
                provisional: None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            coordinator.apply_batch(&responses),
            Err(MeasurementError::DuplicateRequestId { request_id })
        );
        assert!(coordinator.cache.is_empty());
        assert_eq!(coordinator.outstanding.len(), 2);
    }

    #[test]
    fn retained_previous_metrics_and_key_exhaustion_are_covered() {
        let mut coordinator = MeasurementCoordinator::default();
        coordinator.set_environment(1);
        coordinator
            .set_spec(
                node(1),
                element(),
                Some(spec(
                    MeasurementKind::Text,
                    PendingMeasurePolicy::RetainPrevious,
                )),
            )
            .unwrap();
        coordinator.begin_pass();
        coordinator.measure(node(1), constraints(10.0));
        let request = coordinator.finish_pass().unwrap().requests.remove(0);
        coordinator
            .apply_batch(&[MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: 1,
                metrics: metrics(5.0, 4.0),
            }])
            .unwrap();

        let mut changed = spec(MeasurementKind::Text, PendingMeasurePolicy::RetainPrevious);
        changed.content_hash = 99;
        coordinator
            .set_spec(node(1), element(), Some(changed))
            .unwrap();
        coordinator.begin_pass();
        assert_eq!(
            coordinator.measure(node(1), constraints(10.0)),
            LayoutSize::new(5.0, 4.0)
        );
        assert!(coordinator.finish_pass().unwrap().provisional);

        let mut exhausted = MeasurementCoordinator::default();
        exhausted.set_environment(1);
        exhausted.next_key = 0;
        exhausted
            .set_spec(
                node(1),
                element(),
                Some(spec(MeasurementKind::Text, PendingMeasurePolicy::Block)),
            )
            .unwrap();
        exhausted.begin_pass();
        exhausted.measure(node(1), constraints(10.0));
        assert_eq!(exhausted.finish_pass(), Err(MeasurementError::KeyExhausted));
    }

    #[test]
    fn defensive_stale_entries_and_outstanding_cleanup_are_covered() {
        assert_eq!(
            AvailableSpaceKey::from(AvailableSpace::MinContent),
            AvailableSpaceKey::MinContent
        );

        fn pending_coordinator() -> (MeasurementCoordinator, MeasurementReady, CacheKey) {
            let mut coordinator = MeasurementCoordinator::default();
            coordinator.set_environment(1);
            coordinator
                .set_spec(
                    node(1),
                    element(),
                    Some(spec(MeasurementKind::Text, PendingMeasurePolicy::Block)),
                )
                .unwrap();
            coordinator.begin_pass();
            coordinator.measure(node(1), constraints(10.0));
            let key = coordinator.finish_pass().unwrap().requests[0].key;
            let request_id = MeasurementRequestId::new(21).expect("request");
            coordinator
                .apply_batch(&[MeasurementResponse::Pending {
                    key,
                    environment_epoch: 1,
                    request_id,
                    provisional: None,
                }])
                .unwrap();
            let cache_key = coordinator
                .pending
                .get(&request_id)
                .expect("pending")
                .cache_key
                .clone();
            (
                coordinator,
                MeasurementReady {
                    key,
                    request_id,
                    environment_epoch: 1,
                    metrics: metrics(2.0, 2.0),
                },
                cache_key,
            )
        }

        let (mut missing_cache, ready, cache_key) = pending_coordinator();
        missing_cache.cache.remove(&cache_key);
        assert_eq!(
            missing_cache.apply_ready(&ready).unwrap(),
            DeferredMeasurementApply::IgnoredStale
        );

        let (mut wrong_state, ready, cache_key) = pending_coordinator();
        wrong_state.cache.get_mut(&cache_key).expect("cache").state =
            CachedState::Ready(metrics(1.0, 1.0));
        assert_eq!(
            wrong_state.apply_ready(&ready).unwrap(),
            DeferredMeasurementApply::IgnoredStale
        );

        let (mut no_consumers, _, _) = pending_coordinator();
        no_consumers.set_spec(node(1), element(), None).unwrap();
        assert_eq!(no_consumers.pending_count(), 0);

        let mut outstanding = MeasurementCoordinator::default();
        outstanding.set_environment(1);
        for value in 1..=2 {
            outstanding
                .set_spec(
                    node(value),
                    element(),
                    Some(spec(MeasurementKind::Text, PendingMeasurePolicy::Block)),
                )
                .unwrap();
        }
        outstanding.begin_pass();
        outstanding.measure(node(1), constraints(10.0));
        outstanding.measure(node(2), constraints(10.0));
        assert_eq!(outstanding.outstanding.len(), 1);
        outstanding.set_spec(node(1), element(), None).unwrap();
        assert_eq!(outstanding.outstanding.len(), 1);
        outstanding.set_spec(node(2), element(), None).unwrap();
        assert!(outstanding.outstanding.is_empty());
        assert!(outstanding.outstanding_by_cache.is_empty());

        let mut invalid_pending = MeasurementCoordinator::default();
        invalid_pending.set_environment(1);
        invalid_pending
            .set_spec(
                node(1),
                element(),
                Some(spec(MeasurementKind::Text, PendingMeasurePolicy::Block)),
            )
            .unwrap();
        invalid_pending.begin_pass();
        invalid_pending.measure(node(1), constraints(10.0));
        let key = invalid_pending.finish_pass().unwrap().requests[0].key;
        assert_eq!(
            invalid_pending.apply_batch(&[MeasurementResponse::Pending {
                key,
                environment_epoch: 1,
                request_id: MeasurementRequestId::new(22).expect("request"),
                provisional: Some(MeasurementMetrics::from_size(MeasuredSize::new(
                    f32::NAN,
                    1.0,
                ))),
            }]),
            Err(MeasurementError::InvalidMetrics { key })
        );
    }
}
