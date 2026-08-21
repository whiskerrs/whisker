use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use whisker_engine::FrameSink;
use whisker_protocol::{
    ApplyResult, BoxClip, BoxPaint, ElementChildMount, ElementTypeId, FrameMode, FramePacket,
    LayoutGeometry, LayoutRect, NodeId, Operation, OverflowClip, PaintColor, SceneProjection,
    SurfaceId, TextContent, ValidationError, Visibility,
};

use crate::element::{DesktopElementContent, DesktopElementError, DesktopElementRegistry};

#[derive(Clone, Debug)]
struct CommonPresentation {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    layout: LayoutGeometry,
    paint: Option<BoxPaint>,
    clip: BoxClip,
    opacity: f32,
    visibility: Visibility,
    z_order: i32,
}

impl Default for CommonPresentation {
    fn default() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            layout: LayoutGeometry::default(),
            paint: None,
            clip: BoxClip {
                horizontal: OverflowClip::Visible,
                vertical: OverflowClip::Visible,
            },
            opacity: 1.0,
            visibility: Visibility::Visible,
            z_order: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct RenderNode {
    element_type: ElementTypeId,
    presentation: CommonPresentation,
    content: DesktopElementContent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LogicalClip {
    pub(crate) left: Option<f32>,
    pub(crate) top: Option<f32>,
    pub(crate) right: Option<f32>,
    pub(crate) bottom: Option<f32>,
}

impl LogicalClip {
    fn intersect(self, rect: LayoutRect, horizontal: bool, vertical: bool) -> Self {
        Self {
            left: horizontal
                .then(|| self.left.map_or(rect.x, |value| value.max(rect.x)))
                .or(self.left),
            top: vertical
                .then(|| self.top.map_or(rect.y, |value| value.max(rect.y)))
                .or(self.top),
            right: horizontal
                .then(|| {
                    self.right
                        .map_or(rect.x + rect.width, |value| value.min(rect.x + rect.width))
                })
                .or(self.right),
            bottom: vertical
                .then(|| {
                    self.bottom.map_or(rect.y + rect.height, |value| {
                        value.min(rect.y + rect.height)
                    })
                })
                .or(self.bottom),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PaintCommand<'a> {
    Box {
        rect: LayoutRect,
        paint: &'a BoxPaint,
        clip: LogicalClip,
        opacity: f32,
    },
    Text {
        node: NodeId,
        rect: LayoutRect,
        content: &'a TextContent,
        clip: LogicalClip,
        opacity: f32,
    },
}

#[derive(Debug)]
pub(crate) struct DesktopScene {
    validation: SceneProjection,
    elements: DesktopElementRegistry,
    nodes: HashMap<NodeId, RenderNode>,
}

impl DesktopScene {
    pub(crate) fn new(surface: SurfaceId, elements: DesktopElementRegistry) -> Self {
        Self {
            validation: SceneProjection::new(surface),
            elements,
            nodes: HashMap::new(),
        }
    }

    pub(crate) fn paint_commands(&self) -> Vec<PaintCommand<'_>> {
        let mut roots = self
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                node.presentation
                    .parent
                    .is_none()
                    .then_some((*id, node.presentation.z_order))
            })
            .collect::<Vec<_>>();
        roots.sort_by_key(|(id, z)| (*z, id.get()));
        let mut commands = Vec::new();
        for (root, _) in roots {
            self.collect_commands(root, 0.0, 0.0, LogicalClip::default(), 1.0, &mut commands);
        }
        commands
    }

    fn collect_commands<'a>(
        &'a self,
        id: NodeId,
        parent_x: f32,
        parent_y: f32,
        ancestor_clip: LogicalClip,
        ancestor_opacity: f32,
        commands: &mut Vec<PaintCommand<'a>>,
    ) {
        let node = self.nodes.get(&id).expect("retained child remains live");
        let presentation = &node.presentation;
        let border = LayoutRect {
            x: parent_x + presentation.layout.border_box.x,
            y: parent_y + presentation.layout.border_box.y,
            width: presentation.layout.border_box.width,
            height: presentation.layout.border_box.height,
        };
        let opacity = ancestor_opacity * presentation.opacity;
        if presentation.visibility == Visibility::Visible {
            if let Some(paint) = &presentation.paint {
                commands.push(PaintCommand::Box {
                    rect: border,
                    paint,
                    clip: ancestor_clip,
                    opacity,
                });
            }
        }

        let descendant_clip = ancestor_clip.intersect(
            border,
            presentation.clip.horizontal == OverflowClip::Hidden,
            presentation.clip.vertical == OverflowClip::Hidden,
        );
        if presentation.visibility == Visibility::Visible
            && let Some(content) = node.content.text()
        {
            let content_rect = LayoutRect {
                x: border.x + presentation.layout.content_box.x,
                y: border.y + presentation.layout.content_box.y,
                width: presentation.layout.content_box.width,
                height: presentation.layout.content_box.height,
            };
            commands.push(PaintCommand::Text {
                node: id,
                rect: content_rect,
                content,
                clip: descendant_clip.intersect(content_rect, true, true),
                opacity,
            });
        }

        let mut children = node
            .presentation
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let z = self
                    .nodes
                    .get(child)
                    .expect("retained child remains live")
                    .presentation
                    .z_order;
                (*child, z, index)
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|(_, z, index)| (*z, *index));
        for (child, _, _) in children {
            self.collect_commands(
                child,
                border.x,
                border.y,
                descendant_clip,
                opacity,
                commands,
            );
        }
    }

    fn validate_element_operations(&self, packet: &FramePacket) -> Result<(), DesktopElementError> {
        let mut types = if packet.header.mode == FrameMode::Snapshot {
            HashMap::new()
        } else {
            self.nodes
                .iter()
                .map(|(node, state)| (*node, state.element_type))
                .collect()
        };
        for operation in &packet.operations {
            match operation {
                Operation::CreateNode { node, element_type } => {
                    self.elements.create(*element_type)?;
                    types.insert(*node, *element_type);
                }
                Operation::InsertChild { parent, .. } => {
                    if let Some(element_type) = types.get(parent).copied()
                        && self.elements.child_mount(element_type)? == ElementChildMount::None
                    {
                        return Err(DesktopElementError::ChildrenNotAllowed { parent: *parent });
                    }
                }
                Operation::SetText { node, content } => {
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements
                            .create(element_type)?
                            .set_text(*node, content.clone())?;
                    }
                }
                Operation::SetProperty { node, .. } | Operation::ClearProperty { node, .. } => {
                    if types.contains_key(node) {
                        return Err(DesktopElementError::UnsupportedProperty { node: *node });
                    }
                }
                Operation::InvokeCommand { node, .. } => {
                    if types.contains_key(node) {
                        return Err(DesktopElementError::UnsupportedCommand { node: *node });
                    }
                }
                Operation::DeleteNode { .. }
                | Operation::RemoveChild { .. }
                | Operation::MoveChild { .. }
                | Operation::SetLayout { .. }
                | Operation::SetBoxPaint { .. }
                | Operation::SetClip { .. }
                | Operation::SetTransform { .. }
                | Operation::SetOpacity { .. }
                | Operation::SetVisibility { .. }
                | Operation::SetZOrder { .. }
                | Operation::SetEventMask { .. }
                | Operation::SetHitTest { .. }
                | Operation::SetPointerCapture { .. }
                | Operation::ReleasePointerCapture { .. } => {}
            }
        }
        Ok(())
    }

    fn apply_operations(&mut self, packet: &FramePacket) {
        if packet.header.mode == FrameMode::Snapshot {
            self.nodes.clear();
        }
        for operation in &packet.operations {
            match operation {
                Operation::CreateNode { node, element_type } => {
                    let content = self
                        .elements
                        .create(*element_type)
                        .expect("element operations were validated before commit");
                    self.nodes.insert(
                        *node,
                        RenderNode {
                            element_type: *element_type,
                            presentation: CommonPresentation::default(),
                            content,
                        },
                    );
                }
                Operation::DeleteNode { node } => self.delete_subtree(*node),
                Operation::InsertChild {
                    parent,
                    child,
                    index,
                } => {
                    self.nodes
                        .get_mut(child)
                        .expect("validated child")
                        .presentation
                        .parent = Some(*parent);
                    self.nodes
                        .get_mut(parent)
                        .expect("validated parent")
                        .presentation
                        .children
                        .insert(*index as usize, *child);
                }
                Operation::RemoveChild { parent, child } => {
                    self.nodes
                        .get_mut(child)
                        .expect("validated child")
                        .presentation
                        .parent = None;
                    self.nodes
                        .get_mut(parent)
                        .expect("validated parent")
                        .presentation
                        .children
                        .retain(|candidate| candidate != child);
                }
                Operation::MoveChild {
                    parent,
                    child,
                    index,
                } => {
                    let children = &mut self
                        .nodes
                        .get_mut(parent)
                        .expect("validated parent")
                        .presentation
                        .children;
                    let old = children
                        .iter()
                        .position(|candidate| candidate == child)
                        .expect("validated direct child");
                    children.remove(old);
                    children.insert(*index as usize, *child);
                }
                Operation::SetLayout { node, geometry } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .layout = *geometry;
                }
                Operation::SetBoxPaint { node, paint } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .paint = Some(paint.clone());
                }
                Operation::SetClip { node, clip } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .clip = *clip;
                }
                Operation::SetOpacity { node, opacity } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .opacity = *opacity;
                }
                Operation::SetVisibility { node, visibility } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .visibility = *visibility;
                }
                Operation::SetZOrder { node, z_order } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .z_order = *z_order;
                }
                Operation::SetText { node, content } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .content
                        .set_text(*node, content.clone())
                        .expect("element content operation was validated before commit");
                }
                Operation::SetTransform { .. }
                | Operation::SetProperty { .. }
                | Operation::ClearProperty { .. }
                | Operation::SetEventMask { .. }
                | Operation::SetHitTest { .. }
                | Operation::SetPointerCapture { .. }
                | Operation::ReleasePointerCapture { .. }
                | Operation::InvokeCommand { .. } => {}
            }
        }
    }

    fn delete_subtree(&mut self, node: NodeId) {
        let Some(removed) = self.nodes.remove(&node) else {
            return;
        };
        if let Some(parent) = removed.presentation.parent
            && let Some(parent) = self.nodes.get_mut(&parent)
        {
            parent
                .presentation
                .children
                .retain(|candidate| *candidate != node);
        }
        for child in removed.presentation.children {
            self.delete_subtree(child);
        }
    }
}

