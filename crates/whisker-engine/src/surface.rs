//! Surface-level orchestration of retained scene and layout state.

use std::{collections::HashMap, error::Error, fmt};

use whisker_layout::{IntrinsicMeasurer, LayoutError, LayoutSize, LayoutSnapshot, LayoutTree};
use whisker_protocol::{
    ApplyResult, CommandId, ElementTypeId, FramePacket, HitTestBehavior, InputPoint,
    LayoutGeometry, MeasurementMetrics, MeasurementReady, MeasurementResponse, MeasurementSpec,
    NodeId, PointerId, PropertyId, ResultId, SurfaceId, TextContent, WhiskerValue,
};
use whisker_style::{
    ComputedLayoutStyle, ComputedStyle, ComputedTransformStyle, InheritedStyle, PropertyImpactSet,
};

use crate::{
    DeferredMeasurementApply, FrameSink, LayoutProgress, MeasurementApply, MeasurementError,
    PlainTextInput, Scene, SceneError, SceneNode, lower_paint, lower_plain_text, lower_transform,
    measurement::MeasurementCoordinator,
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
    /// A computed transform produced a non-finite matrix after box resolution.
    InvalidTransformOutput {
        /// Node whose transform could not be represented.
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
            Self::SceneLayoutMismatch { .. }
            | Self::InvalidLayoutOutput { .. }
            | Self::InvalidTransformOutput { .. } => None,
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

/// Failure while presenting one prepared transaction to a Host.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfacePresentError<SinkError> {
    /// Scene preparation or revision bookkeeping failed.
    Surface(SurfaceError),
    /// The Host rejected the call before accepting the transaction.
    Sink(SinkError),
}

impl<SinkError: fmt::Debug> fmt::Display for SurfacePresentError<SinkError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker surface presentation error: {self:?}")
    }
}

