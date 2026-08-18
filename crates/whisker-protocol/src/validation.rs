//! Transactional reference validation for semantic frame packets.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use crate::{
    ElementTypeId, FrameMode, FramePacket, NodeId, Operation, PROTOCOL_MAJOR, ResultId, SurfaceId,
};

/// Minimal retained state for one node in a reference projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeProjection {
    element_type: ElementTypeId,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
}

impl NodeProjection {
    /// Returns the negotiated type used to create this node.
    pub const fn element_type(&self) -> ElementTypeId {
        self.element_type
    }

    /// Returns the current logical parent, if attached.
    pub const fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// Returns children in presentation order.
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }
}

/// Result of attempting to apply a well-formed packet to a projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyResult {
    /// The complete transaction was accepted.
    Accepted {
        /// Newly accepted scene revision.
        revision: u64,
    },
    /// A delta did not continue the receiver state and a snapshot is needed.
    NeedSnapshot {
        /// Revision currently held by the receiver.
        host_revision: u64,
    },
}

/// A malformed frame transaction.
#[derive(Clone, Debug, PartialEq)]
pub enum ValidationError {
    /// Packet uses a protocol major this implementation cannot interpret.
    UnsupportedProtocolMajor {
        /// Received major version.
        received: u16,
    },
    /// Packet targets a different surface.
    SurfaceMismatch {
        /// Surface owned by the projection.
        expected: SurfaceId,
        /// Surface declared by the packet.
        received: SurfaceId,
    },
    /// Snapshot did not start from the empty revision.
    SnapshotBaseRevision {
        /// Invalid base revision.
        received: u64,
    },
    /// Target revision did not advance beyond its base.
    RevisionDidNotAdvance {
        /// Packet base revision.
        base: u64,
        /// Packet target revision.
        target: u64,
    },
    /// Snapshot reused the currently accepted scene epoch.
    SnapshotReusedEpoch {
        /// Reused scene epoch.
        epoch: u32,
    },
    /// Node identifier was created more than once in one epoch.
    DuplicateNode {
        /// Duplicate identifier.
        node: NodeId,
    },
    /// An operation referenced a node that does not exist at that point.
    UnknownNode {
        /// Missing identifier.
        node: NodeId,
    },
    /// Insert attempted to attach a child that already has a parent.
    ChildAlreadyAttached {
        /// Child identifier.
        child: NodeId,
        /// Existing parent.
        parent: NodeId,
    },
    /// A parent/child relationship did not match the operation.
    NotDirectChild {
        /// Expected parent.
        parent: NodeId,
        /// Node that was not its direct child.
        child: NodeId,
    },
    /// Child position exceeds the resulting child-list length.
    ChildIndexOutOfBounds {
        /// Parent whose list was addressed.
        parent: NodeId,
        /// Requested position.
        index: u32,
        /// Maximum accepted insertion position.
        len: usize,
    },
    /// Insertion would make a node its own ancestor.
    TreeCycle {
        /// Parent requested by the operation.
        parent: NodeId,
        /// Child whose subtree already contains the parent.
        child: NodeId,
    },
    /// Opacity was non-finite or outside `0.0..=1.0`.
    InvalidOpacity {
        /// Invalid opacity.
        opacity: f32,
    },
    /// A geometry or transform component was NaN or infinite.
    NonFiniteNumber,
    /// One packet reused a command result identifier.
    DuplicateResultId {
        /// Duplicate result correlation.
        result: ResultId,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid Whisker frame: {self:?}")
    }
}

impl Error for ValidationError {}

/// Reference retained-tree receiver for one surface.
///
/// This type validates semantic packets before a packed encoder or Host
/// backend exists. It intentionally stores only structure: renderers may keep
/// richer property state alongside the same accepted revision.
#[derive(Clone, Debug)]
pub struct SceneProjection {
    surface: SurfaceId,
    scene_epoch: Option<u32>,
    revision: u64,
    nodes: HashMap<NodeId, NodeProjection>,
    allocated_nodes: HashSet<NodeId>,
}

impl SceneProjection {
    /// Creates an empty receiver for a surface.
    pub fn new(surface: SurfaceId) -> Self {
        Self {
            surface,
            scene_epoch: None,
            revision: 0,
            nodes: HashMap::new(),
            allocated_nodes: HashSet::new(),
        }
    }

    /// Returns the surface accepted by this receiver.
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the current scene epoch after the first snapshot.
    pub const fn scene_epoch(&self) -> Option<u32> {
        self.scene_epoch
    }