impl FrameSink for DesktopScene {
    type Error = DesktopPresentError;

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        if packet.header.mode == FrameMode::Delta
            && (self.validation.scene_epoch() != Some(packet.header.scene_epoch)
                || self.validation.revision() != packet.header.base_revision)
        {
            return self.validation.apply(packet).map_err(Into::into);
        }
        // Element validation is read-only. The reference projection then
        // validates and commits the complete protocol transaction atomically,
        // allowing the retained Desktop tree to update in place without a
        // whole-scene clone on each delta.
        self.validate_element_operations(packet)?;
        let result = self.validation.apply(packet)?;
        if matches!(result, ApplyResult::Accepted { .. }) {
            self.apply_operations(packet);
        }
        Ok(result)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DesktopPresentError {
    Protocol(ValidationError),
    Element(DesktopElementError),
}

impl fmt::Display for DesktopPresentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Desktop frame rejection: {self:?}")
    }
}

impl Error for DesktopPresentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Protocol(error) => Some(error),
            Self::Element(error) => Some(error),
        }
    }
}

impl From<ValidationError> for DesktopPresentError {
    fn from(error: ValidationError) -> Self {
        Self::Protocol(error)
    }
}

impl From<DesktopElementError> for DesktopPresentError {
    fn from(error: DesktopElementError) -> Self {
        Self::Element(error)
    }
}