impl<SinkError: Error + 'static> Error for SurfacePresentError<SinkError> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Surface(error) => Some(error),
            Self::Sink(error) => Some(error),
        }
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
    transforms: HashMap<NodeId, ComputedTransformStyle>,
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
            transforms: HashMap::new(),
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

    /// Finds the visually topmost retained node at a surface-space point.
    pub fn hit_test(
        &self,
        root: NodeId,
        point: InputPoint,
    ) -> Result<Option<NodeId>, SurfaceError> {
        self.scene
            .hit_test(root, point)
            .map_err(SurfaceError::Scene)
    }

    /// Returns the node currently retaining one pointer capture.
    pub fn pointer_capture_target(&self, pointer: PointerId) -> Option<NodeId> {
        self.scene.pointer_capture_target(pointer)
    }

    /// Replaces one node's event subscription mask.
    pub fn set_event_mask(&mut self, node: NodeId, mask: u64) -> Result<(), SurfaceError> {
        self.scene
            .set_event_mask(node, mask)
            .map_err(SurfaceError::Scene)
    }

    /// Sets one typed element property in the retained scene.
    pub fn set_property(
        &mut self,
        node: NodeId,
        property: PropertyId,
        value: WhiskerValue,
    ) -> Result<(), SurfaceError> {
        self.scene
            .set_property(node, property, value)
            .map_err(SurfaceError::Scene)
    }

    /// Clears one typed element property in the retained scene.
    pub fn clear_property(
        &mut self,
        node: NodeId,
        property: PropertyId,
    ) -> Result<(), SurfaceError> {
        self.scene
            .clear_property(node, property)
            .map_err(SurfaceError::Scene)
    }

    /// Queues one typed element command after preceding visual mutations.
    pub fn invoke_command(
        &mut self,
        node: NodeId,
        command: CommandId,
        arguments: WhiskerValue,
        result: Option<ResultId>,
    ) -> Result<(), SurfaceError> {
        self.scene
            .invoke_command(node, command, arguments, result)
            .map_err(SurfaceError::Scene)
    }

    /// Replaces one node's hit-test participation.
    pub fn set_hit_test(
        &mut self,
        node: NodeId,
        behavior: HitTestBehavior,
    ) -> Result<(), SurfaceError> {
        self.scene
            .set_hit_test(node, behavior)
            .map_err(SurfaceError::Scene)
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
            self.transforms.remove(&removed);
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

    /// Applies every currently supported computed layout and paint value.
    pub fn update_computed_style(
        &mut self,
        node: NodeId,
        style: &ComputedStyle,
    ) -> Result<PropertyImpactSet, SurfaceError> {
        self.ensure_mutable()?;
        let lowered = lower_paint(style.paint(), style.layout());
        let transform_changed = self.transforms.get(&node) != Some(&lowered.transform);
        let projected_transform = self
            .last_layout
            .as_ref()
            .and_then(|layout| layout.get(node))
            .copied()
            .map(|geometry| {
                self.resolve_node_transform(
                    node,
                    &lowered.transform,
                    geometry.border_box.width,
                    geometry.border_box.height,
                )
            })
            .transpose()?;
        let paint_changed = {
            let current = self
                .scene
                .node(node)
                .ok_or(SceneError::UnknownNode { node })?;
            current.box_paint() != Some(&lowered.box_paint)
                || current.visual_effects() != &lowered.visual_effects
                || current.clip() != Some(lowered.clip)
                || current.opacity() != Some(lowered.opacity)
                || current.visibility() != Some(lowered.visibility)
                || current.z_order() != Some(lowered.z_order)
                || transform_changed
        };
        let mut impacts = self.update_layout_style(node, style.layout().clone())?;
        self.scene
            .set_box_paint(node, lowered.box_paint)
            .expect("lowered paint is valid and the scene is mutable");
        self.scene
            .set_visual_effects(node, lowered.visual_effects)
            .expect("lowered visual effects are valid and the scene is mutable");
        self.scene
            .set_clip(node, lowered.clip)
            .expect("the retained scene node was validated above");
        self.scene
            .set_opacity(node, lowered.opacity)
            .expect("computed opacity is valid and the scene node exists");
        self.scene
            .set_visibility(node, lowered.visibility)
            .expect("the retained scene node was validated above");
        self.scene
            .set_z_order(node, lowered.z_order)
            .expect("the retained scene node was validated above");
        self.transforms.insert(node, lowered.transform);
        if let Some(transform) = projected_transform.flatten() {
            self.scene
                .set_transform(node, transform)
                .expect("the transform was validated before retained style mutation");
        }
        if paint_changed {
            impacts |= PropertyImpactSet::PAINT;
        }
        Ok(impacts)
    }

    /// Updates retained background image layers independently from computed
    /// box paint. Resource identity is assigned by the runtime boundary, not
    /// by the layout/style engine.
    pub fn set_background_layers(
        &mut self,
        node: NodeId,
        layers: Vec<whisker_protocol::BackgroundLayer>,
    ) -> Result<(), SurfaceError> {
        self.ensure_mutable()?;
        self.scene.set_background_layers(node, layers)?;
        Ok(())
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

    /// Lowers and registers one plain UTF-8 Text v1 presentation.
    ///
    /// Metric-affecting values come from the already resolved inherited text
    /// context. The same payload is retained for final frame production and
    /// registered with Taffy for Host intrinsic measurement.
    pub fn set_plain_text(
        &mut self,
        node: NodeId,
        input: &PlainTextInput,
        style: &InheritedStyle,
    ) -> Result<bool, SurfaceError> {
        self.ensure_mutable()?;
        let previous = self
            .scene
            .node(node)
            .ok_or(SceneError::UnknownNode { node })?
            .text()
            .cloned();
        let (mut content, measurement) = lower_plain_text(input, style).into_parts();
        if previous.as_ref().map(|value| &value.payload) == Some(&content.payload) {
            content.prepared_content = previous.as_ref().and_then(|value| value.prepared_content);
        }
        let content_changed = previous.as_ref() != Some(&content);
        let measurement_changed = self.set_measurement(node, Some(measurement))?;
        self.scene
            .set_text(node, content)
            .expect("validated synchronized text node remains mutable");
        Ok(content_changed || measurement_changed)
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
        let ready_nodes = self.measurements.ready_nodes();
        self.sync_text_presentations(&ready_nodes);
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
        self.sync_text_presentations(apply.invalidated_nodes());
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
        self.sync_text_presentations(apply.invalidated_nodes());
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

    /// Presents the next transaction and applies the Host acknowledgement.
    ///
    /// `NeedSnapshot` rotates the scene epoch and leaves a complete snapshot
    /// ready for the next call. A sink error discards only the prepared packet;
    /// retained semantic changes remain dirty and can be retried.
    pub fn present<Sink: FrameSink>(
        &mut self,
        viewport_epoch: u32,
        sink: &mut Sink,
    ) -> Result<Option<ApplyResult>, SurfacePresentError<Sink::Error>> {
        let packet = self
            .prepare_frame(viewport_epoch)
            .map_err(SurfacePresentError::Surface)?
            .cloned();
        let Some(packet) = packet else {
            return Ok(None);
        };
        let result = match sink.present(&packet) {
            Ok(result) => result,
            Err(error) => {
                self.discard_pending()
                    .expect("a sink error always follows successful frame preparation");
                return Err(SurfacePresentError::Sink(error));
            }
        };
        match result {
            ApplyResult::Accepted { revision } => self
                .accept_pending(revision)
                .map_err(SurfacePresentError::Surface)?,
            ApplyResult::NeedSnapshot { .. } => self
                .require_snapshot()
                .map_err(SurfacePresentError::Surface)?,
        }
        Ok(Some(result))
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
        let transforms = changed
            .iter()
            .map(|(node, rect)| {
                self.transforms
                    .get(node)
                    .map(|style| {
                        self.resolve_node_transform(
                            *node,
                            style,
                            rect.border_box.width,
                            rect.border_box.height,
                        )
                    })
                    .transpose()
                    .map(Option::flatten)
            })
            .collect::<Result<Vec<_>, SurfaceError>>()?;
        for ((node, rect), transform) in changed.iter().zip(transforms) {
            self.scene
                .set_layout(*node, *rect)
                .expect("validated layout projection must enter a mutable synchronized scene");
            if let Some(transform) = transform {
                self.scene
                    .set_transform(*node, transform)
                    .expect("the transform was validated before layout projection");
            }
        }
        self.last_layout = Some(snapshot);
        self.last_inputs = Some(inputs);
        self.layout_dirty = false;
        Ok(LayoutUpdate::computed(changed.len()))
    }

    fn resolve_node_transform(
        &self,
        node: NodeId,
        style: &ComputedTransformStyle,
        border_width: f32,
        border_height: f32,
    ) -> Result<Option<whisker_protocol::Transform>, SurfaceError> {
        if style.perspective.is_none()
            && matches!(
                style.offset_path,
                whisker_style::ComputedOffsetPathValue::None
            )
            && style.functions.is_empty()
            && self
                .scene
                .node(node)
                .and_then(SceneNode::transform)
                .is_none()
        {
            return Ok(None);
        }
        let transform = lower_transform(style, border_width, border_height)
            .ok_or(SurfaceError::InvalidTransformOutput { node })?;
        Ok(Some(transform))
    }

    fn sync_text_presentations(&mut self, nodes: &[NodeId]) {
        for node in nodes {
            let Some(metrics) = self.measurements.last_ready(*node) else {
                continue;
            };
            let Some(current) = self.scene.node(*node).and_then(SceneNode::text) else {
                continue;
            };
            let content = TextContent {
                payload: current.payload.clone(),
                paint: current.paint.clone(),
                prepared_content: metrics.prepared_content,
            };
            self.scene
                .set_text(*node, content)
                .expect("measured text remains valid and the scene is mutable");
        }
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
    entries: &[(NodeId, LayoutGeometry)],
) -> Result<(), SurfaceError> {
    for (node, geometry) in entries {
        if scene.node(*node).is_none() {
            return Err(SurfaceError::SceneLayoutMismatch { node: *node });
        }
        if !geometry.is_valid() {
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
        ApplyResult, BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode,
        CustomMeasurePayload, ElementTypeId, EmbeddedSurfaceMeasurePayload, FrameMode,
        HitTestBehavior, ImageRepeat, InputPoint, LayoutRect, MeasureFontFamily, MeasureFontStyle,
        MeasureLineHeight, MeasureTextDirection, MeasureTextOverflow, MeasureTextWrap,
        MeasuredSize, MeasurementKey, MeasurementKind, MeasurementMetrics, MeasurementPayload,
        MeasurementReady, MeasurementRequestId, MeasurementResponse, MeasurementSpec,
        NativeControlMeasurePayload, NodeId, Operation, PaintBox, PaintCoordinate, PaintImage,
        PaintPosition, PendingMeasurePolicy, PointerId, ReplacedContentMeasurePayload, ResourceId,
        SurfaceId, TextMeasurePayload, TextMeasureStyle, UnsupportedMeasurementReason,
    };
    use whisker_style::{
        Axes, ComputedLengthPercentage, ComputedSizeValue, ComputedTransformFunction,
        ComputedTransformStyle, LengthPercentageValue, PositionValue, SpecifiedStyle,
        StyleEnvironment, StyleNumber, StyleProperty, StyleValue, TransformFunctionValue,
        TransformValue, resolve_style,
    };

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

    fn resource_background(resource: u64) -> BackgroundLayer {
        BackgroundLayer {
            image: PaintImage::Resource(ResourceId::new(resource).expect("test resource")),
            position: PaintPosition::default(),
            size: BackgroundSize::Auto,
            repeat_x: ImageRepeat::Repeat,
            repeat_y: ImageRepeat::Repeat,
            origin: PaintBox::Padding,
            clip: PaintBox::Border,
            attachment: BackgroundAttachment::Scroll,
            blend_mode: BlendMode::Normal,
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
            content_hash: 1,
            style_hash: 2,
            payload: measurement_payload(kind),
            pending_policy,
        }
    }

    fn measurement_payload(kind: MeasurementKind) -> MeasurementPayload {
        match kind {
            MeasurementKind::Text => MeasurementPayload::Text(TextMeasurePayload {
                text: "Hello, Whisker".into(),
                style: TextMeasureStyle {
                    font_families: vec![MeasureFontFamily::System],
                    font_size: 14.0,
                    font_weight: 400,
                    font_style: MeasureFontStyle::Normal,
                    line_height: MeasureLineHeight::Normal,
                    letter_spacing: 0.0,
                    ..TextMeasureStyle::default()
                },
                locale: Some("en-US".into()),
                direction: MeasureTextDirection::LeftToRight,
                wrap: MeasureTextWrap::Wrap,
                max_lines: None,
                overflow: MeasureTextOverflow::Clip,
            }),
            MeasurementKind::ReplacedContent => {
                MeasurementPayload::ReplacedContent(ReplacedContentMeasurePayload::default())
            }
            MeasurementKind::NativeControl => {
                MeasurementPayload::NativeControl(NativeControlMeasurePayload {
                    control_type: 1,
                    version: 1,
                    state: Vec::new(),
                })
            }
            MeasurementKind::EmbeddedSurface => {
                MeasurementPayload::EmbeddedSurface(EmbeddedSurfaceMeasurePayload {
                    surface: SurfaceId::new(2).expect("child surface"),
                    preferred_size: None,
                })
            }
            MeasurementKind::Custom { version } => {
                MeasurementPayload::Custom(CustomMeasurePayload {
                    version,
                    data: Vec::new(),
                })
            }
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
    fn transform_style_projects_before_and_after_layout_and_rejects_invalid_matrices() {
        let resolved = |function| {
            resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::Transform,
                    StyleValue::Transform(TransformValue(vec![function])),
                ),
                None,
                StyleEnvironment::default(),
            )
            .unwrap()
        };
        let translated = resolved(TransformFunctionValue::TranslateX(
            LengthPercentageValue::Percentage(StyleNumber::new(50.0)),
        ));
        let mut surface = SurfaceEngine::new(surface_id());
        let root = surface
            .create_node(element_type(), translated.computed().layout().clone())
            .unwrap();
        surface
            .update_computed_style(root, translated.computed())
            .unwrap();
        assert_eq!(surface.node(root).unwrap().transform(), None);
        surface
            .compute_layout(root, LayoutSize::new(40.0, 20.0), &mut zero_measure)
            .unwrap();
        let projected = surface.node(root).unwrap().transform().unwrap();
        assert_eq!(projected.0[12], 20.0);

        let rotated = resolved(TransformFunctionValue::Rotate(StyleNumber::new(90.0)));
        assert!(
            surface
                .update_computed_style(root, rotated.computed())
                .unwrap()
                .contains(PropertyImpactSet::PAINT)
        );
        assert_ne!(surface.node(root).unwrap().transform(), Some(projected));

        let huge = resolved(TransformFunctionValue::Matrix([
            StyleNumber::new(f32::MAX),
            StyleNumber::new(0.0),
            StyleNumber::new(0.0),
            StyleNumber::new(1.0),
            StyleNumber::new(0.0),
            StyleNumber::new(0.0),
        ]));
        assert_eq!(
            surface.update_computed_style(root, huge.computed()),
            Err(SurfaceError::InvalidTransformOutput { node: root })
        );

        let mut values = [StyleNumber::new(0.0); 16];
        values[0] = StyleNumber::new(f32::NAN);
        let invalid = ComputedTransformStyle {
            functions: vec![ComputedTransformFunction::Matrix(values)],
            ..ComputedTransformStyle::default()
        };
        assert_eq!(
            surface.resolve_node_transform(root, &invalid, 40.0, 20.0),
            Err(SurfaceError::InvalidTransformOutput { node: root })
        );

        let mut invalid_projection = SurfaceEngine::new(surface_id());
        let invalid_root = invalid_projection
            .create_node(element_type(), sized(40.0, 20.0))
            .unwrap();
        invalid_projection
            .transforms
            .insert(invalid_root, invalid.clone());
        assert_eq!(
            invalid_projection.compute_layout(
                invalid_root,
                LayoutSize::new(40.0, 20.0),
                &mut zero_measure,
            ),
            Err(SurfaceError::InvalidTransformOutput { node: invalid_root })
        );

        let mut empty_surface = SurfaceEngine::new(surface_id());
        let empty = empty_surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        assert_eq!(
            empty_surface
                .resolve_node_transform(empty, &ComputedTransformStyle::default(), 1.0, 1.0)
                .unwrap(),
            None
        );
        empty_surface
            .scene
            .set_transform(empty, whisker_protocol::Transform::IDENTITY)
            .unwrap();
        assert_eq!(
            empty_surface
                .resolve_node_transform(empty, &ComputedTransformStyle::default(), 1.0, 1.0)
                .unwrap(),
            Some(whisker_protocol::Transform::IDENTITY)
        );

        let point = |x, y| whisker_style::MotionPathPointValue {
            x: StyleNumber::new(x),
            y: StyleNumber::new(y),
        };
        let motion = ComputedTransformStyle {
            offset_path: whisker_style::ComputedOffsetPathValue::Path(vec![
                whisker_style::MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                whisker_style::MotionPathCommandValue::LineTo(point(10.0, 0.0)),
            ]),
            offset_distance: StyleNumber::new(0.5),
            ..ComputedTransformStyle::default()
        };
        assert_eq!(
            empty_surface
                .resolve_node_transform(empty, &motion, 1.0, 1.0)
                .unwrap()
                .unwrap()
                .0[12],
            5.0
        );
    }

    #[test]
    fn computed_paint_and_input_wrappers_share_the_retained_scene() {
        let resolved = resolve_style(
            &SpecifiedStyle::new(),
            None,
            StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
        )
        .unwrap();
        let style = resolved.computed();
        let mut surface = SurfaceEngine::new(surface_id());
        let root = surface
            .create_node(element_type(), style.layout().clone())
            .unwrap();
        let missing = node_id(99);

        let background = resource_background(1);
        surface
            .set_background_layers(root, vec![background.clone()])
            .unwrap();
        assert_eq!(
            surface.node(root).unwrap().background_layers(),
            std::slice::from_ref(&background)
        );
        assert_eq!(
            surface.set_background_layers(missing, vec![background]),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: missing
            }))
        );
        let mut invalid_background = resource_background(2);
        invalid_background.position.x = PaintCoordinate {
            length: f32::NAN,
            fraction: 0.0,
        };
        assert_eq!(
            surface.set_background_layers(root, vec![invalid_background]),
            Err(SurfaceError::Scene(SceneError::InvalidBackgroundLayers))
        );

        assert_eq!(
            surface.update_computed_style(missing, style),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: missing
            }))
        );
        let first = surface.update_computed_style(root, style).unwrap();
        assert!(first.contains(PropertyImpactSet::PAINT));
        let lowered = lower_paint(style.paint(), style.layout());
        let node = surface.node(root).unwrap();
        assert_eq!(node.box_paint(), Some(&lowered.box_paint));
        assert_eq!(node.clip(), Some(lowered.clip));
        assert_eq!(node.opacity(), Some(lowered.opacity));
        assert_eq!(node.visibility(), Some(lowered.visibility));
        assert_eq!(node.z_order(), Some(lowered.z_order));
        assert!(
            surface
                .update_computed_style(root, style)
                .unwrap()
                .is_empty()
        );

        let mut different_box = lowered.box_paint.clone();
        different_box.background_color = whisker_protocol::PaintColor::Named("changed".into());
        surface.scene.set_box_paint(root, different_box).unwrap();
        assert!(
            surface
                .update_computed_style(root, style)
                .unwrap()
                .contains(PropertyImpactSet::PAINT)
        );
        surface
            .scene
            .set_clip(
                root,
                whisker_protocol::BoxClip {
                    horizontal: whisker_protocol::OverflowClip::Hidden,
                    vertical: whisker_protocol::OverflowClip::Hidden,
                },
            )
            .unwrap();
        assert!(
            surface
                .update_computed_style(root, style)
                .unwrap()
                .contains(PropertyImpactSet::PAINT)
        );
        surface.scene.set_opacity(root, 0.25).unwrap();
        assert!(
            surface
                .update_computed_style(root, style)
                .unwrap()
                .contains(PropertyImpactSet::PAINT)
        );
        surface
            .scene
            .set_visibility(root, whisker_protocol::Visibility::Hidden)
            .unwrap();
        assert!(
            surface
                .update_computed_style(root, style)
                .unwrap()
                .contains(PropertyImpactSet::PAINT)
        );
        surface.scene.set_z_order(root, 7).unwrap();
        assert!(
            surface
                .update_computed_style(root, style)
                .unwrap()
                .contains(PropertyImpactSet::PAINT)
        );

        let point = InputPoint { x: 5.0, y: 5.0 };
        assert_eq!(surface.hit_test(root, point), Ok(None));
        surface
            .scene
            .set_layout(
                root,
                whisker_protocol::LayoutRect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
            )
            .unwrap();
        assert_eq!(surface.hit_test(root, point), Ok(Some(root)));
        assert_eq!(
            surface.hit_test(missing, point),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: missing
            }))
        );

        surface.set_event_mask(root, 5).unwrap();
        assert_eq!(surface.node(root).unwrap().event_mask(), Some(5));
        assert_eq!(
            surface.set_event_mask(missing, 1),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: missing
            }))
        );

        let property = PropertyId::new(1).unwrap();
        assert_eq!(
            surface.set_property(root, property, WhiskerValue::Bool(true)),
            Ok(())
        );
        assert_eq!(
            surface.set_property(missing, property, WhiskerValue::Bool(false)),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: missing
            }))
        );
        assert_eq!(surface.clear_property(root, property), Ok(()));
        assert_eq!(
            surface.clear_property(missing, property),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: missing
            }))
        );

        let command = CommandId::new(1).unwrap();
        assert_eq!(
            surface.invoke_command(root, command, WhiskerValue::Null, None),
            Ok(())
        );
        assert_eq!(
            surface.invoke_command(missing, command, WhiskerValue::Null, None),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: missing
            }))
        );

        surface.set_hit_test(root, HitTestBehavior::None).unwrap();
        assert_eq!(surface.hit_test(root, point), Ok(None));
        assert_eq!(
            surface.set_hit_test(missing, HitTestBehavior::Auto),
            Err(SurfaceError::Scene(SceneError::UnknownNode {
                node: missing
            }))
        );
        let pointer = PointerId::new(1).unwrap();
        assert_eq!(surface.pointer_capture_target(pointer), None);
        surface.scene.set_pointer_capture(root, pointer).unwrap();
        assert_eq!(surface.pointer_capture_target(pointer), Some(root));

        let unsupported = resolve_style(
            &SpecifiedStyle::new().push(
                whisker_style::StyleProperty::Position,
                whisker_style::StyleValue::Position(PositionValue::Sticky),
            ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            surface.update_computed_style(root, unsupported.computed()),
            Err(SurfaceError::Layout(LayoutError::UnsupportedStyle(
                UnsupportedLayoutFeature::StickyPosition
            )))
        );
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
        let first_pass_calls = calls.get();
        assert!(first_pass_calls > 0);
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
        assert_eq!(calls.get(), first_pass_calls);

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
        let invalidated_pass_calls = calls.get();
        assert!(invalidated_pass_calls > first_pass_calls);
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
        assert!(calls.get() > invalidated_pass_calls);

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
        assert!(!blocked.requests().is_empty());
        assert_eq!(surface.last_layout(), None);
        let initial_requests = blocked.requests().to_vec();

        let repeated = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 1)
            .unwrap();
        assert_eq!(repeated.requests(), initial_requests);

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

        let initial_responses = initial_requests
            .iter()
            .map(|request| MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: 1,
                metrics: measurement_metrics(24.0, 12.0),
            })
            .collect::<Vec<_>>();
        let applied = surface
            .apply_measurement_responses(&initial_responses)
            .unwrap();
        assert_eq!(applied.applied(), initial_responses.len());
        let complete = loop {
            let progress = surface
                .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 1)
                .unwrap();
            if progress.requests().is_empty() {
                break progress;
            }
            let responses = progress
                .requests()
                .iter()
                .map(|request| MeasurementResponse::Ready {
                    key: request.key,
                    environment_epoch: 1,
                    metrics: measurement_metrics(24.0, 12.0),
                })
                .collect::<Vec<_>>();
            surface.apply_measurement_responses(&responses).unwrap();
        };
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
        assert!(!provisional.requests().is_empty());
        let initial_requests = provisional.requests().to_vec();
        let idle = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 7)
            .unwrap();
        assert_eq!(idle.requests(), initial_requests);

        let mut pending_measurements = Vec::new();
        let mut progress = provisional;
        let pending = loop {
            let requests = progress.requests().to_vec();
            if requests.is_empty() {
                break progress;
            }
            let responses = requests
                .iter()
                .map(|request| {
                    let request_id = MeasurementRequestId::new(100 + request.key.get())
                        .expect("pending request ID");
                    pending_measurements.push((request.key, request_id));
                    MeasurementResponse::Pending {
                        key: request.key,
                        environment_epoch: 7,
                        request_id,
                        provisional: Some(measurement_metrics(20.0, 10.0)),
                    }
                })
                .collect::<Vec<_>>();
            surface.apply_measurement_responses(&responses).unwrap();
            progress = surface
                .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 7)
                .unwrap();
        };
        assert!(matches!(
            pending,
            LayoutProgress::Provisional {
                update,
                requests,
                pending,
            } if update.recomputed() && requests.is_empty() && pending > 0
        ));

        let idle_pending = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 7)
            .unwrap();
        assert!(matches!(
            idle_pending,
            LayoutProgress::Provisional {
                update,
                requests,
                pending,
            } if update == LayoutUpdate::default()
                && requests.is_empty()
                && pending > 0
        ));

        let (key, request_id) = pending_measurements[0];
        let invalid_ready = MeasurementReady {
            key,
            request_id,
            environment_epoch: 7,
            metrics: measurement_metrics(f32::NAN, 1.0),
        };
        assert_eq!(
            surface.apply_measurement_ready(&invalid_ready),
            Err(SurfaceError::Measurement(
                MeasurementError::InvalidMetrics { key }
            ))
        );

        let stale = MeasurementReady {
            key: MeasurementKey::new(key.get() + 10_000).expect("wrong key"),
            request_id,
            environment_epoch: 7,
            metrics: measurement_metrics(32.0, 18.0),
        };
        assert_eq!(
            surface.apply_measurement_ready(&stale).unwrap(),
            DeferredMeasurementApply::IgnoredStale
        );
        assert!(!surface.needs_layout());

        let ready = MeasurementReady { key, ..stale };
        assert_eq!(
            surface.apply_measurement_ready(&ready).unwrap(),
            DeferredMeasurementApply::Applied {
                invalidated_nodes: vec![root]
            }
        );
        assert!(surface.needs_layout());
        for (key, request_id) in pending_measurements.into_iter().skip(1) {
            assert_eq!(
                surface
                    .apply_measurement_ready(&MeasurementReady {
                        key,
                        request_id,
                        environment_epoch: 7,
                        metrics: measurement_metrics(32.0, 18.0),
                    })
                    .unwrap(),
                DeferredMeasurementApply::Applied {
                    invalidated_nodes: vec![root]
                }
            );
        }
        let complete = loop {
            let progress = surface
                .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 7)
                .unwrap();
            if progress.requests().is_empty() {
                break progress;
            }
            let responses = progress
                .requests()
                .iter()
                .map(|request| MeasurementResponse::Ready {
                    key: request.key,
                    environment_epoch: 7,
                    metrics: measurement_metrics(32.0, 18.0),
                })
                .collect::<Vec<_>>();
            surface.apply_measurement_responses(&responses).unwrap();
        };
        assert!(matches!(complete, LayoutProgress::Complete(update) if update.recomputed()));
        assert_eq!(
            surface.last_measurement(root),
            Some(&measurement_metrics(32.0, 18.0))
        );

        let new_environment = surface
            .compute_layout_with_measurements(root, LayoutSize::new(100.0, 100.0), 8)
            .unwrap();
        assert!(new_environment.has_layout());
        assert!(!new_environment.requests().is_empty());
    }

    #[test]
    fn invalid_and_unsupported_measurement_surface_diagnostics() {
        assert_eq!(
            measurement_spec(
                MeasurementKind::EmbeddedSurface,
                PendingMeasurePolicy::Block
            )
            .payload
            .kind(),
            MeasurementKind::EmbeddedSurface
        );
        assert_eq!(
            measurement_spec(
                MeasurementKind::Custom { version: 1 },
                PendingMeasurePolicy::Block
            )
            .payload
            .kind(),
            MeasurementKind::Custom { version: 1 }
        );
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
        let computed =
            resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default()).unwrap();
        let lowered = lower_paint(computed.computed().paint(), computed.computed().layout());
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
            surface.update_computed_style(root, computed.computed()),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface.scene.set_box_paint(root, lowered.box_paint),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            surface.set_background_layers(root, vec![resource_background(1)]),
            Err(SurfaceError::Scene(SceneError::FramePending))
        );
        assert_eq!(
            surface
                .scene
                .set_background_layers(root, vec![resource_background(1)]),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            surface.scene.set_clip(root, lowered.clip),
            Err(SceneError::FramePending)
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
        assert_eq!(
            validate_layout_entries(&scene, &[(live, valid.into())]),
            Ok(())
        );
        assert_eq!(
            validate_layout_entries(&scene, &[(node_id(99), valid.into())]),
            Err(SurfaceError::SceneLayoutMismatch { node: node_id(99) })
        );
        let invalid = LayoutRect {
            width: f32::INFINITY,
            ..valid
        };
        assert_eq!(
            validate_layout_entries(&scene, &[(live, invalid.into())]),
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

    #[test]
    fn present_applies_acknowledgements_recovery_and_retry() {
        enum SinkBehavior {
            Record,
            Fail,
            WrongRevision,
            NeedSnapshot,
        }

        struct TestSink {
            behavior: SinkBehavior,
            renderer: RecordingRenderer,
        }

        impl TestSink {
            fn new(surface: SurfaceId) -> Self {
                Self {
                    behavior: SinkBehavior::Record,
                    renderer: RecordingRenderer::new(surface),
                }
            }
        }

        impl FrameSink for TestSink {
            type Error = &'static str;

            fn capabilities(&self) -> whisker_protocol::RenderCapabilities {
                self.renderer.capabilities()
            }

            fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
                match self.behavior {
                    SinkBehavior::Record => self
                        .renderer
                        .present(packet)
                        .map_err(|_| "recording renderer rejected frame"),
                    SinkBehavior::Fail => Err("transport"),
                    SinkBehavior::WrongRevision => Ok(ApplyResult::Accepted {
                        revision: packet.header.target_revision + 1,
                    }),
                    SinkBehavior::NeedSnapshot => Ok(ApplyResult::NeedSnapshot {
                        receiver_revision: 0,
                    }),
                }
            }
        }

        let mut surface = SurfaceEngine::new(surface_id());
        let root = surface
            .create_node(element_type(), ComputedLayoutStyle::default())
            .unwrap();
        let mut sink = TestSink::new(surface_id());
        assert_eq!(
            sink.capabilities(),
            whisker_protocol::RenderCapabilities::all_frame_native()
        );
        let already_prepared = surface.prepare_frame(1).unwrap().unwrap().clone();
        assert_eq!(
            surface.present(1, &mut sink),
            Err(SurfacePresentError::Surface(SurfaceError::Scene(
                SceneError::FramePending
            )))
        );
        surface
            .accept_pending(already_prepared.header.target_revision)
            .unwrap();
        assert_eq!(surface.present(1, &mut sink), Ok(None));
        assert_eq!(surface.scene().accepted_revision(), 1);

        surface
            .scene
            .set_visibility(root, whisker_protocol::Visibility::Hidden)
            .unwrap();
        sink.renderer = RecordingRenderer::new(surface_id());
        assert_eq!(
            surface.present(1, &mut sink),
            Ok(Some(ApplyResult::NeedSnapshot {
                receiver_revision: 0
            }))
        );
        assert_eq!(surface.scene().accepted_revision(), 1);
        assert_eq!(
            surface.present(1, &mut sink),
            Ok(Some(ApplyResult::Accepted { revision: 2 }))
        );
        assert_eq!(
            sink.renderer.frames()[1].packet.header.mode,
            FrameMode::Snapshot
        );

        surface.scene.set_z_order(root, 5).unwrap();
        sink.behavior = SinkBehavior::Fail;
        assert_eq!(
            surface.present(1, &mut sink),
            Err(SurfacePresentError::Sink("transport"))
        );
        assert!(surface.has_pending_work());
        sink.behavior = SinkBehavior::Record;
        assert_eq!(
            surface.present(1, &mut sink),
            Ok(Some(ApplyResult::Accepted { revision: 3 }))
        );
        assert_eq!(surface.present(1, &mut sink), Ok(None));

        surface.scene.set_opacity(root, 0.4).unwrap();
        sink.behavior = SinkBehavior::WrongRevision;
        assert_eq!(
            surface.present(1, &mut sink),
            Err(SurfacePresentError::Surface(SurfaceError::Scene(
                SceneError::AcceptedRevisionMismatch {
                    expected: 4,
                    received: 5,
                }
            )))
        );
        surface.discard_pending().unwrap();

        surface.scene.set_scene_epoch_for_tests(u32::MAX);
        surface.scene.set_opacity(root, 0.6).unwrap();
        sink.behavior = SinkBehavior::NeedSnapshot;
        assert_eq!(
            surface.present(1, &mut sink),
            Err(SurfacePresentError::Surface(SurfaceError::Scene(
                SceneError::SceneEpochExhausted
            )))
        );

        let display = SurfacePresentError::<std::io::Error>::Surface(SurfaceError::Scene(
            SceneError::NoPendingFrame,
        ));
        assert!(display.source().is_some());
        assert!(format!("{display}").starts_with("Whisker surface presentation error:"));
        let sink = SurfacePresentError::Sink(std::io::Error::other("sink"));
        assert!(sink.source().is_some());
    }
}
