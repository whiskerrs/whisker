//! Retained scene state and coalescing frame journal.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt;

use whisker_protocol::{
    BoxClip, BoxPaint, CommandId, ElementTypeId, FrameHeader, FrameMode, FramePacket,
    HitTestBehavior, InputPoint, LayoutRect, NodeId, Operation, OverflowClip, PointerId,
    PropertyId, ProtocolValue, ProtocolVersion, ResultId, SurfaceId, TextContent, TextContentError,
    Transform, Visibility,
};

/// A retained logical node owned by a [`Scene`].
#[derive(Clone, Debug, PartialEq)]
pub struct SceneNode {
    element_type: ElementTypeId,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    layout: Option<LayoutRect>,
    box_paint: Option<BoxPaint>,
    clip: Option<BoxClip>,
    transform: Option<Transform>,
    opacity: Option<f32>,
    visibility: Option<Visibility>,
    z_order: Option<i32>,
    text: Option<TextContent>,
    properties: BTreeMap<PropertyId, ProtocolValue>,
    event_mask: Option<u64>,
    hit_test: Option<HitTestBehavior>,
    captured_pointers: BTreeSet<PointerId>,
}

impl SceneNode {
    fn new(element_type: ElementTypeId) -> Self {
        Self {
            element_type,
            parent: None,
            children: Vec::new(),
            layout: None,
            box_paint: None,
            clip: None,
            transform: None,
            opacity: None,
            visibility: None,
            z_order: None,
            text: None,
            properties: BTreeMap::new(),
            event_mask: None,
            hit_test: None,
            captured_pointers: BTreeSet::new(),
        }
    }

    /// Returns the registered element type.
    pub const fn element_type(&self) -> ElementTypeId {
        self.element_type
    }

    /// Returns the logical parent when attached.
    pub const fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// Returns children in logical presentation order.
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    /// Returns retained plain-text presentation when this is a text node.
    pub const fn text(&self) -> Option<&TextContent> {
        self.text.as_ref()
    }

    /// Returns retained background and border paint.
    pub const fn box_paint(&self) -> Option<&BoxPaint> {
        self.box_paint.as_ref()
    }

    /// Returns retained descendant overflow clipping.
    pub const fn clip(&self) -> Option<BoxClip> {
        self.clip
    }

    /// Returns retained group opacity.
    pub const fn opacity(&self) -> Option<f32> {
        self.opacity
    }

    /// Returns retained paint visibility.
    pub const fn visibility(&self) -> Option<Visibility> {
        self.visibility
    }

    /// Returns retained sibling stacking key.
    pub const fn z_order(&self) -> Option<i32> {
        self.z_order
    }

    /// Returns retained event subscription bits.
    pub const fn event_mask(&self) -> Option<u64> {
        self.event_mask
    }

    /// Returns retained Host hit-test behavior.
    pub const fn hit_test(&self) -> Option<HitTestBehavior> {
        self.hit_test
    }
}

/// A rejected scene mutation or frame-lifecycle transition.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneError {
    /// A mutation or second preparation was attempted while a frame is pending.
    FramePending,
    /// Acceptance or discard was attempted without a prepared frame.
    NoPendingFrame,
    /// Renderer accepted a revision other than the prepared target.
    AcceptedRevisionMismatch {
        /// Prepared target revision.
        expected: u64,
        /// Revision reported by the receiver.
        received: u64,
    },
    /// A node reference is not live in this scene.
    UnknownNode {
        /// Missing node.
        node: NodeId,
    },
    /// An insertion tried to attach a node that already has a parent.
    ChildAlreadyAttached {
        /// Child being inserted.
        child: NodeId,
        /// Existing parent.
        parent: NodeId,
    },
    /// A remove or move operation named a node outside the parent's direct children.
    NotDirectChild {
        /// Expected parent.
        parent: NodeId,
        /// Node that is not its direct child.
        child: NodeId,
    },
    /// A child position exceeded the resulting child-list length.
    ChildIndexOutOfBounds {
        /// Parent whose list was addressed.
        parent: NodeId,
        /// Requested position.
        index: u32,
        /// Maximum accepted insertion position.
        len: usize,
    },
    /// An insertion would introduce a parent cycle.
    TreeCycle {
        /// Requested parent.
        parent: NodeId,
        /// Requested child.
        child: NodeId,
    },
    /// Opacity was non-finite or outside `0.0..=1.0`.
    InvalidOpacity {
        /// Invalid opacity.
        opacity: f32,
    },
    /// Layout or transform contained NaN or infinity.
    NonFiniteNumber,
    /// Plain-text presentation contained invalid shaping inputs.
    InvalidText {
        /// Stable invalid-input category.
        error: TextContentError,
    },
    /// Background or border paint contained an invalid value.
    InvalidBoxPaint,
    /// A pending command result identifier was reused.
    DuplicateResultId {
        /// Duplicate result identifier.
        result: ResultId,
    },
    /// The scene cannot allocate another node identifier.
    NodeIdExhausted,
    /// The scene cannot advance its accepted revision.
    RevisionExhausted,
    /// The scene cannot allocate another diagnostic frame identifier.
    FrameIdExhausted,
    /// The scene cannot rotate to another recovery epoch.
    SceneEpochExhausted,
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Whisker scene error: {self:?}")
    }
}

impl Error for SceneError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DirtySlot {
    Layout(NodeId),
    BoxPaint(NodeId),
    Clip(NodeId),
    Transform(NodeId),
    Opacity(NodeId),
    Visibility(NodeId),
    ZOrder(NodeId),
    Text(NodeId),
    Property(NodeId, PropertyId),
    EventMask(NodeId),
    HitTest(NodeId),
    Pointer(NodeId, PointerId),
}

#[derive(Clone, Debug, Default)]
struct ChangeJournal {
    operations: Vec<Operation>,
    coalesced: HashMap<DirtySlot, usize>,
    result_ids: HashSet<ResultId>,
}

impl ChangeJournal {
    fn push_barrier(&mut self, operation: Operation) {
        self.coalesced.clear();
        self.operations.push(operation);
    }

    fn push_coalesced(&mut self, slot: DirtySlot, operation: Operation) {
        if let Some(index) = self.coalesced.get(&slot).copied() {
            self.operations[index] = operation;
        } else {
            let index = self.operations.len();
            self.operations.push(operation);
            self.coalesced.insert(slot, index);
        }
    }

    fn clear(&mut self) {
        self.operations.clear();
        self.coalesced.clear();
        self.result_ids.clear();
    }
}

