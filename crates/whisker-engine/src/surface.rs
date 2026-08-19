//! Surface-level orchestration of retained scene and layout state.

use std::{error::Error, fmt};

use whisker_layout::{IntrinsicMeasurer, LayoutError, LayoutSize, LayoutSnapshot, LayoutTree};
use whisker_protocol::{
    ElementTypeId, FramePacket, LayoutRect, MeasurementMetrics, MeasurementReady,
    MeasurementResponse, MeasurementSpec, NodeId, SurfaceId,
};
use whisker_style::{ComputedLayoutStyle, PropertyImpactSet};

use crate::{
    DeferredMeasurementApply, LayoutProgress, MeasurementApply, MeasurementError, Scene,
    SceneError, SceneNode, measurement::MeasurementCoordinator,
};

/// Result of requesting one layout pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutUpdate {
    recomputed: bool,
    changed_nodes: usize,
}

impl LayoutUpdate {
    fn computed(changed_nodes: usize) -> Self {
        Self {
            recomputed: true,
            changed_nodes,
        }
    }

    /// Returns whether Taffy ran for this request.
    pub const fn recomputed(self) -> bool {
        self.recomputed
    }

    /// Returns how many scene nodes received changed geometry.
    pub const fn changed_nodes(self) -> usize {
        self.changed_nodes
    }
}

/// Failure while coordinating retained layout and scene state.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceError {
    /// The retained scene rejected an operation.
    Scene(SceneError),
    /// The retained layout tree rejected an operation or computation.
    Layout(LayoutError),
    /// A layout snapshot named a node absent from the paired scene.
    SceneLayoutMismatch {
        /// Node present only in the layout result.
        node: NodeId,
    },
    /// A layout backend produced non-finite geometry.
    InvalidLayoutOutput {
        /// Node whose rectangle was invalid.
        node: NodeId,
    },
    /// Intrinsic measurement state or a Host response was invalid.
    Measurement(MeasurementError),
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker surface error: {self:?}")
    }
}

impl Error for SurfaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Scene(error) => Some(error),
            Self::Layout(error) => Some(error),
            Self::Measurement(error) => Some(error),
            Self::SceneLayoutMismatch { .. } | Self::InvalidLayoutOutput { .. } => None,
        }
    }
}

impl From<SceneError> for SurfaceError {
    fn from(error: SceneError) -> Self {
        Self::Scene(error)
    }
}

impl From<LayoutError> for SurfaceError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<MeasurementError> for SurfaceError {
    fn from(error: MeasurementError) -> Self {
        Self::Measurement(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LayoutInputs {
    root: NodeId,
    viewport: LayoutSize,
}

/// Host-independent rendering pipeline for one Whisker surface.
///
/// This is the sole mutable owner of the paired [`Scene`] and [`LayoutTree`].
/// Structural operations update both retained trees, while a successful layout
/// pass projects only changed rectangles into the scene journal.
#[derive(Clone, Debug)]
pub struct SurfaceEngine {
    scene: Scene,
    layout: LayoutTree,
    last_layout: Option<LayoutSnapshot>,
    last_inputs: Option<LayoutInputs>,
    layout_dirty: bool,
    layout_provisional: bool,
    measurements: MeasurementCoordinator,
}

impl SurfaceEngine {
    /// Creates an empty surface pipeline.
    pub fn new(surface: SurfaceId) -> Self {
        Self {
            scene: Scene::new(surface),
            layout: LayoutTree::new(),
            last_layout: None,
            last_inputs: None,
            layout_dirty: false,
            layout_provisional: false,
            measurements: MeasurementCoordinator::default(),
        }
    }

    /// Returns this pipeline's surface identifier.
    pub const fn surface(&self) -> SurfaceId {
        self.scene.surface()
    }

    /// Returns the retained scene without allowing structural mutation.
    pub const fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Returns the retained layout tree without allowing structural mutation.
    pub const fn layout_tree(&self) -> &LayoutTree {
        &self.layout
    }

    /// Returns the most recent successful layout snapshot.
    pub const fn last_layout(&self) -> Option<&LayoutSnapshot> {
        self.last_layout.as_ref()
    }

    /// Returns whether retained inputs require another layout pass.
    pub const fn needs_layout(&self) -> bool {
        self.layout_dirty
    }

    /// Returns whether the scene has a snapshot, mutation, command, or retry.
    pub fn has_pending_work(&self) -> bool {
        self.scene.has_pending_work()
    }

    /// Returns a retained scene node when it is live.
    pub fn node(&self, node: NodeId) -> Option<&SceneNode> {
        self.scene.node(node)
    }

    /// Creates one unattached node in both retained trees.
    pub fn create_node(
        &mut self,
        element_type: ElementTypeId,
        style: ComputedLayoutStyle,
    ) -> Result<NodeId, SurfaceError> {
        LayoutTree::validate_style(&style)?;
        let node = self.scene.create_node(element_type)?;
        self.layout
            .create_node(node, style)
            .expect("a validated style and fresh scene ID must enter layout");
        self.layout_dirty = true;
        Ok(node)
    }

    /// Deletes a node and its complete subtree from both retained trees.
    pub fn delete_node(&mut self, node: NodeId) -> Result<(), SurfaceError> {
        let mut removed = Vec::new();
        collect_scene_subtree(&self.scene, node, &mut removed)?;
        self.scene.delete_node(node)?;
        self.layout
            .remove_subtree(node)
            .expect("scene and layout trees remain structurally synchronized");
        for removed in removed {
            self.measurements.remove_node(removed);
        }
        self.layout_dirty = true;
        Ok(())
    }

    /// Attaches an unattached child at an index in both retained trees.
    pub fn insert_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: u32,
    ) -> Result<(), SurfaceError> {
        self.scene.insert_child(parent, child, index)?;
        self.layout
            .reparent(child, parent, index as usize)
            .expect("scene and layout trees remain structurally synchronized");
        self.layout_dirty = true;
        Ok(())
    }

    /// Detaches a direct child without deleting it.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), SurfaceError> {
        self.scene.remove_child(parent, child)?;
        let children = self
            .scene
            .node(parent)
            .expect("validated parent remains live")
            .children()
            .to_vec();
        self.layout
            .set_children(parent, &children)
            .expect("scene and layout trees remain structurally synchronized");
        self.layout_dirty = true;
        Ok(())
    }

    /// Moves a direct child within its current parent.
    pub fn move_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: u32,
    ) -> Result<(), SurfaceError> {
        self.scene.move_child(parent, child, index)?;
        self.layout
            .reparent(child, parent, index as usize)
            .expect("scene and layout trees remain structurally synchronized");
        self.layout_dirty = true;
        Ok(())
    }

