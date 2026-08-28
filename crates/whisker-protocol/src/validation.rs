//! Transactional reference validation for semantic frame packets.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use crate::{
    ElementTypeId, FrameMode, FramePacket, NodeId, Operation, PROTOCOL_MAJOR, SurfaceId,
    TextContentError,
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
        receiver_revision: u64,
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
    /// Packet requires a newer protocol minor than this implementation knows.
    UnsupportedProtocolMinor {
        /// Received minor version.
        received: u16,
        /// Highest minor understood by this implementation.
        supported: u16,
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
    /// Box paint contained an invalid color, length, or percentage.
    InvalidBoxPaint,
    /// Background layers contained invalid image, gradient, position, or size data.
    InvalidBackgroundLayers,
    /// A visual effect contained invalid color or numeric data.
    InvalidVisualEffects,
    /// Replaced image content contained invalid position data.
    InvalidImageContent,
    /// A geometry or transform component was NaN or infinite.
    NonFiniteNumber,
    /// Plain-text presentation contained invalid shaping inputs.
    InvalidText {
        /// Stable invalid-input category.
        error: TextContentError,
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
                receiver_revision: self.revision,
            });
        }

        let mut next = if packet.header.mode == FrameMode::Snapshot {
            Self::new(self.surface)
        } else {
            self.clone()
        };
        next.scene_epoch = Some(packet.header.scene_epoch);

        for operation in &packet.operations {
            next.apply_operation(operation)?;
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
        if header.version.minor > crate::PROTOCOL_MINOR {
            return Err(ValidationError::UnsupportedProtocolMinor {
                received: header.version.minor,
                supported: crate::PROTOCOL_MINOR,
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

    fn apply_operation(&mut self, operation: &Operation) -> Result<(), ValidationError> {
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
            Operation::SetLayout { node, geometry } => {
                self.require_node(*node)?;
                if !geometry.is_valid() {
                    return Err(ValidationError::NonFiniteNumber);
                }
            }
            Operation::SetBoxPaint { node, paint } => {
                self.require_node(*node)?;
                if !paint.validate() {
                    return Err(ValidationError::InvalidBoxPaint);
                }
            }
            Operation::SetBackgroundLayers { node, layers } => {
                self.require_node(*node)?;
                if !layers.iter().all(crate::BackgroundLayer::validate) {
                    return Err(ValidationError::InvalidBackgroundLayers);
                }
            }
            Operation::SetVisualEffects { node, effects } => {
                self.require_node(*node)?;
                if !effects.validate() {
                    return Err(ValidationError::InvalidVisualEffects);
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
            Operation::SetText { node, content } => {
                self.require_node(*node)?;
                content
                    .validate()
                    .map_err(|error| ValidationError::InvalidText { error })?;
            }
            Operation::SetTextStyle { node, style } => {
                self.require_node(*node)?;
                style
                    .validate()
                    .map_err(|error| ValidationError::InvalidText { error })?;
            }
            Operation::SetImage { node, content } => {
                self.require_node(*node)?;
                if !content.validate() {
                    return Err(ValidationError::InvalidImageContent);
                }
            }
            Operation::InvokeCommand { node, .. } => {
                self.require_node(*node)?;
            }
            Operation::SetClip { node, .. }
            | Operation::SetVisibility { node, .. }
            | Operation::SetZOrder { node, .. }
            | Operation::SetProperty { node, .. }
            | Operation::ClearProperty { node, .. }
            | Operation::SetEventMask { node, .. }
            | Operation::SetHitTest { node, .. }
            | Operation::SetCursor { node, .. }
            | Operation::SetPointerCapture { node, .. }
            | Operation::ReleasePointerCapture { node, .. } => {
                self.require_node(*node)?;
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
        let len = self.require_node(parent)?.children.len();
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
    use crate::{
        CommandId, ElementTypeId, FrameHeader, HitTestBehavior, MeasureFontFamily,
        MeasureFontStyle, MeasureLineHeight, MeasureTextDirection, MeasureTextOverflow,
        MeasureTextWrap, ObjectFit, Operation, PaintPosition, PointerId, PropertyId,
        ProtocolVersion, ResourceId, TextContent, TextContentError, TextMeasurePayload,
        TextMeasureStyle, Transform, Visibility, WhiskerValue,
    };

    fn surface() -> SurfaceId {
        SurfaceId::new(1).expect("test surface")
    }

    fn node(value: u64) -> NodeId {
        NodeId::new(value).expect("test node")
    }

    fn element_type() -> ElementTypeId {
        ElementTypeId::new(1).expect("test element type")
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
                    ..TextMeasureStyle::default()
                },
                locale: None,
                direction: MeasureTextDirection::Auto,
                alignment: crate::MeasureTextAlignment::Start,
                indent: Default::default(),
                wrap: MeasureTextWrap::Wrap,
                word_break: Default::default(),
                max_lines: None,
                overflow: MeasureTextOverflow::Clip,
            },
            paint: crate::TextPaint::default(),
            prepared_content: None,
        }
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

    fn apply_next(
        scene: &mut SceneProjection,
        operations: Vec<Operation>,
    ) -> Result<ApplyResult, ValidationError> {
        let revision = scene.revision();
        scene.apply(&packet(
            FrameMode::Delta,
            scene.scene_epoch().expect("initialized scene"),
            revision,
            revision + 1,
            operations,
        ))
    }

    fn box_paint() -> crate::BoxPaint {
        let zero = crate::PaintLengthPercentage {
            length: 0.0,
            fraction: 0.0,
        };
        crate::BoxPaint {
            background_color: crate::PaintColor::default(),
            border_widths: crate::PaintEdges {
                top: zero,
                right: zero,
                bottom: zero,
                left: zero,
            },
            border_colors: crate::PaintEdges {
                top: crate::PaintColor::default(),
                right: crate::PaintColor::default(),
                bottom: crate::PaintColor::default(),
                left: crate::PaintColor::default(),
            },
            border_styles: crate::PaintEdges {
                top: crate::BorderLineStyle::None,
                right: crate::BorderLineStyle::None,
                bottom: crate::BorderLineStyle::None,
                left: crate::BorderLineStyle::None,
            },
            border_radii: crate::PaintCorners {
                top_left: crate::PaintCornerRadius::circular(zero),
                top_right: crate::PaintCornerRadius::circular(zero),
                bottom_right: crate::PaintCornerRadius::circular(zero),
                bottom_left: crate::PaintCornerRadius::circular(zero),
            },
        }
    }

    #[test]
    fn snapshot_builds_a_retained_tree() {
        let (scene, root, child) = initial_tree();
        assert_eq!(scene.scene_epoch(), Some(1));
        assert_eq!(scene.revision(), 1);
        assert_eq!(scene.node_count(), 2);
        assert_eq!(scene.surface(), surface());
        assert_eq!(
            scene.node(root).expect("root").element_type(),
            element_type()
        );
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
        assert_eq!(
            result,
            ApplyResult::NeedSnapshot {
                receiver_revision: 1
            }
        );
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
                        geometry: crate::LayoutGeometry {
                            border_box: crate::LayoutRect {
                                width: f32::NAN,
                                ..crate::LayoutRect::default()
                            },
                            ..crate::LayoutGeometry::default()
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
    fn replacement_snapshot_requires_a_new_epoch() {
        let (mut scene, _, _) = initial_tree();
        let error = scene
            .apply(&packet(FrameMode::Snapshot, 1, 0, 1, Vec::new()))
            .expect_err("snapshot must rotate its epoch");
        assert_eq!(error, ValidationError::SnapshotReusedEpoch { epoch: 1 });
        assert_eq!(scene.revision(), 1);
    }

    #[test]
    fn replacement_snapshot_accepts_a_new_epoch_and_discards_old_nodes() {
        let (mut scene, old_root, _) = initial_tree();
        let new_root = node(9);
        let result = scene
            .apply(&packet(
                FrameMode::Snapshot,
                2,
                0,
                1,
                vec![Operation::CreateNode {
                    node: new_root,
                    element_type: element_type(),
                }],
            ))
            .expect("new epoch snapshot");

        assert_eq!(result, ApplyResult::Accepted { revision: 1 });
        assert_eq!(scene.scene_epoch(), Some(2));
        assert!(scene.node(old_root).is_none());
        assert!(scene.node(new_root).is_some());
    }

    #[test]
    fn header_validation_reports_every_malformed_case() {
        let cases = [
            (
                {
                    let mut value = packet(FrameMode::Snapshot, 1, 0, 1, Vec::new());
                    value.header.version.major += 1;
                    value
                },
                ValidationError::UnsupportedProtocolMajor { received: 2 },
            ),
            (
                {
                    let mut value = packet(FrameMode::Snapshot, 1, 0, 1, Vec::new());
                    value.header.version.minor += 1;
                    value
                },
                ValidationError::UnsupportedProtocolMinor {
                    received: crate::PROTOCOL_MINOR + 1,
                    supported: crate::PROTOCOL_MINOR,
                },
            ),
            (
                {
                    let mut value = packet(FrameMode::Snapshot, 1, 0, 1, Vec::new());
                    value.header.surface = SurfaceId::new(2).expect("other surface");
                    value
                },
                ValidationError::SurfaceMismatch {
                    expected: surface(),
                    received: SurfaceId::new(2).expect("other surface"),
                },
            ),
            (
                packet(FrameMode::Snapshot, 1, 2, 3, Vec::new()),
                ValidationError::SnapshotBaseRevision { received: 2 },
            ),
            (
                packet(FrameMode::Snapshot, 1, 0, 0, Vec::new()),
                ValidationError::RevisionDidNotAdvance { base: 0, target: 0 },
            ),
        ];

        for (packet, expected) in cases {
            let mut scene = SceneProjection::new(surface());
            let error = scene.apply(&packet).expect_err("malformed header");
            assert_eq!(error, expected);
            assert!(error.to_string().starts_with("invalid Whisker frame:"));
            let as_error: &dyn Error = &error;
            assert!(as_error.source().is_none());
            assert_eq!(scene.revision(), 0);
        }
    }

    #[test]
    fn scene_epoch_drift_requests_a_snapshot() {
        let (mut scene, _, _) = initial_tree();
        let result = scene
            .apply(&packet(FrameMode::Delta, 2, 1, 2, Vec::new()))
            .expect("epoch drift is recoverable");
        assert_eq!(
            result,
            ApplyResult::NeedSnapshot {
                receiver_revision: 1
            }
        );
        assert_eq!(scene.scene_epoch(), Some(1));
    }

    #[test]
    fn all_non_structural_operations_accept_valid_values() {
        let (mut scene, root, _) = initial_tree();
        let property = PropertyId::new(1).expect("test property");
        let pointer = PointerId::new(1).expect("test pointer");
        let command = CommandId::new(1).expect("test command");

        let outcome = apply_next(
            &mut scene,
            vec![
                Operation::SetLayout {
                    node: root,
                    geometry: crate::LayoutGeometry {
                        border_box: crate::LayoutRect {
                            x: -1.0,
                            y: 2.0,
                            width: 30.0,
                            height: 40.0,
                        },
                        content_box: crate::LayoutRect {
                            width: 30.0,
                            height: 40.0,
                            ..crate::LayoutRect::default()
                        },
                    },
                },
                Operation::SetBoxPaint {
                    node: root,
                    paint: box_paint(),
                },
                Operation::SetBackgroundLayers {
                    node: root,
                    layers: Vec::new(),
                },
                Operation::SetVisualEffects {
                    node: root,
                    effects: crate::VisualEffects::default(),
                },
                Operation::SetClip {
                    node: root,
                    clip: crate::BoxClip {
                        horizontal: crate::OverflowClip::Hidden,
                        vertical: crate::OverflowClip::Visible,
                    },
                },
                Operation::SetTransform {
                    node: root,
                    transform: Transform::IDENTITY,
                },
                Operation::SetOpacity {
                    node: root,
                    opacity: 0.0,
                },
                Operation::SetOpacity {
                    node: root,
                    opacity: 1.0,
                },
                Operation::SetVisibility {
                    node: root,
                    visibility: Visibility::Hidden,
                },
                Operation::SetZOrder {
                    node: root,
                    z_order: -2,
                },
                Operation::SetText {
                    node: root,
                    content: text_content("hello"),
                },
                Operation::SetTextStyle {
                    node: root,
                    style: crate::TextStyleSnapshot::from(&text_content("styled")),
                },
                Operation::SetImage {
                    node: root,
                    content: crate::ImageContent {
                        resource: ResourceId::new(1).unwrap(),
                        fit: ObjectFit::Contain,
                        position: PaintPosition::default(),
                    },
                },
                Operation::SetProperty {
                    node: root,
                    property,
                    value: WhiskerValue::map([("enabled", WhiskerValue::Bool(true))]),
                },
                Operation::ClearProperty {
                    node: root,
                    property,
                },
                Operation::SetEventMask {
                    node: root,
                    event_mask: 3,
                },
                Operation::SetHitTest {
                    node: root,
                    behavior: HitTestBehavior::BoxOnly,
                },
                Operation::SetCursor {
                    node: root,
                    cursor: crate::Cursor {
                        resources: Vec::new(),
                        fallback: crate::CursorKeyword::Pointer,
                    },
                },
                Operation::SetPointerCapture {
                    node: root,
                    pointer,
                },
                Operation::ReleasePointerCapture {
                    node: root,
                    pointer,
                },
                Operation::InvokeCommand {
                    node: root,
                    command,
                    arguments: WhiskerValue::Array(Vec::new()),
                },
            ],
        )
        .expect("all valid operations");

        assert_eq!(outcome, ApplyResult::Accepted { revision: 2 });
    }

    #[test]
    fn structural_operations_support_detach_reinsert_and_move() {
        let (mut scene, root, child) = initial_tree();
        let sibling = node(3);
        apply_next(
            &mut scene,
            vec![
                Operation::CreateNode {
                    node: sibling,
                    element_type: element_type(),
                },
                Operation::InsertChild {
                    parent: root,
                    child: sibling,
                    index: 1,
                },
                Operation::MoveChild {
                    parent: root,
                    child: sibling,
                    index: 0,
                },
                Operation::RemoveChild {
                    parent: root,
                    child,
                },
                Operation::InsertChild {
                    parent: root,
                    child,
                    index: 1,
                },
            ],
        )
        .expect("valid structural transaction");

        assert_eq!(
            scene.node(root).expect("root").children(),
            &[sibling, child]
        );
        assert_eq!(scene.node(child).expect("child").parent(), Some(root));
    }

    #[test]
    fn unattached_root_can_be_deleted() {
        let (mut scene, root, _) = initial_tree();
        apply_next(&mut scene, vec![Operation::DeleteNode { node: root }])
            .expect("delete unattached scene root");
        assert_eq!(scene.node_count(), 0);
    }

    #[test]
    fn unknown_node_is_rejected_by_specialized_and_generic_operations() {
        let (mut scene, _, _) = initial_tree();
        let missing = node(99);
        let property = PropertyId::new(1).expect("test property");
        let operations = [
            Operation::DeleteNode { node: missing },
            Operation::SetLayout {
                node: missing,
                geometry: crate::LayoutGeometry::default(),
            },
            Operation::SetBoxPaint {
                node: missing,
                paint: box_paint(),
            },
            Operation::SetBackgroundLayers {
                node: missing,
                layers: Vec::new(),
            },
            Operation::SetVisualEffects {
                node: missing,
                effects: crate::VisualEffects::default(),
            },
            Operation::SetClip {
                node: missing,
                clip: crate::BoxClip {
                    horizontal: crate::OverflowClip::Visible,
                    vertical: crate::OverflowClip::Visible,
                },
            },
            Operation::SetTransform {
                node: missing,
                transform: Transform::IDENTITY,
            },
            Operation::SetOpacity {
                node: missing,
                opacity: 1.0,
            },
            Operation::SetText {
                node: missing,
                content: text_content("missing"),
            },
            Operation::SetTextStyle {
                node: missing,
                style: crate::TextStyleSnapshot::from(&text_content("missing style")),
            },
            Operation::SetImage {
                node: missing,
                content: crate::ImageContent {
                    resource: ResourceId::new(1).unwrap(),
                    fit: ObjectFit::Contain,
                    position: PaintPosition::default(),
                },
            },
            Operation::SetProperty {
                node: missing,
                property,
                value: WhiskerValue::Null,
            },
            Operation::InvokeCommand {
                node: missing,
                command: CommandId::new(1).expect("test command"),
                arguments: WhiskerValue::Null,
            },
        ];

        for operation in operations {
            let error = apply_next(&mut scene, vec![operation]).expect_err("unknown node");
            assert_eq!(error, ValidationError::UnknownNode { node: missing });
            assert_eq!(scene.revision(), 1);
        }
    }

    #[test]
    fn malformed_box_paint_is_rejected_transactionally() {
        let (mut scene, root, _) = initial_tree();
        let mut paint = box_paint();
        paint.border_radii.bottom_left.horizontal.length = -1.0;

        let error = apply_next(
            &mut scene,
            vec![Operation::SetBoxPaint { node: root, paint }],
        )
        .expect_err("invalid box paint");

        assert_eq!(error, ValidationError::InvalidBoxPaint);
        assert_eq!(scene.revision(), 1);
    }

    #[test]
    fn malformed_extended_visual_operations_are_rejected_transactionally() {
        let (mut scene, root, _) = initial_tree();
        let revision = scene.revision();
        let background = crate::BackgroundLayer {
            image: crate::PaintImage::LinearGradient {
                angle_degrees: f32::NAN,
                repeating: false,
                stops: Vec::new(),
            },
            position: PaintPosition::default(),
            size: crate::BackgroundSize::Auto,
            repeat_x: crate::ImageRepeat::NoRepeat,
            repeat_y: crate::ImageRepeat::NoRepeat,
            origin: crate::PaintBox::Padding,
            clip: crate::PaintBox::Border,
            attachment: crate::BackgroundAttachment::Scroll,
            blend_mode: crate::BlendMode::Normal,
        };
        let error = apply_next(
            &mut scene,
            vec![Operation::SetBackgroundLayers {
                node: root,
                layers: vec![background],
            }],
        )
        .expect_err("invalid gradient");
        assert_eq!(error, ValidationError::InvalidBackgroundLayers);
        assert_eq!(scene.revision(), revision);

        let effects = crate::VisualEffects {
            backdrop_blur: Some(-1.0),
            ..crate::VisualEffects::default()
        };
        let error = apply_next(
            &mut scene,
            vec![Operation::SetVisualEffects {
                node: root,
                effects,
            }],
        )
        .expect_err("invalid backdrop blur");
        assert_eq!(error, ValidationError::InvalidVisualEffects);
        assert_eq!(scene.revision(), revision);

        let error = apply_next(
            &mut scene,
            vec![Operation::SetImage {
                node: root,
                content: crate::ImageContent {
                    resource: ResourceId::new(1).unwrap(),
                    fit: ObjectFit::Contain,
                    position: PaintPosition {
                        x: crate::PaintCoordinate {
                            length: f32::INFINITY,
                            fraction: 0.0,
                        },
                        y: crate::PaintCoordinate::default(),
                    },
                },
            }],
        )
        .expect_err("invalid image position");
        assert_eq!(error, ValidationError::InvalidImageContent);
        assert_eq!(scene.revision(), revision);
    }

    #[test]
    fn malformed_text_is_rejected_transactionally() {
        let (mut scene, root, _) = initial_tree();
        let mut content = text_content("invalid");
        content.payload.style.font_families.clear();
        let error = apply_next(
            &mut scene,
            vec![Operation::SetText {
                node: root,
                content,
            }],
        )
        .expect_err("invalid text payload");
        assert_eq!(
            error,
            ValidationError::InvalidText {
                error: TextContentError::InvalidMeasurement(
                    crate::MeasurementPayloadError::InvalidFontFamily,
                ),
            }
        );
        assert_eq!(scene.revision(), 1);

        let mut style = crate::TextStyleSnapshot::from(&text_content("invalid style"));
        style.style.font_families.clear();
        let error = apply_next(
            &mut scene,
            vec![Operation::SetTextStyle { node: root, style }],
        )
        .expect_err("invalid inherited text style");
        assert_eq!(
            error,
            ValidationError::InvalidText {
                error: TextContentError::InvalidMeasurement(
                    crate::MeasurementPayloadError::InvalidFontFamily,
                ),
            }
        );
        assert_eq!(scene.revision(), 1);
    }

    #[test]
    fn insert_reports_unknown_attached_and_index_errors() {
        let (mut scene, root, child) = initial_tree();
        let missing = node(99);
        let unattached = node(3);
        apply_next(
            &mut scene,
            vec![Operation::CreateNode {
                node: unattached,
                element_type: element_type(),
            }],
        )
        .expect("create unattached node");

        let cases = [
            (
                Operation::InsertChild {
                    parent: missing,
                    child: unattached,
                    index: 0,
                },
                ValidationError::UnknownNode { node: missing },
            ),
            (
                Operation::InsertChild {
                    parent: root,
                    child: missing,
                    index: 0,
                },
                ValidationError::UnknownNode { node: missing },
            ),
            (
                Operation::InsertChild {
                    parent: root,
                    child,
                    index: 0,
                },
                ValidationError::ChildAlreadyAttached {
                    child,
                    parent: root,
                },
            ),
            (
                Operation::InsertChild {
                    parent: root,
                    child: unattached,
                    index: 2,
                },
                ValidationError::ChildIndexOutOfBounds {
                    parent: root,
                    index: 2,
                    len: 1,
                },
            ),
            (
                Operation::InsertChild {
                    parent: unattached,
                    child: unattached,
                    index: 0,
                },
                ValidationError::TreeCycle {
                    parent: unattached,
                    child: unattached,
                },
            ),
        ];

        for (operation, expected) in cases {
            let error = apply_next(&mut scene, vec![operation]).expect_err("invalid insertion");
            assert_eq!(error, expected);
            assert_eq!(scene.revision(), 2);
        }
    }

    #[test]
    fn remove_and_move_report_relationship_and_index_errors() {
        let (mut scene, root, child) = initial_tree();
        let sibling = node(3);
        let missing = node(99);
        apply_next(
            &mut scene,
            vec![Operation::CreateNode {
                node: sibling,
                element_type: element_type(),
            }],
        )
        .expect("create sibling");

        let not_child = ValidationError::NotDirectChild {
            parent: root,
            child: sibling,
        };
        for operation in [
            Operation::RemoveChild {
                parent: root,
                child: sibling,
            },
            Operation::MoveChild {
                parent: root,
                child: sibling,
                index: 0,
            },
        ] {
            assert_eq!(
                apply_next(&mut scene, vec![operation]).expect_err("not a child"),
                not_child
            );
        }

        for operation in [
            Operation::RemoveChild {
                parent: missing,
                child,
            },
            Operation::MoveChild {
                parent: root,
                child: missing,
                index: 0,
            },
        ] {
            assert_eq!(
                apply_next(&mut scene, vec![operation]).expect_err("unknown relationship node"),
                ValidationError::UnknownNode { node: missing }
            );
        }

        let error = apply_next(
            &mut scene,
            vec![Operation::MoveChild {
                parent: root,
                child,
                index: 1,
            }],
        )
        .expect_err("index is checked after removal");
        assert_eq!(
            error,
            ValidationError::ChildIndexOutOfBounds {
                parent: root,
                index: 1,
                len: 0,
            }
        );
        assert_eq!(scene.node(root).expect("root").children(), &[child]);
    }

    #[test]
    fn numeric_validation_rejects_transform_and_opacity_edges() {
        let (mut scene, root, _) = initial_tree();
        let mut invalid_transform = Transform::IDENTITY;
        invalid_transform.0[15] = f32::INFINITY;
        let transform_error = apply_next(
            &mut scene,
            vec![Operation::SetTransform {
                node: root,
                transform: invalid_transform,
            }],
        )
        .expect_err("infinite transform");
        assert_eq!(transform_error, ValidationError::NonFiniteNumber);

        let range_error = apply_next(
            &mut scene,
            vec![Operation::SetOpacity {
                node: root,
                opacity: -0.1,
            }],
        )
        .expect_err("out-of-range opacity");
        assert_eq!(
            range_error,
            ValidationError::InvalidOpacity { opacity: -0.1 }
        );

        let nan_error = apply_next(
            &mut scene,
            vec![Operation::SetOpacity {
                node: root,
                opacity: f32::NAN,
            }],
        )
        .expect_err("NaN opacity");
        assert_eq!(format!("{nan_error:?}"), "InvalidOpacity { opacity: NaN }");
        assert_eq!(scene.revision(), 1);
    }
}
