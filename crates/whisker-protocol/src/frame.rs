//! Owned semantic frame values.

use crate::{CommandId, ElementTypeId, NodeId, PointerId, PropertyId, ResultId, SurfaceId};

/// Protocol major version implemented by this semantic model.
pub const PROTOCOL_MAJOR: u16 = 1;

/// Protocol minor version implemented by this semantic model.
pub const PROTOCOL_MINOR: u16 = 0;

/// A negotiated frame protocol version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion {
    /// Breaking protocol generation.
    pub major: u16,
    /// Backward-compatible feature generation within one major version.
    pub minor: u16,
}

impl ProtocolVersion {
    /// Version supported by this crate.
    pub const CURRENT: Self = Self {
        major: PROTOCOL_MAJOR,
        minor: PROTOCOL_MINOR,
    };
}

/// Whether a packet replaces or advances the receiver projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameMode {
    /// Replaces all nodes and starts the declared scene epoch.
    Snapshot,
    /// Advances the existing scene from `base_revision`.
    Delta,
}

/// Metadata that orders a frame within one surface and scene epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameHeader {
    /// Protocol version used by the packet.
    pub version: ProtocolVersion,
    /// Destination surface.
    pub surface: SurfaceId,
    /// Generation in which node identifiers remain unique.
    pub scene_epoch: u32,
    /// Monotonically increasing diagnostic frame identifier.
    pub frame_id: u64,
    /// Revision the receiver must currently have for a delta.
    pub base_revision: u64,
    /// Revision accepted after the complete packet succeeds.
    pub target_revision: u64,
    /// Viewport/environment generation used to compute geometry.
    pub viewport_epoch: u32,
    /// Snapshot or incremental transaction.
    pub mode: FrameMode,
}

/// One owned frame transaction before packed wire encoding.
#[derive(Clone, Debug, PartialEq)]
pub struct FramePacket {
    /// Transaction metadata.
    pub header: FrameHeader,
    /// Operations applied in order after complete validation succeeds.
    pub operations: Vec<Operation>,
}

/// Backend-independent rectangle in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutRect {
    /// Horizontal offset from the parent content origin.
    pub x: f32,
    /// Vertical offset from the parent content origin.
    pub y: f32,
    /// Border-box width.
    pub width: f32,
    /// Border-box height.
    pub height: f32,
}

/// A column-major 4-by-4 transform matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform(pub [f32; 16]);

impl Transform {
    /// Identity transform.
    pub const IDENTITY: Self = Self([
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
}

/// Whether a node participates in presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// The node is presented normally.
    Visible,
    /// The node retains scene state but is not presented.
    Hidden,
}

/// How Host hit testing treats a node and its descendants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HitTestBehavior {
    /// The node and descendants use their normal hit-test rules.
    Auto,
    /// Neither the node nor its descendants receives pointer input.
    None,
    /// The node may receive input but its descendants do not.
    BoxOnly,
    /// Descendants may receive input but the node itself does not.
    DescendantsOnly,
}

/// An owned typed value in the semantic model.
///
/// The packed protocol will store repeated and variable-sized values in
/// tables. This recursive owned form exists for engine tests and does not
/// prescribe a per-property wire allocation.
#[derive(Clone, Debug, PartialEq)]
pub enum ProtocolValue {
    /// Absence of a value where a schema permits it.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Signed integer value.
    I64(i64),
    /// Floating-point value.
    F64(f64),
    /// UTF-8 string value.
    String(String),
    /// Opaque binary value interpreted by its declared schema.
    Bytes(Vec<u8>),
    /// Ordered homogeneous or heterogeneous values.
    Array(Vec<Self>),
    /// Ordered named fields.
    Object(Vec<(String, Self)>),
}

/// A semantic mutation within a [`FramePacket`].
#[derive(Clone, Debug, PartialEq)]
pub enum Operation {
    /// Creates an unattached node.
    CreateNode {
        /// New node identifier.
        node: NodeId,
        /// Negotiated element type.
        element_type: ElementTypeId,
    },
    /// Deletes a node and its complete attached subtree.
    DeleteNode {
        /// Subtree root to delete.
        node: NodeId,
    },
    /// Attaches an unattached child at an index.
    InsertChild {
        /// Destination parent.
        parent: NodeId,
        /// Unattached child.
        child: NodeId,
        /// Position in the resulting child list.
        index: u32,
    },
    /// Detaches a direct child without deleting it.
    RemoveChild {
        /// Current parent.
        parent: NodeId,
        /// Direct child to detach.
        child: NodeId,
    },
    /// Reorders a direct child within its current parent.
    MoveChild {
        /// Current parent.
        parent: NodeId,
        /// Direct child to move.
        child: NodeId,
        /// Position after removing the child from its old position.
        index: u32,
    },
    /// Sets resolved box geometry.
    SetLayout {
        /// Target node.
        node: NodeId,
        /// Resolved logical-pixel rectangle.
        rect: LayoutRect,
    },
    /// Sets the resolved transform matrix.
    SetTransform {
        /// Target node.
        node: NodeId,
        /// Resolved transform.
        transform: Transform,
    },
    /// Sets resolved opacity.
    SetOpacity {
        /// Target node.
        node: NodeId,
        /// Opacity in the inclusive range `0.0..=1.0`.
        opacity: f32,
    },
    /// Sets whether a node is presented.
    SetVisibility {
        /// Target node.
        node: NodeId,
        /// Resolved visibility.
        visibility: Visibility,
    },
    /// Sets resolved sibling stacking order.
    SetZOrder {
        /// Target node.
        node: NodeId,
        /// Backend-independent stacking order.
        z_order: i32,
    },
    /// Sets a typed common or element-specific property.
    SetProperty {
        /// Target node.
        node: NodeId,
        /// Negotiated property identifier.
        property: PropertyId,
        /// Typed property payload.
        value: ProtocolValue,
    },
    /// Restores a property to its schema-defined absence/default state.
    ClearProperty {
        /// Target node.
        node: NodeId,
        /// Negotiated property identifier.
        property: PropertyId,
    },
    /// Replaces the event subscription bitset for a node.
    SetEventMask {
        /// Target node.
        node: NodeId,
        /// Negotiated event bits.
        event_mask: u64,
    },
    /// Sets Host hit-test participation.
    SetHitTest {
        /// Target node.
        node: NodeId,
        /// Resolved behavior.
        behavior: HitTestBehavior,
    },
    /// Captures a pointer stream for a node.
    SetPointerCapture {
        /// Target node.
        node: NodeId,
        /// Pointer to capture.
        pointer: PointerId,
    },
    /// Releases a pointer stream previously captured by a node.
    ReleasePointerCapture {
        /// Target node.
        node: NodeId,
        /// Pointer to release.
        pointer: PointerId,
    },
    /// Invokes an element command after preceding mutations.
    InvokeCommand {
        /// Target node.
        node: NodeId,
        /// Negotiated command identifier.
        command: CommandId,
        /// Typed command arguments.
        arguments: ProtocolValue,
        /// Optional asynchronous result correlation.
        result: Option<ResultId>,
    },
}