    /// Replaces a node's computed layout input and reports its impact.
    pub fn update_layout_style(
        &mut self,
        node: NodeId,
        style: ComputedLayoutStyle,
    ) -> Result<PropertyImpactSet, SurfaceError> {
        self.ensure_mutable()?;
        let impact = self.layout.update_style(node, style)?;
        if impact.contains(PropertyImpactSet::LAYOUT) {
            self.layout_dirty = true;
        }
        Ok(impact)
    }

    /// Marks or unmarks a leaf as requiring intrinsic Host measurement.
    ///
    /// Returns whether retained measurement behavior changed.
    pub fn set_measurable(&mut self, node: NodeId, measurable: bool) -> Result<bool, SurfaceError> {
        self.ensure_mutable()?;
        let changed = self.layout.set_measurable(node, measurable)?;
        self.layout_dirty |= changed;
        Ok(changed)
    }

    /// Registers or clears Host-backed intrinsic measurement for one leaf.
    ///
    /// Element modules provide a semantic kind, complete content/style hashes,
    /// a versioned payload, and an explicit pending policy. Ordinary boxes and
    /// explicitly sized media should leave this unset.
    pub fn set_measurement(
        &mut self,
        node: NodeId,
        spec: Option<MeasurementSpec>,
    ) -> Result<bool, SurfaceError> {
        self.ensure_mutable()?;
        let element_type = self
            .scene
            .node(node)
            .ok_or(SceneError::UnknownNode { node })?
            .element_type();
        if !self.layout.contains(node) {
            return Err(LayoutError::UnknownNode(node).into());
        }
        let measurable = spec.is_some();
        let spec_changed = self.measurements.set_spec(node, element_type, spec)?;
        let behavior_changed = self
            .layout
            .set_measurable(node, measurable)
            .expect("scene-validated node remains in synchronized layout");
        let changed = spec_changed || behavior_changed;
        if changed {
            self.layout
                .invalidate_measurement(node)
                .expect("configured measurement node remains in synchronized layout");
            self.layout_dirty = true;
        }
        Ok(changed)
    }

    /// Returns the last final Host metrics retained for a node.
    ///
    /// Baselines, overflow, and prepared-content handles remain available for
    /// later text and paint lowering even though Taffy consumes only the size.
    pub fn last_measurement(&self, node: NodeId) -> Option<&MeasurementMetrics> {
        self.measurements.last_ready(node)
    }

    /// Applies invalidation categories for a changed node property.
    ///
    /// Paint-only changes do not schedule Taffy. Intrinsic-measure changes
    /// invalidate the layout backend's cached measurement before scheduling.
    /// Returns whether a layout pass is required by the supplied impacts.
    pub fn invalidate_node(
        &mut self,
        node: NodeId,
        impacts: PropertyImpactSet,
    ) -> Result<bool, SurfaceError> {
        self.ensure_mutable()?;
        if self.scene.node(node).is_none() {
            return Err(SceneError::UnknownNode { node }.into());
        }
        let intrinsic = impacts.contains(PropertyImpactSet::INTRINSIC_MEASURE);
        if intrinsic {
            self.layout.invalidate_measurement(node)?;
        }
        let requires_layout = intrinsic || impacts.contains(PropertyImpactSet::LAYOUT);
        self.layout_dirty |= requires_layout;
        Ok(requires_layout)
    }

    /// Computes layout when inputs are dirty and journals changed rectangles.
    pub fn compute_layout(
        &mut self,
        root: NodeId,
        viewport: LayoutSize,
        measurer: &mut dyn IntrinsicMeasurer,
    ) -> Result<LayoutUpdate, SurfaceError> {
        self.ensure_mutable()?;
        let inputs = LayoutInputs { root, viewport };
        if !self.layout_dirty && self.last_inputs == Some(inputs) {
            return Ok(LayoutUpdate::default());
        }

        let snapshot = self.layout.compute(root, viewport, measurer)?;
        let update = self.project_layout(snapshot, inputs)?;
        self.layout_provisional = false;
        Ok(update)
    }