pub(crate) fn is_transparent(color: &PaintColor) -> bool {
    matches!(
        color,
        PaintColor::Srgba { alpha, .. } | PaintColor::Hsla { alpha, .. } if *alpha == 0.0
    ) || matches!(color, PaintColor::Named(name) if name.eq_ignore_ascii_case("transparent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker::standard_element_registrations;
    use whisker_protocol::{
        ElementContentKind, FrameHeader, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
        MeasureTextDirection, MeasureTextOverflow, MeasureTextWrap, PaintCorners, PaintEdges,
        PaintLengthPercentage, ProtocolVersion, TextMeasurePayload, TextMeasureStyle, TextPaint,
    };

    fn element_type(content: ElementContentKind) -> ElementTypeId {
        standard_element_registrations()
            .into_iter()
            .find(|registration| registration.content == content)
            .expect("standard content registration")
            .element_type
    }

    fn scene(surface: SurfaceId) -> DesktopScene {
        DesktopScene::new(
            surface,
            DesktopElementRegistry::bind(
                &standard_element_registrations(),
                &crate::element::standard_desktop_element_factories(),
            )
            .unwrap(),
        )
    }

    fn id(value: u64) -> NodeId {
        NodeId::new(value).unwrap()
    }

    fn geometry(x: f32, y: f32, width: f32, height: f32) -> LayoutGeometry {
        LayoutGeometry {
            border_box: LayoutRect {
                x,
                y,
                width,
                height,
            },
            content_box: LayoutRect {
                x: 1.0,
                y: 2.0,
                width: (width - 2.0).max(0.0),
                height: (height - 4.0).max(0.0),
            },
        }
    }

    fn paint(color: PaintColor) -> BoxPaint {
        let zero = PaintLengthPercentage::default();
        BoxPaint {
            background_color: color,
            border_widths: PaintEdges {
                top: zero,
                right: zero,
                bottom: zero,
                left: zero,
            },
            border_colors: PaintEdges {
                top: PaintColor::default(),
                right: PaintColor::default(),
                bottom: PaintColor::default(),
                left: PaintColor::default(),
            },
            border_styles: PaintEdges {
                top: whisker_protocol::BorderLineStyle::None,
                right: whisker_protocol::BorderLineStyle::None,
                bottom: whisker_protocol::BorderLineStyle::None,
                left: whisker_protocol::BorderLineStyle::None,
            },
            border_radii: PaintCorners {
                top_left: zero,
                top_right: zero,
                bottom_right: zero,
                bottom_left: zero,
            },
        }
    }

    fn text() -> TextContent {
        TextContent {
            payload: TextMeasurePayload {
                text: "native".into(),
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
            paint: TextPaint::default(),
            prepared_content: None,
        }
    }

    fn packet(mode: FrameMode, base: u64, target: u64, operations: Vec<Operation>) -> FramePacket {
        FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: SurfaceId::new(1).unwrap(),
                scene_epoch: 1,
                frame_id: target,
                base_revision: base,
                target_revision: target,
                viewport_epoch: 1,
                mode,
            },
            operations,
        }
    }

    #[test]
    fn accepted_projection_lowers_content_geometry_clip_and_opacity() {
        let root = id(1);
        let child = id(2);
        let box_type = element_type(ElementContentKind::None);
        let text_type = element_type(ElementContentKind::Text);
        let mut scene = scene(SurfaceId::new(1).unwrap());
        let snapshot = packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: root,
                    element_type: box_type,
                },
                Operation::CreateNode {
                    node: child,
                    element_type: text_type,
                },
                Operation::InsertChild {
                    parent: root,
                    child,
                    index: 0,
                },
                Operation::SetLayout {
                    node: root,
                    geometry: geometry(4.0, 5.0, 100.0, 80.0),
                },
                Operation::SetLayout {
                    node: child,
                    geometry: geometry(2.0, 3.0, 20.0, 10.0),
                },
                Operation::SetBoxPaint {
                    node: root,
                    paint: paint(PaintColor::Named("red".into())),
                },
                Operation::SetClip {
                    node: root,
                    clip: BoxClip {
                        horizontal: OverflowClip::Hidden,
                        vertical: OverflowClip::Visible,
                    },
                },
                Operation::SetOpacity {
                    node: root,
                    opacity: 0.5,
                },
                Operation::SetOpacity {
                    node: child,
                    opacity: 0.5,
                },
                Operation::SetText {
                    node: child,
                    content: text(),
                },
            ],
        );
        assert_eq!(
            scene.present(&snapshot),
            Ok(ApplyResult::Accepted { revision: 1 })
        );
        let commands = scene.paint_commands();
        assert!(matches!(
            &commands[0],
            PaintCommand::Box { rect, opacity, .. }
                if *rect == LayoutRect { x: 4.0, y: 5.0, width: 100.0, height: 80.0 }
                    && *opacity == 0.5
        ));
        assert!(matches!(
            &commands[1],
            PaintCommand::Text { rect, clip, opacity, .. }
                if *rect == LayoutRect { x: 7.0, y: 10.0, width: 18.0, height: 6.0 }
                    && clip.left == Some(7.0)
                    && clip.right == Some(25.0)
                    && *opacity == 0.25
        ));
    }

    #[test]
    fn rejected_delta_does_not_partially_change_desktop_state() {
        let root = id(1);
        let element_type = element_type(ElementContentKind::None);
        let mut scene = scene(SurfaceId::new(1).unwrap());
        scene
            .present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![
                    Operation::CreateNode {
                        node: root,
                        element_type,
                    },
                    Operation::SetLayout {
                        node: root,
                        geometry: geometry(0.0, 0.0, 10.0, 10.0),
                    },
                    Operation::SetBoxPaint {
                        node: root,
                        paint: paint(PaintColor::Named("blue".into())),
                    },
                ],
            ))
            .unwrap();
        let before_len = scene.paint_commands().len();
        let invalid = LayoutGeometry {
            border_box: LayoutRect {
                width: f32::NAN,
                ..LayoutRect::default()
            },
            ..LayoutGeometry::default()
        };
        assert_eq!(
            scene.present(&packet(
                FrameMode::Delta,
                1,
                2,
                vec![Operation::SetLayout {
                    node: root,
                    geometry: invalid
                }],
            )),
            Err(DesktopPresentError::Protocol(
                ValidationError::NonFiniteNumber
            ))
        );
        assert_eq!(scene.paint_commands().len(), before_len);
        assert!(
            matches!(scene.paint_commands()[0], PaintCommand::Box { rect, .. } if rect.width == 10.0)
        );
    }

    #[test]
    fn unknown_element_type_rejects_the_whole_frame() {
        let root = id(1);
        let mut scene = scene(SurfaceId::new(1).unwrap());
        let unknown = ElementTypeId::new(900).unwrap();

        assert_eq!(
            scene.present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![Operation::CreateNode {
                    node: root,
                    element_type: unknown,
                }],
            )),
            Err(DesktopPresentError::Element(
                DesktopElementError::UnknownElementType {
                    element_type: unknown,
                }
            ))
        );

        assert_eq!(
            scene.present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![Operation::CreateNode {
                    node: root,
                    element_type: element_type(ElementContentKind::None),
                }],
            )),
            Ok(ApplyResult::Accepted { revision: 1 })
        );
    }

    #[test]
    fn text_content_operation_is_dispatched_by_registered_element_type() {
        let root = id(1);
        let mut scene = scene(SurfaceId::new(1).unwrap());

        assert_eq!(
            scene.present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![
                    Operation::CreateNode {
                        node: root,
                        element_type: element_type(ElementContentKind::None),
                    },
                    Operation::SetText {
                        node: root,
                        content: text(),
                    },
                ],
            )),
            Err(DesktopPresentError::Element(
                DesktopElementError::UnexpectedText { node: root }
            ))
        );

        assert_eq!(
            scene.present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![
                    Operation::CreateNode {
                        node: root,
                        element_type: element_type(ElementContentKind::Text),
                    },
                    Operation::SetText {
                        node: root,
                        content: text(),
                    },
                    Operation::SetBoxPaint {
                        node: root,
                        paint: paint(PaintColor::Named("green".into())),
                    },
                ],
            )),
            Ok(ApplyResult::Accepted { revision: 1 })
        );
        assert!(matches!(
            scene.paint_commands().as_slice(),
            [PaintCommand::Box { .. }, PaintCommand::Text { node, .. }] if *node == root
        ));
    }

    #[test]
    fn leaf_element_rejects_scene_children_without_partial_commit() {
        let parent = id(1);
        let child = id(2);
        let mut scene = scene(SurfaceId::new(1).unwrap());

        assert_eq!(
            scene.present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![
                    Operation::CreateNode {
                        node: parent,
                        element_type: element_type(ElementContentKind::Text),
                    },
                    Operation::CreateNode {
                        node: child,
                        element_type: element_type(ElementContentKind::None),
                    },
                    Operation::InsertChild {
                        parent,
                        child,
                        index: 0,
                    },
                ],
            )),
            Err(DesktopPresentError::Element(
                DesktopElementError::ChildrenNotAllowed { parent }
            ))
        );

        assert_eq!(
            scene.present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![Operation::CreateNode {
                    node: parent,
                    element_type: element_type(ElementContentKind::Text),
                }],
            )),
            Ok(ApplyResult::Accepted { revision: 1 })
        );
    }
}