    /// Returns the last accepted revision.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the number of retained nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns retained structural state for a node.
    pub fn node(&self, node: NodeId) -> Option<&NodeProjection> {
        self.nodes.get(&node)
    }

    /// Validates and atomically applies a packet.
    ///
    /// Revision or epoch drift on a delta returns [`ApplyResult::NeedSnapshot`]
    /// without classifying the sender as malformed. Every
    /// [`ValidationError`] also leaves this projection unchanged.
    pub fn apply(&mut self, packet: &FramePacket) -> Result<ApplyResult, ValidationError> {
        self.validate_header(packet)?;

        if packet.header.mode == FrameMode::Delta
            && (self.scene_epoch != Some(packet.header.scene_epoch)
                || self.revision != packet.header.base_revision)
        {
            return Ok(ApplyResult::NeedSnapshot {
                host_revision: self.revision,
            });
        }

        let mut next = if packet.header.mode == FrameMode::Snapshot {
            Self::new(self.surface)
        } else {
            self.clone()
        };
        next.scene_epoch = Some(packet.header.scene_epoch);

        let mut result_ids = HashSet::new();
        for operation in &packet.operations {
            next.apply_operation(operation, &mut result_ids)?;
        }

        next.revision = packet.header.target_revision;
        *self = next;
        Ok(ApplyResult::Accepted {
            revision: self.revision,
        })
    }

    fn validate_header(&self, packet: &FramePacket) -> Result<(), ValidationError> {
        let header = packet.header;
        if header.version.major != PROTOCOL_MAJOR {
            return Err(ValidationError::UnsupportedProtocolMajor {
                received: header.version.major,
            });
        }
        if header.surface != self.surface {
            return Err(ValidationError::SurfaceMismatch {
                expected: self.surface,
                received: header.surface,
            });
        }
        if header.target_revision <= header.base_revision {
            return Err(ValidationError::RevisionDidNotAdvance {
                base: header.base_revision,
                target: header.target_revision,
            });
        }
        if header.mode == FrameMode::Snapshot {
            if header.base_revision != 0 {
                return Err(ValidationError::SnapshotBaseRevision {
                    received: header.base_revision,
                });
            }
            if self.scene_epoch == Some(header.scene_epoch) {
                return Err(ValidationError::SnapshotReusedEpoch {
                    epoch: header.scene_epoch,
                });
            }
        }
        Ok(())
    }

    fn apply_operation(
        &mut self,
        operation: &Operation,
        result_ids: &mut HashSet<ResultId>,
    ) -> Result<(), ValidationError> {
        match operation {
            Operation::CreateNode { node, element_type } => {
                if !self.allocated_nodes.insert(*node) {
                    return Err(ValidationError::DuplicateNode { node: *node });
                }
                self.nodes.insert(
                    *node,
                    NodeProjection {
                        element_type: *element_type,
                        parent: None,
                        children: Vec::new(),
                    },
                );
            }
            Operation::DeleteNode { node } => self.delete_subtree(*node)?,
            Operation::InsertChild {
                parent,
                child,
                index,
            } => self.insert_child(*parent, *child, *index)?,
            Operation::RemoveChild { parent, child } => self.remove_child(*parent, *child)?,
            Operation::MoveChild {
                parent,
                child,
                index,
            } => self.move_child(*parent, *child, *index)?,
            Operation::SetLayout { node, rect } => {
                self.require_node(*node)?;
                if ![rect.x, rect.y, rect.width, rect.height]
                    .into_iter()
                    .all(f32::is_finite)
                {
                    return Err(ValidationError::NonFiniteNumber);
                }
            }
            Operation::SetTransform { node, transform } => {
                self.require_node(*node)?;
                if !transform.0.into_iter().all(f32::is_finite) {
                    return Err(ValidationError::NonFiniteNumber);
                }
            }
            Operation::SetOpacity { node, opacity } => {
                self.require_node(*node)?;
                if !opacity.is_finite() || !(0.0..=1.0).contains(opacity) {
                    return Err(ValidationError::InvalidOpacity { opacity: *opacity });
                }
            }
            Operation::InvokeCommand { node, result, .. } => {
                self.require_node(*node)?;
                if let Some(result) = result
                    && !result_ids.insert(*result)
                {
                    return Err(ValidationError::DuplicateResultId { result: *result });
                }
            }
            operation => {
                if let Some(node) = operation.target_node() {
                    self.require_node(node)?;
                }
            }
        }
        Ok(())
    }