    /// Advances layout using retained, batched Host intrinsic measurement.
    ///
    /// A blocked result must not be presented. Provisional geometry is emitted
    /// only when the element schema or provider supplied an explicit fallback.
    /// Reinvoke after applying immediate or deferred responses until complete.
    pub fn compute_layout_with_measurements(
        &mut self,
        root: NodeId,
        viewport: LayoutSize,
        environment_epoch: u64,
    ) -> Result<LayoutProgress, SurfaceError> {
        self.ensure_mutable()?;
        for node in self.measurements.set_environment(environment_epoch) {
            self.layout
                .invalidate_measurement(node)
                .expect("measurement specs remain synchronized with layout nodes");
            self.layout_dirty = true;
        }
        let inputs = LayoutInputs { root, viewport };
        if !self.layout_dirty && self.last_inputs == Some(inputs) {
            return if self.layout_provisional {
                Ok(LayoutProgress::Provisional {
                    update: LayoutUpdate::default(),
                    requests: self.measurements.outstanding_requests(),
                    pending: self.measurements.pending_count(),
                })
            } else {
                Ok(LayoutProgress::Complete(LayoutUpdate::default()))
            };
        }

        for node in self.measurements.unresolved_nodes() {
            self.layout
                .invalidate_measurement(node)
                .expect("measurement consumers remain synchronized with layout nodes");
        }

        self.measurements.begin_pass();
        let snapshot = self
            .layout
            .compute(root, viewport, &mut self.measurements)?;
        let pass = self.measurements.finish_pass()?;
        if pass.blocking {
            self.layout_dirty = true;
            return Ok(LayoutProgress::Blocked {
                requests: pass.requests,
                pending: pass.pending,
            });
        }

        let update = self.project_layout(snapshot, inputs)?;
        if pass.provisional || !pass.requests.is_empty() || pass.pending != 0 {
            self.layout_provisional = true;
            Ok(LayoutProgress::Provisional {
                update,
                requests: pass.requests,
                pending: pass.pending,
            })
        } else {
            self.layout_provisional = false;
            Ok(LayoutProgress::Complete(update))
        }
    }

    /// Applies synchronous results from one Host measurement batch.
    ///
    /// Unknown and wrong-epoch responses are counted as stale and ignored.
    /// Accepted results invalidate only their current Taffy consumers.
    pub fn apply_measurement_responses(
        &mut self,
        responses: &[MeasurementResponse],
    ) -> Result<MeasurementApply, SurfaceError> {
        self.ensure_mutable()?;
        let apply = self.measurements.apply_batch(responses)?;
        for node in apply.invalidated_nodes() {
            self.layout
                .invalidate_measurement(*node)
                .expect("response consumers remain synchronized with layout nodes");
        }
        self.layout_dirty |= !apply.invalidated_nodes().is_empty();
        Ok(apply)
    }

    /// Applies a deferred Host-to-Rust measurement completion.
    pub fn apply_measurement_ready(
        &mut self,
        ready: &MeasurementReady,
    ) -> Result<DeferredMeasurementApply, SurfaceError> {
        self.ensure_mutable()?;
        let apply = self.measurements.apply_ready(ready)?;
        for node in apply.invalidated_nodes() {
            self.layout
                .invalidate_measurement(*node)
                .expect("deferred consumers remain synchronized with layout nodes");
        }
        self.layout_dirty |= !apply.invalidated_nodes().is_empty();
        Ok(apply)
    }

    /// Prepares the next scene snapshot or delta frame.
    pub fn prepare_frame(
        &mut self,
        viewport_epoch: u32,
    ) -> Result<Option<&FramePacket>, SurfaceError> {
        self.scene.prepare_frame(viewport_epoch).map_err(Into::into)
    }

    /// Commits the prepared frame after renderer acceptance.
    pub fn accept_pending(&mut self, revision: u64) -> Result<(), SurfaceError> {
        self.scene.accept_pending(revision).map_err(Into::into)
    }

    /// Discards the prepared frame while retaining its semantic changes.
    pub fn discard_pending(&mut self) -> Result<(), SurfaceError> {
        self.scene.discard_pending().map_err(Into::into)
    }

    /// Requires the next frame to rebuild the receiver from a full snapshot.
    pub fn require_snapshot(&mut self) -> Result<(), SurfaceError> {
        self.scene.require_snapshot().map_err(Into::into)
    }

    fn ensure_mutable(&self) -> Result<(), SurfaceError> {
        if self.scene.has_prepared_frame() {
            Err(SceneError::FramePending.into())
        } else {
            Ok(())
        }
    }