impl Operation {
    /// Returns the node that must exist when this operation executes.
    pub fn target_node(&self) -> Option<NodeId> {
        match self {
            Self::CreateNode { .. } => None,
            Self::DeleteNode { node }
            | Self::SetLayout { node, .. }
            | Self::SetTransform { node, .. }
            | Self::SetOpacity { node, .. }
            | Self::SetVisibility { node, .. }
            | Self::SetZOrder { node, .. }
            | Self::SetProperty { node, .. }
            | Self::ClearProperty { node, .. }
            | Self::SetEventMask { node, .. }
            | Self::SetHitTest { node, .. }
            | Self::SetPointerCapture { node, .. }
            | Self::ReleasePointerCapture { node, .. }
            | Self::InvokeCommand { node, .. } => Some(*node),
            Self::InsertChild { parent, .. }
            | Self::RemoveChild { parent, .. }
            | Self::MoveChild { parent, .. } => Some(*parent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(value: u64) -> NodeId {
        NodeId::new(value).expect("test node")
    }

    #[test]
    fn target_node_covers_every_operation_group() {
        let target = node(1);
        let child = node(2);
        let element_type = ElementTypeId::new(1).expect("test element type");
        let property = PropertyId::new(1).expect("test property");
        let pointer = PointerId::new(1).expect("test pointer");
        let command = CommandId::new(1).expect("test command");

        let operations = [
            Operation::CreateNode {
                node: target,
                element_type,
            },
            Operation::DeleteNode { node: target },
            Operation::InsertChild {
                parent: target,
                child,
                index: 0,
            },
            Operation::RemoveChild {
                parent: target,
                child,
            },
            Operation::MoveChild {
                parent: target,
                child,
                index: 0,
            },
            Operation::SetLayout {
                node: target,
                rect: LayoutRect::default(),
            },
            Operation::SetTransform {
                node: target,
                transform: Transform::IDENTITY,
            },
            Operation::SetOpacity {
                node: target,
                opacity: 1.0,
            },
            Operation::SetVisibility {
                node: target,
                visibility: Visibility::Visible,
            },
            Operation::SetZOrder {
                node: target,
                z_order: 0,
            },
            Operation::SetProperty {
                node: target,
                property,
                value: ProtocolValue::Null,
            },
            Operation::ClearProperty {
                node: target,
                property,
            },
            Operation::SetEventMask {
                node: target,
                event_mask: 0,
            },
            Operation::SetHitTest {
                node: target,
                behavior: HitTestBehavior::Auto,
            },
            Operation::SetPointerCapture {
                node: target,
                pointer,
            },
            Operation::ReleasePointerCapture {
                node: target,
                pointer,
            },
            Operation::InvokeCommand {
                node: target,
                command,
                arguments: ProtocolValue::Null,
                result: None,
            },
        ];

        assert_eq!(operations[0].target_node(), None);
        for operation in &operations[1..] {
            assert_eq!(operation.target_node(), Some(target));
        }
    }

    #[test]
    fn semantic_values_represent_every_owned_value_shape() {
        let values = [
            ProtocolValue::Null,
            ProtocolValue::Bool(true),
            ProtocolValue::I64(-1),
            ProtocolValue::F64(0.5),
            ProtocolValue::String("value".into()),
            ProtocolValue::Bytes(vec![1, 2]),
            ProtocolValue::Array(vec![ProtocolValue::Null]),
            ProtocolValue::Object(vec![("key".into(), ProtocolValue::Bool(false))]),
        ];

        assert_eq!(values.len(), 8);
        assert_ne!(Visibility::Visible, Visibility::Hidden);
        assert_ne!(HitTestBehavior::None, HitTestBehavior::BoxOnly);
        assert_ne!(HitTestBehavior::Auto, HitTestBehavior::DescendantsOnly);
    }
}