/// Host-independent retained scene for one surface.
///
/// The scene begins in snapshot mode. After a frame is prepared, mutations are
/// rejected until the caller accepts or discards it. This makes clearing the
/// journal on acceptance lossless without requiring a second concurrent dirty
/// generation.
#[derive(Clone, Debug)]
pub struct Scene {
    surface: SurfaceId,
    scene_epoch: u32,
    accepted_revision: u64,
    next_node_id: u64,
    next_frame_id: u64,
    needs_snapshot: bool,
    nodes: BTreeMap<NodeId, SceneNode>,
    journal: ChangeJournal,
    pending: Option<FramePacket>,
}

impl Scene {
    /// Creates an empty scene that will emit a snapshot on its first frame.
    pub fn new(surface: SurfaceId) -> Self {
        Self {
            surface,
            scene_epoch: 1,
            accepted_revision: 0,
            next_node_id: 1,
            next_frame_id: 1,
            needs_snapshot: true,
            nodes: BTreeMap::new(),
            journal: ChangeJournal::default(),
            pending: None,
        }
    }

    /// Returns the destination surface.
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the current scene epoch.
    pub const fn scene_epoch(&self) -> u32 {
        self.scene_epoch
    }

    /// Returns the last renderer-accepted revision.
    pub const fn accepted_revision(&self) -> u64 {
        self.accepted_revision
    }

    /// Returns the number of live retained nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns a retained node when it is live.
    pub fn node(&self, node: NodeId) -> Option<&SceneNode> {
        self.nodes.get(&node)
    }

    /// Finds the visually topmost node at one surface-space point.
    pub fn hit_test(&self, root: NodeId, point: InputPoint) -> Result<Option<NodeId>, SceneError> {
        self.require_node(root)?;
        Ok(self.hit_test_node(root, point, 0.0, 0.0))
    }

    /// Returns the node currently retaining one pointer capture.
    pub fn pointer_capture_target(&self, pointer: PointerId) -> Option<NodeId> {
        self.nodes
            .iter()
            .find_map(|(node, state)| state.captured_pointers.contains(&pointer).then_some(*node))
    }

    fn hit_test_node(
        &self,
        node: NodeId,
        point: InputPoint,
        parent_x: f32,
        parent_y: f32,
    ) -> Option<NodeId> {
        let state = self.nodes.get(&node)?;
        if state.visibility == Some(Visibility::Hidden)
            || state.hit_test == Some(HitTestBehavior::None)
        {
            return None;
        }
        let layout = state.layout?;
        let x = parent_x + layout.x;
        let y = parent_y + layout.y;
        let contains_x = point.x >= x && point.x <= x + layout.width;
        let contains_y = point.y >= y && point.y <= y + layout.height;
        let contains = contains_x && contains_y;
        let children_clipped = state.clip.is_some_and(|clip| {
            (clip.horizontal == OverflowClip::Hidden && !contains_x)
                || (clip.vertical == OverflowClip::Hidden && !contains_y)
        });

        if state.hit_test != Some(HitTestBehavior::BoxOnly) && !children_clipped {
            let mut children: Vec<(usize, NodeId)> =
                state.children.iter().copied().enumerate().collect();
            children.sort_by_key(|(index, child)| {
                (
                    self.nodes
                        .get(child)
                        .and_then(|child| child.z_order)
                        .unwrap_or(0),
                    *index,
                )
            });
            for (_, child) in children.into_iter().rev() {
                if let Some(target) = self.hit_test_node(child, point, x, y) {
                    return Some(target);
                }
            }
        }

        (contains && state.hit_test != Some(HitTestBehavior::DescendantsOnly)).then_some(node)
    }

    /// Returns whether a snapshot, mutation, command, or retry needs a frame.
    pub fn has_pending_work(&self) -> bool {
        self.pending.is_some() || self.needs_snapshot || !self.journal.operations.is_empty()
    }

    /// Returns whether a prepared frame is waiting for acceptance or discard.
    pub fn has_prepared_frame(&self) -> bool {
        self.pending.is_some()
    }

    /// Creates an unattached retained node and returns its epoch-unique ID.
    pub fn create_node(&mut self, element_type: ElementTypeId) -> Result<NodeId, SceneError> {
        self.ensure_mutable()?;
        let node = NodeId::new(self.next_node_id).ok_or(SceneError::NodeIdExhausted)?;
        self.next_node_id = self.next_node_id.checked_add(1).unwrap_or(0);
        self.nodes.insert(node, SceneNode::new(element_type));
        self.journal
            .push_barrier(Operation::CreateNode { node, element_type });
        Ok(node)
    }