    fn project_layout(
        &mut self,
        snapshot: LayoutSnapshot,
        inputs: LayoutInputs,
    ) -> Result<LayoutUpdate, SurfaceError> {
        let entries = snapshot
            .iter()
            .map(|(node, rect)| (node, *rect))
            .collect::<Vec<_>>();
        validate_layout_entries(&self.scene, &entries)?;
        let changed = entries
            .iter()
            .copied()
            .filter(|(node, rect)| {
                self.last_layout
                    .as_ref()
                    .and_then(|previous| previous.get(*node))
                    != Some(rect)
            })
            .collect::<Vec<_>>();
        for (node, rect) in &changed {
            self.scene
                .set_layout(*node, *rect)
                .expect("validated layout projection must enter a mutable synchronized scene");
        }
        self.last_layout = Some(snapshot);
        self.last_inputs = Some(inputs);
        self.layout_dirty = false;
        Ok(LayoutUpdate::computed(changed.len()))
    }
}

fn collect_scene_subtree(
    scene: &Scene,
    node: NodeId,
    output: &mut Vec<NodeId>,
) -> Result<(), SurfaceError> {
    let retained = scene.node(node).ok_or(SceneError::UnknownNode { node })?;
    output.push(node);
    for child in retained.children() {
        collect_scene_subtree(scene, *child, output)
            .expect("retained scene children remain live while their parent is live");
    }
    Ok(())
}