    fn require_node(&self, node: NodeId) -> Result<&NodeProjection, ValidationError> {
        self.nodes
            .get(&node)
            .ok_or(ValidationError::UnknownNode { node })
    }

    fn insert_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: u32,
    ) -> Result<(), ValidationError> {
        self.require_node(parent)?;
        let child_state = self.require_node(child)?;
        if let Some(existing) = child_state.parent {
            return Err(ValidationError::ChildAlreadyAttached {
                child,
                parent: existing,
            });
        }
        if parent == child || self.is_descendant(child, parent) {
            return Err(ValidationError::TreeCycle { parent, child });
        }

        let len = self.require_node(parent)?.children.len();
        let index = usize::try_from(index).expect("u32 always fits supported Rust targets");
        if index > len {
            return Err(ValidationError::ChildIndexOutOfBounds {
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
        Ok(())
    }

    fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), ValidationError> {
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
        Ok(())
    }

    fn move_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: u32,
    ) -> Result<(), ValidationError> {
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

        let index = usize::try_from(index).expect("u32 always fits supported Rust targets");
        if index > children.len() {
            return Err(ValidationError::ChildIndexOutOfBounds {
                parent,
                index: index as u32,
                len: children.len(),
            });
        }
        children.insert(index, child);
        Ok(())
    }

    fn require_direct_child(&self, parent: NodeId, child: NodeId) -> Result<(), ValidationError> {
        self.require_node(parent)?;
        let child_state = self.require_node(child)?;
        if child_state.parent != Some(parent) {
            return Err(ValidationError::NotDirectChild { parent, child });
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

    fn delete_subtree(&mut self, node: NodeId) -> Result<(), ValidationError> {
        let state = self.require_node(node)?.clone();
        if let Some(parent) = state.parent {
            let siblings = &mut self
                .nodes
                .get_mut(&parent)
                .expect("attached parent exists")
                .children;
            siblings.retain(|candidate| *candidate != node);
        }

        let mut pending = vec![node];
        while let Some(current) = pending.pop() {
            let state = self
                .nodes
                .remove(&current)
                .expect("subtree children exist while parent exists");
            pending.extend(state.children);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ElementTypeId, FrameHeader, LayoutRect, Operation, ProtocolVersion, ResultId};

    fn surface() -> SurfaceId {
        SurfaceId::new(1).expect("test surface")
    }

    fn node(value: u64) -> NodeId {
        NodeId::new(value).expect("test node")
    }

    fn element_type() -> ElementTypeId {
        ElementTypeId::new(1).expect("test element type")
    }

    fn packet(
        mode: FrameMode,
        epoch: u32,
        base: u64,
        target: u64,
        operations: Vec<Operation>,
    ) -> FramePacket {
        FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: surface(),
                scene_epoch: epoch,
                frame_id: target,
                base_revision: base,
                target_revision: target,
                viewport_epoch: 1,
                mode,
            },
            operations,
        }
    }

    fn initial_tree() -> (SceneProjection, NodeId, NodeId) {
        let root = node(1);
        let child = node(2);
        let mut scene = SceneProjection::new(surface());
        let result = scene
            .apply(&packet(
                FrameMode::Snapshot,
                1,
                0,
                1,
                vec![
                    Operation::CreateNode {
                        node: root,
                        element_type: element_type(),
                    },
                    Operation::CreateNode {
                        node: child,
                        element_type: element_type(),
                    },
                    Operation::InsertChild {
                        parent: root,
                        child,
                        index: 0,
                    },
                ],
            ))
            .expect("valid snapshot");
        assert_eq!(result, ApplyResult::Accepted { revision: 1 });
        (scene, root, child)
    }

    #[test]
    fn snapshot_builds_a_retained_tree() {
        let (scene, root, child) = initial_tree();
        assert_eq!(scene.scene_epoch(), Some(1));
        assert_eq!(scene.revision(), 1);
        assert_eq!(scene.node_count(), 2);
        assert_eq!(scene.node(root).expect("root").children(), &[child]);
        assert_eq!(scene.node(child).expect("child").parent(), Some(root));
    }

    #[test]
    fn revision_drift_requests_a_snapshot_without_mutation() {
        let (mut scene, root, _) = initial_tree();
        let result = scene
            .apply(&packet(
                FrameMode::Delta,
                1,
                7,
                8,
                vec![Operation::SetOpacity {
                    node: root,
                    opacity: 0.5,
                }],
            ))
            .expect("revision drift is recoverable");
        assert_eq!(result, ApplyResult::NeedSnapshot { host_revision: 1 });
        assert_eq!(scene.revision(), 1);
        assert_eq!(scene.node_count(), 2);
    }

    #[test]
    fn malformed_operation_rolls_back_the_complete_packet() {
        let (mut scene, root, child) = initial_tree();
        let new_child = node(3);
        let error = scene
            .apply(&packet(
                FrameMode::Delta,
                1,
                1,
                2,
                vec![
                    Operation::CreateNode {
                        node: new_child,
                        element_type: element_type(),
                    },
                    Operation::InsertChild {
                        parent: root,
                        child: new_child,
                        index: 1,
                    },
                    Operation::SetLayout {
                        node: child,
                        rect: LayoutRect {
                            width: f32::NAN,
                            ..LayoutRect::default()
                        },
                    },
                ],
            ))
            .expect_err("NaN must reject the transaction");
        assert_eq!(error, ValidationError::NonFiniteNumber);
        assert_eq!(scene.revision(), 1);
        assert_eq!(scene.node_count(), 2);
        assert_eq!(scene.node(root).expect("root").children(), &[child]);
        assert_eq!(scene.node(new_child), None);
    }

    #[test]
    fn deleting_a_node_invalidates_its_subtree() {
        let (mut scene, root, child) = initial_tree();
        let grandchild = node(3);
        scene
            .apply(&packet(
                FrameMode::Delta,
                1,
                1,
                2,
                vec![
                    Operation::CreateNode {
                        node: grandchild,
                        element_type: element_type(),
                    },
                    Operation::InsertChild {
                        parent: child,
                        child: grandchild,
                        index: 0,
                    },
                    Operation::DeleteNode { node: child },
                ],
            ))
            .expect("valid subtree deletion");
        assert_eq!(scene.node_count(), 1);
        assert!(scene.node(child).is_none());
        assert!(scene.node(grandchild).is_none());
        assert!(scene.node(root).expect("root").children().is_empty());
    }

    #[test]
    fn deleted_node_id_cannot_be_reused_in_the_same_epoch() {
        let (mut scene, _, child) = initial_tree();
        scene
            .apply(&packet(
                FrameMode::Delta,
                1,
                1,
                2,
                vec![Operation::DeleteNode { node: child }],
            ))
            .expect("valid deletion");

        let error = scene
            .apply(&packet(
                FrameMode::Delta,
                1,
                2,
                3,
                vec![Operation::CreateNode {
                    node: child,
                    element_type: element_type(),
                }],
            ))
            .expect_err("retired ID must remain unavailable");
        assert_eq!(error, ValidationError::DuplicateNode { node: child });
        assert_eq!(scene.revision(), 2);
        assert!(scene.node(child).is_none());
    }

    #[test]
    fn insertion_rejects_a_cycle() {
        let (mut scene, root, child) = initial_tree();
        let error = scene
            .apply(&packet(
                FrameMode::Delta,
                1,
                1,
                2,
                vec![Operation::InsertChild {
                    parent: child,
                    child: root,
                    index: 0,
                }],
            ))
            .expect_err("cycle must be rejected");
        assert_eq!(
            error,
            ValidationError::TreeCycle {
                parent: child,
                child: root
            }
        );
        assert_eq!(scene.node(root).expect("root").children(), &[child]);
    }

    #[test]
    fn command_result_ids_are_unique_within_a_packet() {
        let (mut scene, root, _) = initial_tree();
        let result = ResultId::new(9).expect("test result ID");
        let command = crate::CommandId::new(1).expect("test command");
        let invoke = || Operation::InvokeCommand {
            node: root,
            command,
            arguments: crate::ProtocolValue::Null,
            result: Some(result),
        };
        let error = scene
            .apply(&packet(FrameMode::Delta, 1, 1, 2, vec![invoke(), invoke()]))
            .expect_err("duplicate result must be rejected");
        assert_eq!(error, ValidationError::DuplicateResultId { result });
        assert_eq!(scene.revision(), 1);
    }

    #[test]
    fn replacement_snapshot_requires_a_new_epoch() {
        let (mut scene, _, _) = initial_tree();
        let error = scene
            .apply(&packet(FrameMode::Snapshot, 1, 0, 1, Vec::new()))
            .expect_err("snapshot must rotate its epoch");
        assert_eq!(error, ValidationError::SnapshotReusedEpoch { epoch: 1 });
        assert_eq!(scene.revision(), 1);
    }
}