    /// Deletes a node and its complete retained subtree.
    pub fn delete_node(&mut self, node: NodeId) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        let state = self.require_node(node)?.clone();
        if let Some(parent) = state.parent {
            self.nodes
                .get_mut(&parent)
                .expect("attached parent remains live")
                .children
                .retain(|candidate| *candidate != node);
        }
        let mut pending = vec![node];
        while let Some(current) = pending.pop() {
            let removed = self
                .nodes
                .remove(&current)
                .expect("retained subtree is internally complete");
            pending.extend(removed.children);
        }
        self.journal.push_barrier(Operation::DeleteNode { node });
        Ok(())
    }

    /// Attaches an unattached child at an index.
    pub fn insert_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: u32,
    ) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        let len = self.require_node(parent)?.children.len();
        let child_state = self.require_node(child)?;
        if let Some(existing) = child_state.parent {
            return Err(SceneError::ChildAlreadyAttached {
                child,
                parent: existing,
            });
        }
        if parent == child || self.is_descendant(child, parent) {
            return Err(SceneError::TreeCycle { parent, child });
        }
        let index = index as usize;
        if index > len {
            return Err(SceneError::ChildIndexOutOfBounds {
                parent,
                index: index as u32,
                len,
            });
        }
        self.nodes
            .get_mut(&parent)
            .expect("parent checked above")
            .children
            .insert(index, child);
        self.nodes
            .get_mut(&child)
            .expect("child checked above")
            .parent = Some(parent);
        self.journal.push_barrier(Operation::InsertChild {
            parent,
            child,
            index: index as u32,
        });
        Ok(())
    }

    /// Detaches a direct child without deleting it.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        self.require_direct_child(parent, child)?;
        let children = &mut self
            .nodes
            .get_mut(&parent)
            .expect("parent checked above")
            .children;
        let position = children
            .iter()
            .position(|candidate| *candidate == child)
            .expect("parent link and child list agree");
        children.remove(position);
        self.nodes
            .get_mut(&child)
            .expect("child checked above")
            .parent = None;
        self.journal
            .push_barrier(Operation::RemoveChild { parent, child });
        Ok(())
    }

    /// Moves a direct child within its current parent.
    pub fn move_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: u32,
    ) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        self.require_direct_child(parent, child)?;
        let children = &mut self
            .nodes
            .get_mut(&parent)
            .expect("parent checked above")
            .children;
        let old_index = children
            .iter()
            .position(|candidate| *candidate == child)
            .expect("parent link and child list agree");
        children.remove(old_index);
        let index = index as usize;
        if index > children.len() {
            children.insert(old_index, child);
            return Err(SceneError::ChildIndexOutOfBounds {
                parent,
                index: index as u32,
                len: children.len() - 1,
            });
        }
        children.insert(index, child);
        self.journal.push_barrier(Operation::MoveChild {
            parent,
            child,
            index: index as u32,
        });
        Ok(())
    }

    /// Sets resolved layout when it differs from retained state.
    pub fn set_layout(&mut self, node: NodeId, rect: LayoutRect) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if ![rect.x, rect.y, rect.width, rect.height]
            .into_iter()
            .all(f32::is_finite)
        {
            return Err(SceneError::NonFiniteNumber);
        }
        if self.require_node(node)?.layout == Some(rect) {
            return Ok(());
        }
        self.nodes
            .get_mut(&node)
            .expect("node checked above")
            .layout = Some(rect);
        self.journal
            .push_coalesced(DirtySlot::Layout(node), Operation::SetLayout { node, rect });
        Ok(())
    }

    /// Sets background and border paint when it differs from retained state.
    pub fn set_box_paint(&mut self, node: NodeId, paint: BoxPaint) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if !paint.validate() {
            return Err(SceneError::InvalidBoxPaint);
        }
        if self.require_node(node)?.box_paint.as_ref() == Some(&paint) {
            return Ok(());
        }
        self.nodes
            .get_mut(&node)
            .expect("node checked above")
            .box_paint = Some(paint.clone());
        self.journal.push_coalesced(
            DirtySlot::BoxPaint(node),
            Operation::SetBoxPaint { node, paint },
        );
        Ok(())
    }

    /// Sets descendant overflow clipping when it differs from retained state.
    pub fn set_clip(&mut self, node: NodeId, clip: BoxClip) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if self.require_node(node)?.clip == Some(clip) {
            return Ok(());
        }
        self.nodes.get_mut(&node).expect("node checked above").clip = Some(clip);
        self.journal
            .push_coalesced(DirtySlot::Clip(node), Operation::SetClip { node, clip });
        Ok(())
    }

    /// Sets a resolved transform when it differs from retained state.
    pub fn set_transform(&mut self, node: NodeId, transform: Transform) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if !transform.0.into_iter().all(f32::is_finite) {
            return Err(SceneError::NonFiniteNumber);
        }
        if self.require_node(node)?.transform == Some(transform) {
            return Ok(());
        }
        self.nodes
            .get_mut(&node)
            .expect("node checked above")
            .transform = Some(transform);
        self.journal.push_coalesced(
            DirtySlot::Transform(node),
            Operation::SetTransform { node, transform },
        );
        Ok(())
    }

    /// Sets resolved opacity when it differs from retained state.
    pub fn set_opacity(&mut self, node: NodeId, opacity: f32) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(SceneError::InvalidOpacity { opacity });
        }
        if self.require_node(node)?.opacity == Some(opacity) {
            return Ok(());
        }
        self.nodes
            .get_mut(&node)
            .expect("node checked above")
            .opacity = Some(opacity);
        self.journal.push_coalesced(
            DirtySlot::Opacity(node),
            Operation::SetOpacity { node, opacity },
        );
        Ok(())
    }

    /// Sets resolved visibility when it differs from retained state.
    pub fn set_visibility(
        &mut self,
        node: NodeId,
        visibility: Visibility,
    ) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if self.require_node(node)?.visibility == Some(visibility) {
            return Ok(());
        }
        self.nodes
            .get_mut(&node)
            .expect("node checked above")
            .visibility = Some(visibility);
        self.journal.push_coalesced(
            DirtySlot::Visibility(node),
            Operation::SetVisibility { node, visibility },
        );
        Ok(())
    }

    /// Sets resolved stacking order when it differs from retained state.
    pub fn set_z_order(&mut self, node: NodeId, z_order: i32) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if self.require_node(node)?.z_order == Some(z_order) {
            return Ok(());
        }
        self.nodes
            .get_mut(&node)
            .expect("node checked above")
            .z_order = Some(z_order);
        self.journal.push_coalesced(
            DirtySlot::ZOrder(node),
            Operation::SetZOrder { node, z_order },
        );
        Ok(())
    }

    /// Sets plain-text presentation when it differs from retained state.
    pub fn set_text(&mut self, node: NodeId, content: TextContent) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        content
            .validate()
            .map_err(|error| SceneError::InvalidText { error })?;
        if self.require_node(node)?.text.as_ref() == Some(&content) {
            return Ok(());
        }
        self.nodes.get_mut(&node).expect("node checked above").text = Some(content.clone());
        self.journal
            .push_coalesced(DirtySlot::Text(node), Operation::SetText { node, content });
        Ok(())
    }

    /// Sets a typed property when it differs from retained state.
    pub fn set_property(
        &mut self,
        node: NodeId,
        property: PropertyId,
        value: ProtocolValue,
    ) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if self.require_node(node)?.properties.get(&property) == Some(&value) {
            return Ok(());
        }
        self.nodes
            .get_mut(&node)
            .expect("node checked above")
            .properties
            .insert(property, value.clone());
        self.journal.push_coalesced(
            DirtySlot::Property(node, property),
            Operation::SetProperty {
                node,
                property,
                value,
            },
        );
        Ok(())
    }

    /// Clears a typed property when it is present.
    pub fn clear_property(&mut self, node: NodeId, property: PropertyId) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if self
            .nodes
            .get_mut(&node)
            .ok_or(SceneError::UnknownNode { node })?
            .properties
            .remove(&property)
            .is_none()
        {
            return Ok(());
        }
        self.journal.push_coalesced(
            DirtySlot::Property(node, property),
            Operation::ClearProperty { node, property },
        );
        Ok(())
    }

    /// Sets a node's event subscription mask.
    pub fn set_event_mask(&mut self, node: NodeId, event_mask: u64) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if self.require_node(node)?.event_mask == Some(event_mask) {
            return Ok(());
        }
        self.nodes
            .get_mut(&node)
            .expect("node checked above")
            .event_mask = Some(event_mask);
        self.journal.push_coalesced(
            DirtySlot::EventMask(node),
            Operation::SetEventMask { node, event_mask },
        );
        Ok(())
    }

    /// Sets a node's Host hit-test participation.
    pub fn set_hit_test(
        &mut self,
        node: NodeId,
        behavior: HitTestBehavior,
    ) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if self.require_node(node)?.hit_test == Some(behavior) {
            return Ok(());
        }
        self.nodes
            .get_mut(&node)
            .expect("node checked above")
            .hit_test = Some(behavior);
        self.journal.push_coalesced(
            DirtySlot::HitTest(node),
            Operation::SetHitTest { node, behavior },
        );
        Ok(())
    }

    /// Captures a pointer when it is not already captured by this node.
    pub fn set_pointer_capture(
        &mut self,
        node: NodeId,
        pointer: PointerId,
    ) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if !self
            .nodes
            .get_mut(&node)
            .ok_or(SceneError::UnknownNode { node })?
            .captured_pointers
            .insert(pointer)
        {
            return Ok(());
        }
        self.journal.push_coalesced(
            DirtySlot::Pointer(node, pointer),
            Operation::SetPointerCapture { node, pointer },
        );
        Ok(())
    }

    /// Releases a pointer when it is currently captured by this node.
    pub fn release_pointer_capture(
        &mut self,
        node: NodeId,
        pointer: PointerId,
    ) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        if !self
            .nodes
            .get_mut(&node)
            .ok_or(SceneError::UnknownNode { node })?
            .captured_pointers
            .remove(&pointer)
        {
            return Ok(());
        }
        self.journal.push_coalesced(
            DirtySlot::Pointer(node, pointer),
            Operation::ReleasePointerCapture { node, pointer },
        );
        Ok(())
    }

    /// Queues an element command after preceding visual mutations.
    pub fn invoke_command(
        &mut self,
        node: NodeId,
        command: CommandId,
        arguments: ProtocolValue,
        result: Option<ResultId>,
    ) -> Result<(), SceneError> {
        self.ensure_mutable()?;
        self.require_node(node)?;
        if let Some(result) = result
            && !self.journal.result_ids.insert(result)
        {
            return Err(SceneError::DuplicateResultId { result });
        }
        self.journal.push_barrier(Operation::InvokeCommand {
            node,
            command,
            arguments,
            result,
        });
        Ok(())
    }

    /// Prepares the next snapshot or delta and keeps it pending.
    ///
    /// Returns `Ok(None)` when the accepted scene is idle. Repeated calls while
    /// a packet is pending return [`SceneError::FramePending`]; the caller must
    /// first accept or discard that packet.
    pub fn prepare_frame(
        &mut self,
        viewport_epoch: u32,
    ) -> Result<Option<&FramePacket>, SceneError> {
        if self.pending.is_some() {
            return Err(SceneError::FramePending);
        }
        if !self.needs_snapshot && self.journal.operations.is_empty() {
            return Ok(None);
        }
        let target_revision = self
            .accepted_revision
            .checked_add(1)
            .ok_or(SceneError::RevisionExhausted)?;
        let frame_id = self.next_frame_id;
        self.next_frame_id = self
            .next_frame_id
            .checked_add(1)
            .ok_or(SceneError::FrameIdExhausted)?;
        let mode = if self.needs_snapshot {
            FrameMode::Snapshot
        } else {
            FrameMode::Delta
        };
        let operations = if mode == FrameMode::Snapshot {
            self.snapshot_operations()
        } else {
            self.journal.operations.clone()
        };
        self.pending = Some(FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: self.surface,
                scene_epoch: self.scene_epoch,
                frame_id,
                base_revision: if mode == FrameMode::Snapshot {
                    0
                } else {
                    self.accepted_revision
                },
                target_revision,
                viewport_epoch,
                mode,
            },
            operations,
        });
        Ok(self.pending.as_ref())
    }

    /// Commits the pending packet after renderer acceptance.
    pub fn accept_pending(&mut self, revision: u64) -> Result<(), SceneError> {
        let expected = self
            .pending
            .as_ref()
            .ok_or(SceneError::NoPendingFrame)?
            .header
            .target_revision;
        if revision != expected {
            return Err(SceneError::AcceptedRevisionMismatch {
                expected,
                received: revision,
            });
        }
        self.accepted_revision = revision;
        self.pending = None;
        self.needs_snapshot = false;
        self.journal.clear();
        Ok(())
    }

    /// Discards a prepared packet after a transport or renderer failure.
    ///
    /// Retained mutations remain dirty, so the next preparation retries the
    /// same semantic work with a new diagnostic frame ID.
    pub fn discard_pending(&mut self) -> Result<(), SceneError> {
        self.pending.take().ok_or(SceneError::NoPendingFrame)?;
        Ok(())
    }

    /// Switches the next frame to a full snapshot with a new scene epoch.
    ///
    /// This is called after a renderer returns `NeedSnapshot`. Any pending
    /// delta is discarded, while retained state and queued commands remain.
    pub fn require_snapshot(&mut self) -> Result<(), SceneError> {
        self.pending = None;
        if !self.needs_snapshot {
            self.scene_epoch = self
                .scene_epoch
                .checked_add(1)
                .ok_or(SceneError::SceneEpochExhausted)?;
            self.needs_snapshot = true;
        }
        Ok(())
    }

    fn ensure_mutable(&self) -> Result<(), SceneError> {
        if self.pending.is_some() {
            Err(SceneError::FramePending)
        } else {
            Ok(())
        }
    }

    fn require_node(&self, node: NodeId) -> Result<&SceneNode, SceneError> {
        self.nodes
            .get(&node)
            .ok_or(SceneError::UnknownNode { node })
    }

    fn require_direct_child(&self, parent: NodeId, child: NodeId) -> Result<(), SceneError> {
        self.require_node(parent)?;
        let child_state = self.require_node(child)?;
        if child_state.parent != Some(parent) {
            return Err(SceneError::NotDirectChild { parent, child });
        }
        Ok(())
    }

    fn is_descendant(&self, ancestor: NodeId, candidate: NodeId) -> bool {
        let mut cursor = Some(candidate);
        while let Some(node) = cursor {
            if node == ancestor {
                return true;
            }
            cursor = self.nodes.get(&node).and_then(|entry| entry.parent);
        }
        false
    }

    fn snapshot_operations(&self) -> Vec<Operation> {
        let mut operations = Vec::new();
        for (node, state) in &self.nodes {
            operations.push(Operation::CreateNode {
                node: *node,
                element_type: state.element_type,
            });
        }
        for (parent, state) in &self.nodes {
            for (index, child) in state.children.iter().enumerate() {
                operations.push(Operation::InsertChild {
                    parent: *parent,
                    child: *child,
                    index: index as u32,
                });
            }
        }
        for (node, state) in &self.nodes {
            if let Some(rect) = state.layout {
                operations.push(Operation::SetLayout { node: *node, rect });
            }
            if let Some(paint) = &state.box_paint {
                operations.push(Operation::SetBoxPaint {
                    node: *node,
                    paint: paint.clone(),
                });
            }
            if let Some(clip) = state.clip {
                operations.push(Operation::SetClip { node: *node, clip });
            }
            if let Some(transform) = state.transform {
                operations.push(Operation::SetTransform {
                    node: *node,
                    transform,
                });
            }
            if let Some(opacity) = state.opacity {
                operations.push(Operation::SetOpacity {
                    node: *node,
                    opacity,
                });
            }
            if let Some(visibility) = state.visibility {
                operations.push(Operation::SetVisibility {
                    node: *node,
                    visibility,
                });
            }
            if let Some(z_order) = state.z_order {
                operations.push(Operation::SetZOrder {
                    node: *node,
                    z_order,
                });
            }
            if let Some(content) = &state.text {
                operations.push(Operation::SetText {
                    node: *node,
                    content: content.clone(),
                });
            }
            for (property, value) in &state.properties {
                operations.push(Operation::SetProperty {
                    node: *node,
                    property: *property,
                    value: value.clone(),
                });
            }
            if let Some(event_mask) = state.event_mask {
                operations.push(Operation::SetEventMask {
                    node: *node,
                    event_mask,
                });
            }
            if let Some(behavior) = state.hit_test {
                operations.push(Operation::SetHitTest {
                    node: *node,
                    behavior,
                });
            }
            for pointer in &state.captured_pointers {
                operations.push(Operation::SetPointerCapture {
                    node: *node,
                    pointer: *pointer,
                });
            }
        }
        operations.extend(self.journal.operations.iter().filter_map(|operation| {
            let Operation::InvokeCommand { node, .. } = operation else {
                return None;
            };
            self.nodes.contains_key(node).then(|| operation.clone())
        }));
        operations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FrameSink, RecordingRenderer};
    use whisker_protocol::{
        ApplyResult, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection,
        MeasureTextOverflow, MeasureTextWrap, TextContentError, TextMeasurePayload,
        TextMeasureStyle, ValidationError,
    };

    fn surface() -> SurfaceId {
        SurfaceId::new(1).expect("test surface")
    }

    fn node(value: u64) -> NodeId {
        NodeId::new(value).expect("test node")
    }

    fn element_type(value: u32) -> ElementTypeId {
        ElementTypeId::new(value).expect("test element type")
    }

    fn property(value: u32) -> PropertyId {
        PropertyId::new(value).expect("test property")
    }

    fn pointer(value: u64) -> PointerId {
        PointerId::new(value).expect("test pointer")
    }

    fn command(value: u32) -> CommandId {
        CommandId::new(value).expect("test command")
    }

    fn result_id(value: u64) -> ResultId {
        ResultId::new(value).expect("test result")
    }

    fn text_content(text: &str) -> TextContent {
        TextContent {
            payload: TextMeasurePayload {
                text: text.into(),
                style: TextMeasureStyle {
                    font_families: vec![MeasureFontFamily::System],
                    font_size: 14.0,
                    font_weight: 400,
                    font_style: MeasureFontStyle::Normal,
                    line_height: MeasureLineHeight::Normal,
                    letter_spacing: 0.0,
                },
                locale: None,
                direction: MeasureTextDirection::Auto,
                wrap: MeasureTextWrap::Wrap,
                max_lines: None,
                overflow: MeasureTextOverflow::Clip,
            },
            paint: whisker_protocol::TextPaint::default(),
            prepared_content: None,
        }
    }

    fn prepared(scene: &mut Scene) -> FramePacket {
        scene
            .prepare_frame(7)
            .expect("prepare frame")
            .expect("scene has work")
            .clone()
    }

    fn present_and_accept(scene: &mut Scene, renderer: &mut RecordingRenderer) -> FramePacket {
        let packet = prepared(scene);
        let outcome = renderer.present(&packet).expect("valid engine packet");
        let revision = packet.header.target_revision;
        assert_eq!(outcome, ApplyResult::Accepted { revision });
        scene.accept_pending(revision).expect("matching acceptance");
        packet
    }

    fn initialized_scene() -> (Scene, RecordingRenderer, NodeId, NodeId) {
        let mut scene = Scene::new(surface());
        let root = scene.create_node(element_type(1)).expect("create root");
        let child = scene.create_node(element_type(2)).expect("create child");
        scene.insert_child(root, child, 0).expect("attach child");
        let mut renderer = RecordingRenderer::new(surface());
        present_and_accept(&mut scene, &mut renderer);
        (scene, renderer, root, child)
    }

    #[test]
    fn initial_snapshot_rebuilds_complete_retained_state() {
        let mut scene = Scene::new(surface());
        assert_eq!(scene.surface(), surface());
        assert_eq!(scene.scene_epoch(), 1);
        assert_eq!(scene.accepted_revision(), 0);
        assert!(scene.has_pending_work());

        let root = scene.create_node(element_type(1)).expect("create root");
        let child = scene.create_node(element_type(2)).expect("create child");
        scene.insert_child(root, child, 0).expect("attach child");
        let rect = LayoutRect {
            x: 1.0,
            y: 2.0,
            width: 300.0,
            height: 200.0,
        };
        scene.set_layout(root, rect).expect("layout");
        scene
            .set_transform(root, Transform::IDENTITY)
            .expect("transform");
        scene.set_opacity(root, 0.75).expect("opacity");
        scene
            .set_visibility(root, Visibility::Visible)
            .expect("visibility");
        scene.set_z_order(root, -1).expect("z order");
        let text = text_content("hello");
        scene.set_text(root, text.clone()).expect("text");
        scene
            .set_property(root, property(1), ProtocolValue::String("red".into()))
            .expect("property");
        scene.set_event_mask(root, 3).expect("event mask");
        scene
            .set_hit_test(root, HitTestBehavior::BoxOnly)
            .expect("hit test");
        scene
            .set_pointer_capture(root, pointer(1))
            .expect("pointer capture");
        scene
            .invoke_command(
                root,
                command(1),
                ProtocolValue::Array(Vec::new()),
                Some(result_id(1)),
            )
            .expect("command");

        let root_state = scene.node(root).expect("root state");
        assert_eq!(root_state.element_type(), element_type(1));
        assert_eq!(root_state.parent(), None);
        assert_eq!(root_state.children(), &[child]);
        assert_eq!(root_state.text(), Some(&text));
        assert_eq!(scene.node(child).expect("child state").parent(), Some(root));
        assert_eq!(scene.node_count(), 2);

        let mut renderer = RecordingRenderer::new(surface());
        let packet = present_and_accept(&mut scene, &mut renderer);
        assert_eq!(packet.header.mode, FrameMode::Snapshot);
        assert_eq!(packet.header.base_revision, 0);
        assert_eq!(packet.header.target_revision, 1);
        assert_eq!(packet.header.viewport_epoch, 7);
        assert_eq!(renderer.projection().node_count(), 2);
        assert_eq!(scene.accepted_revision(), 1);
        assert!(!scene.has_pending_work());
        assert_eq!(scene.prepare_frame(8).expect("idle frame"), None);
    }

    #[test]
    fn equal_values_are_idle_and_delta_values_coalesce_between_barriers() {
        let (mut scene, mut renderer, root, _) = initialized_scene();
        let first = LayoutRect {
            width: 10.0,
            height: 20.0,
            ..LayoutRect::default()
        };
        let second = LayoutRect {
            width: 30.0,
            height: 40.0,
            ..LayoutRect::default()
        };
        scene.set_layout(root, first).expect("first layout");
        scene.set_layout(root, second).expect("second layout");
        scene.set_layout(root, second).expect("equal layout");
        scene
            .set_transform(root, Transform::IDENTITY)
            .expect("transform");
        scene
            .set_transform(root, Transform::IDENTITY)
            .expect("equal transform");
        scene.set_opacity(root, 0.2).expect("first opacity");
        scene.set_opacity(root, 0.4).expect("coalesced opacity");
        scene.set_opacity(root, 0.4).expect("equal opacity");
        scene
            .set_visibility(root, Visibility::Hidden)
            .expect("visibility");
        scene
            .set_visibility(root, Visibility::Hidden)
            .expect("equal visibility");
        scene.set_z_order(root, 4).expect("z order");
        scene.set_z_order(root, 4).expect("equal z order");
        let text = text_content("updated");
        scene.set_text(root, text.clone()).expect("text");
        scene.set_text(root, text).expect("equal text");
        scene
            .set_property(root, property(1), ProtocolValue::I64(1))
            .expect("first property");
        scene
            .set_property(root, property(1), ProtocolValue::I64(2))
            .expect("coalesced property");
        scene
            .set_property(root, property(1), ProtocolValue::I64(2))
            .expect("equal property");
        scene.set_event_mask(root, 7).expect("event mask");
        scene.set_event_mask(root, 7).expect("equal event mask");
        scene
            .set_hit_test(root, HitTestBehavior::DescendantsOnly)
            .expect("hit test");
        scene
            .set_hit_test(root, HitTestBehavior::DescendantsOnly)
            .expect("equal hit test");
        scene
            .set_pointer_capture(root, pointer(1))
            .expect("capture");
        scene
            .set_pointer_capture(root, pointer(1))
            .expect("equal capture");

        scene
            .invoke_command(root, command(1), ProtocolValue::Null, None)
            .expect("barrier command");
        scene.set_opacity(root, 0.8).expect("post-barrier opacity");

        let packet = present_and_accept(&mut scene, &mut renderer);
        assert_eq!(packet.header.mode, FrameMode::Delta);
        assert_eq!(packet.header.base_revision, 1);
        assert_eq!(packet.header.target_revision, 2);
        assert_eq!(
            packet
                .operations
                .iter()
                .filter(|operation| matches!(operation, Operation::SetLayout { .. }))
                .count(),
            1
        );
        assert_eq!(
            packet
                .operations
                .iter()
                .filter(|operation| matches!(operation, Operation::SetOpacity { .. }))
                .count(),
            2
        );
        assert!(!scene.has_pending_work());
    }

    #[test]
    fn clear_and_pointer_release_coalesce_and_skip_absent_values() {
        let (mut scene, mut renderer, root, _) = initialized_scene();
        let property = property(1);
        let pointer = pointer(1);
        scene.clear_property(root, property).expect("absent clear");
        scene
            .release_pointer_capture(root, pointer)
            .expect("absent release");
        assert!(!scene.has_pending_work());

        scene
            .set_property(root, property, ProtocolValue::Bool(true))
            .expect("set property");
        scene
            .clear_property(root, property)
            .expect("clear property");
        scene
            .set_pointer_capture(root, pointer)
            .expect("capture pointer");
        scene
            .release_pointer_capture(root, pointer)
            .expect("release pointer");
        let packet = present_and_accept(&mut scene, &mut renderer);
        assert_eq!(
            packet.operations,
            vec![
                Operation::ClearProperty {
                    node: root,
                    property,
                },
                Operation::ReleasePointerCapture {
                    node: root,
                    pointer,
                },
            ]
        );
    }

    #[test]
    fn structural_delta_supports_move_remove_reinsert_and_subtree_delete() {
        let (mut scene, mut renderer, root, child) = initialized_scene();
        let sibling = scene.create_node(element_type(3)).expect("sibling");
        let grandchild = scene.create_node(element_type(4)).expect("grandchild");
        scene
            .insert_child(root, sibling, 1)
            .expect("insert sibling");
        scene.move_child(root, sibling, 0).expect("move sibling");
        scene.remove_child(root, child).expect("remove child");
        scene.insert_child(root, child, 1).expect("reinsert child");
        scene
            .insert_child(child, grandchild, 0)
            .expect("insert grandchild");
        scene.delete_node(child).expect("delete attached subtree");
        assert_eq!(scene.node_count(), 2);
        assert_eq!(scene.node(root).expect("root").children(), &[sibling]);
        assert!(scene.node(child).is_none());
        assert!(scene.node(grandchild).is_none());
        present_and_accept(&mut scene, &mut renderer);

        scene.delete_node(root).expect("delete unattached root");
        present_and_accept(&mut scene, &mut renderer);
        assert_eq!(scene.node_count(), 0);
        assert_eq!(renderer.projection().node_count(), 0);
    }

    #[test]
    fn pending_frame_requires_explicit_accept_or_discard() {
        let (mut scene, _, root, _) = initialized_scene();
        scene.set_opacity(root, 0.5).expect("dirty scene");
        let first = prepared(&mut scene);
        assert!(scene.has_pending_work());
        assert_eq!(scene.prepare_frame(7), Err(SceneError::FramePending));
        assert_eq!(scene.set_opacity(root, 0.7), Err(SceneError::FramePending));
        assert_eq!(
            scene.set_text(root, text_content("pending")),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.create_node(element_type(3)),
            Err(SceneError::FramePending)
        );
        assert_eq!(scene.delete_node(root), Err(SceneError::FramePending));
        assert_eq!(
            scene.insert_child(root, root, 0),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.remove_child(root, root),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.move_child(root, root, 0),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.set_layout(root, LayoutRect::default()),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.set_transform(root, Transform::IDENTITY),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.set_visibility(root, Visibility::Visible),
            Err(SceneError::FramePending)
        );
        assert_eq!(scene.set_z_order(root, 0), Err(SceneError::FramePending));
        assert_eq!(
            scene.set_property(root, property(1), ProtocolValue::Null),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.clear_property(root, property(1)),
            Err(SceneError::FramePending)
        );
        assert_eq!(scene.set_event_mask(root, 0), Err(SceneError::FramePending));
        assert_eq!(
            scene.set_hit_test(root, HitTestBehavior::Auto),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.set_pointer_capture(root, pointer(1)),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.release_pointer_capture(root, pointer(1)),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.invoke_command(root, command(1), ProtocolValue::Null, None),
            Err(SceneError::FramePending)
        );
        assert_eq!(
            scene.accept_pending(first.header.target_revision + 1),
            Err(SceneError::AcceptedRevisionMismatch {
                expected: first.header.target_revision,
                received: first.header.target_revision + 1,
            })
        );
        scene.discard_pending().expect("discard retry");
        assert_eq!(scene.discard_pending(), Err(SceneError::NoPendingFrame));
        let retry = prepared(&mut scene);
        assert_ne!(first.header.frame_id, retry.header.frame_id);
        assert_eq!(first.operations, retry.operations);
        scene
            .accept_pending(retry.header.target_revision)
            .expect("accept retry");
        assert_eq!(scene.accept_pending(3), Err(SceneError::NoPendingFrame));
    }

    #[test]
    fn need_snapshot_rotates_epoch_and_rebuilds_current_state() {
        let (mut scene, _, root, child) = initialized_scene();
        scene.set_opacity(root, 0.5).expect("delta");
        let delta = prepared(&mut scene);
        let mut empty_renderer = RecordingRenderer::new(surface());
        assert_eq!(
            empty_renderer.present(&delta),
            Ok(ApplyResult::NeedSnapshot { host_revision: 0 })
        );

        scene.require_snapshot().expect("recovery snapshot");
        assert_eq!(scene.scene_epoch(), 2);
        scene.require_snapshot().expect("already snapshot mode");
        assert_eq!(scene.scene_epoch(), 2);
        let snapshot = prepared(&mut scene);
        assert_eq!(snapshot.header.mode, FrameMode::Snapshot);
        assert_eq!(snapshot.header.scene_epoch, 2);
        assert!(snapshot.operations.iter().any(
            |operation| matches!(operation, Operation::InsertChild { parent, child: value, .. } if *parent == root && *value == child)
        ));
        let outcome = empty_renderer
            .present(&snapshot)
            .expect("recovery snapshot accepted");
        let revision = snapshot.header.target_revision;
        assert_eq!(outcome, ApplyResult::Accepted { revision });
        scene.accept_pending(revision).expect("accept snapshot");
        assert_eq!(empty_renderer.projection().node_count(), 2);
    }

    #[test]
    fn deleted_command_target_is_omitted_from_initial_snapshot() {
        let mut scene = Scene::new(surface());
        let doomed = scene.create_node(element_type(1)).expect("doomed node");
        scene
            .invoke_command(doomed, command(1), ProtocolValue::Null, Some(result_id(1)))
            .expect("queued command");
        scene.delete_node(doomed).expect("delete command target");
        let snapshot = prepared(&mut scene);
        assert!(snapshot.operations.is_empty());
    }

    #[test]
    fn mutation_validation_preserves_the_retained_tree() {
        let (mut scene, _, root, child) = initialized_scene();
        let missing = node(99);
        let unattached = scene.create_node(element_type(3)).expect("unattached");

        assert_eq!(
            scene.insert_child(missing, unattached, 0),
            Err(SceneError::UnknownNode { node: missing })
        );
        assert_eq!(
            scene.insert_child(root, missing, 0),
            Err(SceneError::UnknownNode { node: missing })
        );
        assert_eq!(
            scene.insert_child(root, child, 0),
            Err(SceneError::ChildAlreadyAttached {
                child,
                parent: root,
            })
        );
        assert_eq!(
            scene.insert_child(root, unattached, 2),
            Err(SceneError::ChildIndexOutOfBounds {
                parent: root,
                index: 2,
                len: 1,
            })
        );
        assert_eq!(
            scene.insert_child(unattached, unattached, 0),
            Err(SceneError::TreeCycle {
                parent: unattached,
                child: unattached,
            })
        );
        assert_eq!(
            scene.insert_child(child, root, 0),
            Err(SceneError::TreeCycle {
                parent: child,
                child: root,
            })
        );
        assert_eq!(
            scene.remove_child(root, unattached),
            Err(SceneError::NotDirectChild {
                parent: root,
                child: unattached,
            })
        );
        assert_eq!(
            scene.move_child(root, unattached, 0),
            Err(SceneError::NotDirectChild {
                parent: root,
                child: unattached,
            })
        );
        assert_eq!(
            scene.move_child(root, child, 1),
            Err(SceneError::ChildIndexOutOfBounds {
                parent: root,
                index: 1,
                len: 0,
            })
        );
        assert_eq!(scene.node(root).expect("root").children(), &[child]);
    }

    #[test]
    fn unknown_nodes_are_rejected_by_every_mutation_family() {
        let (mut scene, _, _, _) = initialized_scene();
        let missing = node(99);
        let operations = [
            scene.delete_node(missing),
            scene.set_layout(missing, LayoutRect::default()),
            scene.set_transform(missing, Transform::IDENTITY),
            scene.set_opacity(missing, 1.0),
            scene.set_visibility(missing, Visibility::Visible),
            scene.set_z_order(missing, 0),
            scene.set_text(missing, text_content("missing")),
            scene.set_property(missing, property(1), ProtocolValue::Null),
            scene.clear_property(missing, property(1)),
            scene.set_event_mask(missing, 0),
            scene.set_hit_test(missing, HitTestBehavior::Auto),
            scene.set_pointer_capture(missing, pointer(1)),
            scene.release_pointer_capture(missing, pointer(1)),
            scene.invoke_command(missing, command(1), ProtocolValue::Null, None),
            scene.remove_child(missing, node(1)),
            scene.remove_child(node(1), missing),
        ];
        for outcome in operations {
            assert_eq!(outcome, Err(SceneError::UnknownNode { node: missing }));
        }
    }

    #[test]
    fn numeric_and_result_validation_reject_invalid_values() {
        let (mut scene, _, root, _) = initialized_scene();
        let invalid_layout = LayoutRect {
            width: f32::NAN,
            ..LayoutRect::default()
        };
        assert_eq!(
            scene.set_layout(root, invalid_layout),
            Err(SceneError::NonFiniteNumber)
        );
        let mut invalid_text = text_content("invalid");
        invalid_text.payload.style.font_families.clear();
        assert_eq!(
            scene.set_text(root, invalid_text),
            Err(SceneError::InvalidText {
                error: TextContentError::InvalidMeasurement(
                    whisker_protocol::MeasurementPayloadError::InvalidFontFamily,
                ),
            })
        );
        let mut invalid_transform = Transform::IDENTITY;
        invalid_transform.0[0] = f32::INFINITY;
        assert_eq!(
            scene.set_transform(root, invalid_transform),
            Err(SceneError::NonFiniteNumber)
        );
        assert_eq!(
            scene.set_opacity(root, -0.1),
            Err(SceneError::InvalidOpacity { opacity: -0.1 })
        );
        let nan_error = scene.set_opacity(root, f32::NAN).expect_err("NaN opacity");
        assert_eq!(format!("{nan_error:?}"), "InvalidOpacity { opacity: NaN }");

        let result = result_id(1);
        scene
            .invoke_command(root, command(1), ProtocolValue::Null, Some(result))
            .expect("first result");
        assert_eq!(
            scene.invoke_command(root, command(1), ProtocolValue::Null, Some(result)),
            Err(SceneError::DuplicateResultId { result })
        );
        assert!(
            SceneError::NonFiniteNumber
                .to_string()
                .starts_with("Whisker scene error:")
        );
        let as_error: &dyn Error = &SceneError::NonFiniteNumber;
        assert!(as_error.source().is_none());
    }

    #[test]
    fn allocation_and_frame_counters_report_exhaustion() {
        let mut scene = Scene::new(surface());
        scene.next_node_id = 0;
        assert_eq!(
            scene.create_node(element_type(1)),
            Err(SceneError::NodeIdExhausted)
        );

        let mut scene = Scene::new(surface());
        scene.accepted_revision = u64::MAX;
        assert_eq!(scene.prepare_frame(1), Err(SceneError::RevisionExhausted));

        let mut scene = Scene::new(surface());
        scene.next_frame_id = u64::MAX;
        assert_eq!(scene.prepare_frame(1), Err(SceneError::FrameIdExhausted));

        let (mut scene, _, _, _) = initialized_scene();
        scene.scene_epoch = u32::MAX;
        assert_eq!(
            scene.require_snapshot(),
            Err(SceneError::SceneEpochExhausted)
        );
    }

    #[test]
    fn result_identifier_can_be_reused_after_acceptance() {
        let (mut scene, mut renderer, root, _) = initialized_scene();
        let result = result_id(1);
        scene
            .invoke_command(root, command(1), ProtocolValue::Null, Some(result))
            .expect("first frame result");
        present_and_accept(&mut scene, &mut renderer);
        scene
            .invoke_command(root, command(1), ProtocolValue::Null, Some(result))
            .expect("next frame result");
    }

    #[test]
    fn recording_renderer_surfaces_protocol_validation_failures() {
        let mut scene = Scene::new(surface());
        let packet = prepared(&mut scene);
        let mut wrong_renderer =
            RecordingRenderer::new(SurfaceId::new(2).expect("different recording surface"));
        assert_eq!(
            wrong_renderer.present(&packet),
            Err(ValidationError::SurfaceMismatch {
                expected: SurfaceId::new(2).expect("different recording surface"),
                received: surface(),
            })
        );
    }

    #[test]
    fn hit_testing_respects_z_order_visibility_clip_and_pointer_capture() {
        let mut scene = Scene::new(surface());
        let root = scene.create_node(element_type(1)).unwrap();
        let back = scene.create_node(element_type(1)).unwrap();
        let front = scene.create_node(element_type(1)).unwrap();
        scene.insert_child(root, back, 0).unwrap();
        scene.insert_child(root, front, 1).unwrap();
        scene
            .set_layout(
                root,
                LayoutRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            )
            .unwrap();
        for child in [back, front] {
            scene
                .set_layout(
                    child,
                    LayoutRect {
                        x: 0.0,
                        y: 0.0,
                        width: 50.0,
                        height: 50.0,
                    },
                )
                .unwrap();
        }
        scene.set_z_order(back, 1).unwrap();
        scene.set_z_order(front, 2).unwrap();
        let point = InputPoint { x: 10.0, y: 10.0 };
        assert_eq!(scene.hit_test(root, point), Ok(Some(front)));

        scene.set_visibility(front, Visibility::Hidden).unwrap();
        assert_eq!(scene.hit_test(root, point), Ok(Some(back)));
        scene
            .set_hit_test(back, HitTestBehavior::DescendantsOnly)
            .unwrap();
        assert_eq!(scene.hit_test(root, point), Ok(Some(root)));

        scene
            .set_layout(
                back,
                LayoutRect {
                    x: 120.0,
                    y: 0.0,
                    width: 50.0,
                    height: 50.0,
                },
            )
            .unwrap();
        scene.set_hit_test(back, HitTestBehavior::Auto).unwrap();
        scene
            .set_clip(
                root,
                BoxClip {
                    horizontal: OverflowClip::Hidden,
                    vertical: OverflowClip::Visible,
                },
            )
            .unwrap();
        let outside = InputPoint { x: 130.0, y: 10.0 };
        assert_eq!(scene.hit_test(root, outside), Ok(None));
        scene
            .set_clip(
                root,
                BoxClip {
                    horizontal: OverflowClip::Visible,
                    vertical: OverflowClip::Visible,
                },
            )
            .unwrap();
        assert_eq!(scene.hit_test(root, outside), Ok(Some(back)));

        let pointer = pointer(9);
        scene.set_pointer_capture(back, pointer).unwrap();
        assert_eq!(scene.pointer_capture_target(pointer), Some(back));
        scene.release_pointer_capture(back, pointer).unwrap();
        assert_eq!(scene.pointer_capture_target(pointer), None);
        assert_eq!(
            scene.hit_test(node(999), point),
            Err(SceneError::UnknownNode { node: node(999) })
        );
    }
}