fn validate_layout_entries(
    scene: &Scene,
    entries: &[(NodeId, LayoutRect)],
) -> Result<(), SurfaceError> {
    for (node, rect) in entries {
        if scene.node(*node).is_none() {
            return Err(SurfaceError::SceneLayoutMismatch { node: *node });
        }
        if ![rect.x, rect.y, rect.width, rect.height]
            .into_iter()
            .all(f32::is_finite)
        {
            return Err(SurfaceError::InvalidLayoutOutput { node: *node });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use whisker_layout::{AvailableSpace, LayoutSize, MeasureRequest, UnsupportedLayoutFeature};
    use whisker_protocol::{
        ApplyResult, ElementTypeId, FrameMode, MeasuredSize, MeasurementKey, MeasurementKind,
        MeasurementMetrics, MeasurementReady, MeasurementRequestId, MeasurementResponse,
        MeasurementSpec, NodeId, Operation, PendingMeasurePolicy, ProtocolValue, SurfaceId,
        UnsupportedMeasurementReason,
    };
    use whisker_style::{Axes, ComputedLengthPercentage, ComputedSizeValue, PositionValue};

    use super::*;
    use crate::{FrameSink, RecordingRenderer};

    fn surface_id() -> SurfaceId {
        SurfaceId::new(1).expect("test surface")
    }

    fn element_type() -> ElementTypeId {
        ElementTypeId::new(1).expect("test element type")
    }

    fn node_id(value: u64) -> NodeId {
        NodeId::new(value).expect("test node")
    }

    fn sized(width: f32, height: f32) -> ComputedLayoutStyle {
        ComputedLayoutStyle {
            size: Axes {
                width: ComputedSizeValue::Value(ComputedLengthPercentage::new(width, 0.0)),
                height: ComputedSizeValue::Value(ComputedLengthPercentage::new(height, 0.0)),
            },
            ..ComputedLayoutStyle::default()
        }
    }

    fn zero_measure(_: NodeId, _: MeasureRequest) -> LayoutSize {
        LayoutSize::default()
    }

    fn measurement_spec(
        kind: MeasurementKind,
        pending_policy: PendingMeasurePolicy,
    ) -> MeasurementSpec {
        MeasurementSpec {
            kind,
            content_hash: 1,
            style_hash: 2,
            payload: ProtocolValue::Null,
            pending_policy,
        }
    }

    fn measurement_metrics(width: f32, height: f32) -> MeasurementMetrics {
        MeasurementMetrics::from_size(MeasuredSize::new(width, height))
    }

    fn present_and_accept(
        surface: &mut SurfaceEngine,
        renderer: &mut RecordingRenderer,
        viewport_epoch: u32,
    ) -> FramePacket {
        let packet = surface
            .prepare_frame(viewport_epoch)
            .expect("prepare frame")
            .expect("pending work")
            .clone();
        let revision = packet.header.target_revision;
        assert_eq!(
            renderer.present(&packet),
            Ok(ApplyResult::Accepted { revision })
        );
        surface.accept_pending(revision).expect("accept frame");
        packet
    }

    fn layout_operation_count(packet: &FramePacket) -> usize {
        packet
            .operations
            .iter()
            .filter(|operation| matches!(operation, Operation::SetLayout { .. }))
            .count()
    }

    #[test]
    fn layout_snapshot_projects_only_changed_geometry_into_frames() {
        let mut surface = SurfaceEngine::new(surface_id());
        assert_eq!(surface.surface(), surface_id());
        assert!(surface.scene().has_pending_work());
        assert!(surface.has_pending_work());
        assert!(surface.layout_tree().is_empty());
        assert_eq!(surface.last_layout(), None);
        assert!(!surface.needs_layout());
        assert_eq!(LayoutUpdate::default().changed_nodes(), 0);
        assert!(!LayoutUpdate::default().recomputed());
        assert_eq!(
            zero_measure(
                node_id(1),
                MeasureRequest {
                    known_dimensions: [None, None],
                    available_space: [AvailableSpace::MaxContent; 2],
                },
            ),
            LayoutSize::default()
        );

        let root = surface
            .create_node(element_type(), sized(100.0, 20.0))
            .unwrap();
        let left = surface
            .create_node(element_type(), sized(10.0, 10.0))
            .unwrap();
        let right = surface
            .create_node(element_type(), sized(10.0, 10.0))
            .unwrap();
        surface.insert_child(root, left, 0).unwrap();
        surface.insert_child(root, right, 1).unwrap();
        assert_eq!(surface.node(root).unwrap().children(), [left, right]);
        assert_eq!(surface.layout_tree().len(), 3);
        assert!(surface.needs_layout());

        let first = surface
            .compute_layout(root, LayoutSize::new(100.0, 20.0), &mut zero_measure)
            .unwrap();
        assert!(first.recomputed());
        assert_eq!(first.changed_nodes(), 3);
        assert!(!surface.needs_layout());
        assert_eq!(surface.last_layout().unwrap().len(), 3);

        let mut renderer = RecordingRenderer::new(surface_id());
        let snapshot = present_and_accept(&mut surface, &mut renderer, 1);
        assert_eq!(snapshot.header.mode, FrameMode::Snapshot);
        assert_eq!(layout_operation_count(&snapshot), 3);
        assert!(!surface.has_pending_work());

        let idle = surface
            .compute_layout(root, LayoutSize::new(100.0, 20.0), &mut zero_measure)
            .unwrap();
        assert!(!idle.recomputed());
        assert_eq!(idle.changed_nodes(), 0);
        assert!(surface.prepare_frame(2).unwrap().is_none());

        let impact = surface
            .update_layout_style(left, sized(20.0, 10.0))
            .unwrap();
        assert!(impact.contains(PropertyImpactSet::LAYOUT));
        let changed = surface
            .compute_layout(root, LayoutSize::new(100.0, 20.0), &mut zero_measure)
            .unwrap();
        assert!(changed.recomputed());
        assert_eq!(changed.changed_nodes(), 2);
        let delta = present_and_accept(&mut surface, &mut renderer, 2);
        assert_eq!(delta.header.mode, FrameMode::Delta);
        assert_eq!(layout_operation_count(&delta), 2);
    }

    #[test]
    fn structural_operations_keep_scene_and_layout_in_lockstep() {
        let mut surface = SurfaceEngine::new(surface_id());
        let unsupported = ComputedLayoutStyle {
            position: PositionValue::Fixed,
            ..ComputedLayoutStyle::default()
        };
        assert_eq!(
            surface.create_node(element_type(), unsupported),
            Err(SurfaceError::Layout(LayoutError::UnsupportedStyle(
                UnsupportedLayoutFeature::FixedPosition
            )))
        );
        assert_eq!(surface.scene().node_count(), 0);
        assert!(surface.layout_tree().is_empty());

        let root = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        assert_eq!(root, node_id(1));
        let first = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        let second = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        let grandchild = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        let unknown = node_id(99);

        assert_eq!(
            surface.insert_child(root, unknown, 0),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: unknown
            }))
        );
        assert_eq!(
            surface.remove_child(root, second),
            Err(SurfaceError::Scene(SceneError::NotDirectChild {
                parent: root,
                child: second,
            }))
        );
        surface.insert_child(root, first, 0).unwrap();
        surface.insert_child(root, second, 1).unwrap();
        surface.insert_child(first, grandchild, 0).unwrap();
        assert_eq!(
            surface.move_child(root, second, 2),
            Err(SurfaceError::Scene(SceneError::ChildIndexOutOfBounds {
                parent: root,
                index: 2,
                len: 1,
            }))
        );
        surface.move_child(root, second, 0).unwrap();
        assert_eq!(surface.node(root).unwrap().children(), [second, first]);
        surface.remove_child(root, second).unwrap();
        assert_eq!(surface.node(second).unwrap().parent(), None);
        surface.insert_child(first, second, 1).unwrap();
        assert_eq!(
            surface.node(first).unwrap().children(),
            [grandchild, second]
        );

        surface.delete_node(first).unwrap();
        assert_eq!(surface.scene().node_count(), 1);
        assert_eq!(surface.layout_tree().len(), 1);
        assert!(!surface.layout_tree().contains(first));
        assert!(!surface.layout_tree().contains(second));
        assert!(!surface.layout_tree().contains(grandchild));
        assert_eq!(
            surface.delete_node(unknown),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: unknown
            }))
        );
        assert_eq!(
            surface.set_measurable(unknown, true),
            Err(SurfaceError::Layout(LayoutError::UnknownNode(unknown)))
        );
    }

    #[test]
    fn measurement_and_impact_invalidation_schedule_only_required_layout() {
        let mut surface = SurfaceEngine::new(surface_id());
        let root = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        assert!(surface.set_measurable(root, true).unwrap());
        assert!(!surface.set_measurable(root, true).unwrap());

        let calls = Cell::new(0);
        let mut measure = |_: NodeId, _: MeasureRequest| {
            calls.set(calls.get() + 1);
            LayoutSize::new(12.0, 8.0)
        };
        let first = surface
            .compute_layout(root, LayoutSize::new(100.0, 100.0), &mut measure)
            .unwrap();
        assert_eq!(first.changed_nodes(), 1);
        assert_eq!(calls.get(), 1);
        let mut renderer = RecordingRenderer::new(surface_id());
        present_and_accept(&mut surface, &mut renderer, 1);

        assert!(
            !surface
                .invalidate_node(root, PropertyImpactSet::PAINT)
                .unwrap()
        );
        assert!(!surface.needs_layout());
        let skipped = surface
            .compute_layout(root, LayoutSize::new(100.0, 100.0), &mut measure)
            .unwrap();
        assert!(!skipped.recomputed());
        assert_eq!(calls.get(), 1);

        assert!(
            surface
                .invalidate_node(root, PropertyImpactSet::TEXT_METRICS)
                .unwrap()
        );
        let measured = surface
            .compute_layout(root, LayoutSize::new(100.0, 100.0), &mut measure)
            .unwrap();
        assert!(measured.recomputed());
        assert_eq!(measured.changed_nodes(), 0);
        assert_eq!(calls.get(), 2);
        assert!(surface.prepare_frame(2).unwrap().is_none());

        assert_eq!(
            surface.invalidate_node(node_id(99), PropertyImpactSet::LAYOUT),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: node_id(99)
            }))
        );
        surface
            .invalidate_node(root, PropertyImpactSet::TEXT_METRICS)
            .unwrap();
        assert_eq!(
            surface.compute_layout(root, LayoutSize::new(100.0, 100.0), &mut |_, _| {
                LayoutSize::new(f32::NAN, 1.0)
            },),
            Err(SurfaceError::Layout(LayoutError::InvalidMeasurement(root)))
        );
        assert!(surface.needs_layout());
        surface
            .compute_layout(root, LayoutSize::new(100.0, 100.0), &mut measure)
            .unwrap();
        assert_eq!(calls.get(), 3);

        let equal = surface
            .update_layout_style(root, ComputedLayoutStyle::default())
            .unwrap();
        assert!(equal.is_empty());
        assert!(!surface.needs_layout());
        let viewport_change = surface
            .compute_layout(root, LayoutSize::new(200.0, 100.0), &mut measure)
            .unwrap();
        assert!(viewport_change.recomputed());

        let unsupported = ComputedLayoutStyle {
            position: PositionValue::Sticky,
            ..ComputedLayoutStyle::default()
        };
        assert_eq!(
            surface.update_layout_style(root, unsupported),
            Err(SurfaceError::Layout(LayoutError::UnsupportedStyle(
                UnsupportedLayoutFeature::StickyPosition
            )))
        );
        assert!(!surface.needs_layout());
        assert!(surface.set_measurable(root, false).unwrap());
    }

    #[test]
    fn host_measurement_blocks_batches_caches_and_completes_layout() {
        let mut surface = SurfaceEngine::new(surface_id());
        let root = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        assert_eq!(surface.last_measurement(root), None);
        assert_eq!(
            surface.set_measurement(node_id(99), None),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: node_id(99)
            }))
        );
        assert!(
            surface
                .set_measurement(
                    root,
                    Some(measurement_spec(
                        MeasurementKind::Text,
                        PendingMeasurePolicy::Block,
                    )),
                )
                .unwrap()
        );
        assert!(
            !surface
                .set_measurement(
                    root,
                    Some(measurement_spec(
                        MeasurementKind::Text,
                        PendingMeasurePolicy::Block,
                    )),
                )
                .unwrap()
        );

        let blocked = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 1)
            .unwrap();
        assert!(!blocked.has_layout());
        assert_eq!(blocked.requests().len(), 1);
        assert_eq!(surface.last_layout(), None);
        let request = blocked.requests()[0].clone();

        let repeated = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 1)
            .unwrap();
        assert_eq!(repeated.requests(), std::slice::from_ref(&request));

        let stale = surface
            .apply_measurement_responses(&[MeasurementResponse::Ready {
                key: MeasurementKey::new(99).expect("stale key"),
                environment_epoch: 1,
                metrics: measurement_metrics(1.0, 1.0),
            }])
            .unwrap();
        assert_eq!(stale.applied(), 0);
        assert_eq!(stale.stale(), 1);
        assert!(surface.needs_layout());

        let applied = surface
            .apply_measurement_responses(&[MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: 1,
                metrics: measurement_metrics(24.0, 12.0),
            }])
            .unwrap();
        assert_eq!(applied.applied(), 1);
        let complete = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 1)
            .unwrap();
        assert!(complete.has_layout());
        assert!(complete.requests().is_empty());
        assert_eq!(
            complete,
            LayoutProgress::Complete(LayoutUpdate::computed(1))
        );
        assert_eq!(
            surface.last_measurement(root),
            Some(&measurement_metrics(24.0, 12.0))
        );

        let idle = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 1)
            .unwrap();
        assert_eq!(idle, LayoutProgress::Complete(LayoutUpdate::default()));
        assert!(idle.requests().is_empty());
        assert!(surface.set_measurement(root, None).unwrap());
        assert!(!surface.set_measurement(root, None).unwrap());
        assert_eq!(surface.last_measurement(root), None);
    }

    #[test]
    fn provisional_resource_measurement_relayouts_after_deferred_completion() {
        let mut surface = SurfaceEngine::new(surface_id());
        let root = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        surface
            .set_measurement(
                root,
                Some(measurement_spec(
                    MeasurementKind::ReplacedContent,
                    PendingMeasurePolicy::Placeholder(MeasuredSize::new(16.0, 9.0)),
                )),
            )
            .unwrap();

        let provisional = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 7)
            .unwrap();
        assert!(provisional.has_layout());
        assert_eq!(provisional.requests().len(), 1);
        let request = provisional.requests()[0].clone();
        let idle = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 7)
            .unwrap();
        assert_eq!(idle.requests(), std::slice::from_ref(&request));

        let request_id = MeasurementRequestId::new(8).expect("pending request");
        surface
            .apply_measurement_responses(&[MeasurementResponse::Pending {
                key: request.key,
                environment_epoch: 7,
                request_id,
                provisional: Some(measurement_metrics(20.0, 10.0)),
            }])
            .unwrap();
        let pending = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 7)
            .unwrap();
        assert_eq!(
            pending,
            LayoutProgress::Provisional {
                update: LayoutUpdate::computed(1),
                requests: Vec::new(),
                pending: 1,
            }
        );

        let idle_pending = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 7)
            .unwrap();
        assert_eq!(
            idle_pending,
            LayoutProgress::Provisional {
                update: LayoutUpdate::default(),
                requests: Vec::new(),
                pending: 1,
            }
        );

        let invalid_ready = MeasurementReady {
            key: request.key,
            request_id,
            environment_epoch: 7,
            metrics: measurement_metrics(f32::NAN, 1.0),
        };
        assert_eq!(
            surface.apply_measurement_ready(&invalid_ready),
            Err(SurfaceError::Measurement(
                MeasurementError::InvalidMetrics { key: request.key }
            ))
        );

        let stale = MeasurementReady {
            key: MeasurementKey::new(request.key.get() + 1).expect("wrong key"),
            request_id,
            environment_epoch: 7,
            metrics: measurement_metrics(32.0, 18.0),
        };
        assert_eq!(
            surface.apply_measurement_ready(&stale).unwrap(),
            DeferredMeasurementApply::IgnoredStale
        );
        assert!(!surface.needs_layout());

        let ready = MeasurementReady {
            key: request.key,
            ..stale
        };
        assert_eq!(
            surface.apply_measurement_ready(&ready).unwrap(),
            DeferredMeasurementApply::Applied {
                invalidated_nodes: vec![root]
            }
        );
        assert!(surface.needs_layout());
        assert_eq!(
            surface
                .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 7)
                .unwrap(),
            LayoutProgress::Complete(LayoutUpdate::computed(1))
        );
        assert_eq!(
            surface.last_measurement(root),
            Some(&measurement_metrics(32.0, 18.0))
        );

        let new_environment = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 8)
            .unwrap();
        assert!(new_environment.has_layout());
        assert_eq!(new_environment.requests().len(), 1);
    }

    #[test]
    fn invalid_and_unsupported_measurement_surface_diagnostics() {
        let mut surface = SurfaceEngine::new(surface_id());
        let root = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        let invalid = measurement_spec(
            MeasurementKind::NativeControl,
            PendingMeasurePolicy::Placeholder(MeasuredSize::new(f32::NAN, 1.0)),
        );
        assert_eq!(
            surface.set_measurement(root, Some(invalid)),
            Err(SurfaceError::Measurement(
                MeasurementError::InvalidPlaceholder { node: root }
            ))
        );

        surface
            .set_measurement(
                root,
                Some(measurement_spec(
                    MeasurementKind::NativeControl,
                    PendingMeasurePolicy::Block,
                )),
            )
            .unwrap();
        let blocked = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 1)
            .unwrap();
        let request = blocked.requests()[0].clone();
        assert_eq!(
            surface.apply_measurement_responses(&[MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: 1,
                metrics: measurement_metrics(f32::NAN, 1.0),
            }]),
            Err(SurfaceError::Measurement(
                MeasurementError::InvalidMetrics { key: request.key }
            ))
        );
        surface
            .apply_measurement_responses(&[MeasurementResponse::Unsupported {
                key: request.key,
                environment_epoch: 1,
                reason: UnsupportedMeasurementReason::Environment,
            }])
            .unwrap();
        let error = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 1)
            .unwrap_err();
        assert_eq!(
            error,
            SurfaceError::Measurement(MeasurementError::Unsupported {
                node: root,
                reason: UnsupportedMeasurementReason::Environment,
            })
        );
        assert!(error.source().is_some());
        assert_eq!(
            format!("{}", MeasurementError::KeyExhausted),
            "Whisker measurement error: KeyExhausted"
        );
    }

    #[test]
    fn prepared_frame_blocks_every_layout_mutation_until_resolved() {
        let mut surface = SurfaceEngine::new(surface_id());
        let root = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        let pending = surface.prepare_frame(1).unwrap().unwrap().clone();
        assert!(surface.scene().has_prepared_frame());
        assert_eq!(
            surface.create_node(element_type(), ComputedLayoutStyle::default()),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.update_layout_style(root, ComputedLayoutStyle::default()),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.set_measurable(root, true),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.set_measurement(root, None),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.invalidate_node(root, PropertyImpactSet::LAYOUT),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.compute_layout(root, LayoutSize::new(1.0, 1.0), &mut zero_measure),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.compute_layout_with_measurements(root, LayoutSize::new(1.0, 1.0), 1),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.apply_measurement_responses(&[]),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        let fake_ready = MeasurementReady {
            key: MeasurementKey::new(1).expect("key"),
            request_id: MeasurementRequestId::new(1).expect("request"),
            environment_epoch: 1,
            metrics: measurement_metrics(1.0, 1.0),
        };
        assert_eq!(
            surface.apply_measurement_ready(&fake_ready),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.delete_node(root),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.prepare_frame(1),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );

        surface.discard_pending().unwrap();
        assert!(!surface.scene().has_prepared_frame());
        let retried = surface.prepare_frame(1).unwrap().unwrap().clone();
        assert!(retried.header.frame_id > pending.header.frame_id);
        surface.require_snapshot().unwrap();
        assert!(!surface.scene().has_prepared_frame());

        let mut renderer = RecordingRenderer::new(surface_id());
        let snapshot = present_and_accept(&mut surface, &mut renderer, 1);
        assert_eq!(snapshot.header.mode, FrameMode::Snapshot);
        assert_eq!(
            surface.accept_pending(snapshot.header.target_revision),
            Err(SurfaceError::Scene(SceneError::NoPendingFrame))
        );
        assert_eq!(
            surface.discard_pending(),
            Err(SurfaceError::Scene(SceneError::NoPendingFrame))
        );
        surface.require_snapshot().unwrap();
        let recovery = present_and_accept(&mut surface, &mut renderer, 2);
        assert_eq!(recovery.header.mode, FrameMode::Snapshot);
    }

    #[test]
    fn projection_validation_and_error_sources_are_diagnostic() {
        let mut scene = Scene::new(surface_id());
        let live = scene.create_node(element_type()).unwrap();
        let valid = LayoutRect {
            width: 1.0,
            height: 1.0,
            ..LayoutRect::default()
        };
        assert_eq!(validate_layout_entries(&scene, &[(live, valid)]), Ok(()));
        assert_eq!(
            validate_layout_entries(&scene, &[(node_id(99), valid)]),
            Err(SurfaceError::SceneLayoutMismatch { node: node_id(99) })
        );
        let invalid = LayoutRect {
            width: f32::INFINITY,
            ..valid
        };
        assert_eq!(
            validate_layout_entries(&scene, &[(live, invalid)]),
            Err(SurfaceError::InvalidLayoutOutput { node: live })
        );

        let mut missing_layout = SurfaceEngine::new(surface_id());
        let scene_only = missing_layout.scene.create_node(element_type()).unwrap();
        assert_eq!(
            missing_layout.set_measurement(scene_only, None),
            Err(SurfaceError::Layout(LayoutError::UnknownNode(scene_only)))
        );
        assert_eq!(
            missing_layout.invalidate_node(scene_only, PropertyImpactSet::TEXT_METRICS),
            Err(SurfaceError::Layout(LayoutError::UnknownNode(scene_only)))
        );

        let mut missing_scene = SurfaceEngine::new(surface_id());
        let layout_only = missing_scene
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        missing_scene.scene.delete_node(layout_only).unwrap();
        assert_eq!(
            missing_scene
                .compute_layout(layout_only, LayoutSize::new(1.0, 1.0), &mut zero_measure,),
            Err(SurfaceError::SceneLayoutMismatch { node: layout_only })
        );

        let mut missing_host_scene = SurfaceEngine::new(surface_id());
        let host_layout_only = missing_host_scene
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        missing_host_scene
            .scene
            .delete_node(host_layout_only)
            .unwrap();
        assert_eq!(
            missing_host_scene.compute_layout_with_measurements(
                host_layout_only,
                LayoutSize::new(1.0, 1.0),
                1,
            ),
            Err(SurfaceError::SceneLayoutMismatch {
                node: host_layout_only
            })
        );

        let mut invalid_host_layout = SurfaceEngine::new(surface_id());
        let host_root = invalid_host_layout
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        assert_eq!(
            invalid_host_layout.compute_layout_with_measurements(
                host_root,
                LayoutSize::new(f32::NAN, 1.0),
                1,
            ),
            Err(SurfaceError::Layout(LayoutError::InvalidViewport))
        );
        assert_eq!(
            invalid_host_layout.compute_layout_with_measurements(
                node_id(99),
                LayoutSize::new(1.0, 1.0),
                1,
            ),
            Err(SurfaceError::Layout(LayoutError::UnknownNode(node_id(99))))
        );

        let scene_error = SurfaceError::from(SceneError::NoPendingFrame);
        let layout_error = SurfaceError::from(LayoutError::InvalidViewport);
        assert!(scene_error.source().is_some());
        assert!(layout_error.source().is_some());
        assert!(
            SurfaceError::SceneLayoutMismatch { node: live }
                .source()
                .is_none()
        );
        assert!(
            SurfaceError::InvalidLayoutOutput { node: live }
                .source()
                .is_none()
        );
        assert_eq!(
            format!("{layout_error}"),
            "Whisker surface error: Layout(InvalidViewport)"
        );
    }
}
