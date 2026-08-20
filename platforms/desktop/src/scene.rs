use std::collections::HashMap;

use whisker_engine::FrameSink;
use whisker_protocol::{
    ApplyResult, BoxClip, BoxPaint, FrameMode, FramePacket, LayoutGeometry, LayoutRect, NodeId,
    Operation, OverflowClip, PaintColor, SceneProjection, SurfaceId, TextContent, ValidationError,
    Visibility,
};

#[derive(Clone, Debug)]
struct RenderNode {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    layout: LayoutGeometry,
    paint: Option<BoxPaint>,
    clip: BoxClip,
    opacity: f32,
    visibility: Visibility,
    z_order: i32,
    text: Option<TextContent>,
}

impl Default for RenderNode {
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
            text: None,
        }
    }
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
    nodes: HashMap<NodeId, RenderNode>,
}

impl DesktopScene {
    pub(crate) fn new(surface: SurfaceId) -> Self {
        Self {
            validation: SceneProjection::new(surface),
            nodes: HashMap::new(),
        }
    }

    pub(crate) fn paint_commands(&self) -> Vec<PaintCommand<'_>> {
        let mut roots = self
            .nodes
            .iter()
            .filter_map(|(id, node)| node.parent.is_none().then_some((*id, node.z_order)))
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
        let border = LayoutRect {
            x: parent_x + node.layout.border_box.x,
            y: parent_y + node.layout.border_box.y,
            width: node.layout.border_box.width,
            height: node.layout.border_box.height,
        };
        let opacity = ancestor_opacity * node.opacity;
        if node.visibility == Visibility::Visible {
            if let Some(paint) = &node.paint {
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
            node.clip.horizontal == OverflowClip::Hidden,
            node.clip.vertical == OverflowClip::Hidden,
        );
        if node.visibility == Visibility::Visible
            && let Some(content) = &node.text
        {
            let content_rect = LayoutRect {
                x: border.x + node.layout.content_box.x,
                y: border.y + node.layout.content_box.y,
                width: node.layout.content_box.width,
                height: node.layout.content_box.height,
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
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let z = self
                    .nodes
                    .get(child)
                    .expect("retained child remains live")
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

    fn apply_operations(&mut self, packet: &FramePacket) {
        if packet.header.mode == FrameMode::Snapshot {
            self.nodes.clear();
        }
        for operation in &packet.operations {
            match operation {
                Operation::CreateNode { node, .. } => {
                    self.nodes.insert(*node, RenderNode::default());
                }
                Operation::DeleteNode { node } => self.delete_subtree(*node),
                Operation::InsertChild {
                    parent,
                    child,
                    index,
                } => {
                    self.nodes.get_mut(child).expect("validated child").parent = Some(*parent);
                    self.nodes
                        .get_mut(parent)
                        .expect("validated parent")
                        .children
                        .insert(*index as usize, *child);
                }
                Operation::RemoveChild { parent, child } => {
                    self.nodes.get_mut(child).expect("validated child").parent = None;
                    self.nodes
                        .get_mut(parent)
                        .expect("validated parent")
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
                        .children;
                    let old = children
                        .iter()
                        .position(|candidate| candidate == child)
                        .expect("validated direct child");
                    children.remove(old);
                    children.insert(*index as usize, *child);
                }
                Operation::SetLayout { node, geometry } => {
                    self.nodes.get_mut(node).expect("validated node").layout = *geometry;
                }
                Operation::SetBoxPaint { node, paint } => {
                    self.nodes.get_mut(node).expect("validated node").paint = Some(paint.clone());
                }
                Operation::SetClip { node, clip } => {
                    self.nodes.get_mut(node).expect("validated node").clip = *clip;
                }
                Operation::SetOpacity { node, opacity } => {
                    self.nodes.get_mut(node).expect("validated node").opacity = *opacity;
                }
                Operation::SetVisibility { node, visibility } => {
                    self.nodes.get_mut(node).expect("validated node").visibility = *visibility;
                }
                Operation::SetZOrder { node, z_order } => {
                    self.nodes.get_mut(node).expect("validated node").z_order = *z_order;
                }
                Operation::SetText { node, content } => {
                    self.nodes.get_mut(node).expect("validated node").text = Some(content.clone());
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
        if let Some(parent) = removed.parent
            && let Some(parent) = self.nodes.get_mut(&parent)
        {
            parent.children.retain(|candidate| *candidate != node);
        }
        for child in removed.children {
            self.delete_subtree(child);
        }
    }
}

impl FrameSink for DesktopScene {
    type Error = ValidationError;

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        let result = self.validation.apply(packet)?;
        if matches!(result, ApplyResult::Accepted { .. }) {
            // The reference projection has validated the complete transaction,
            // so every lookup and index below is now infallible. Applying in
            // place avoids cloning the retained Host tree on every delta while
            // preserving atomic rejection for malformed packets.
            self.apply_operations(packet);
        }
        Ok(result)
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
    use whisker_protocol::{
        ElementTypeId, FrameHeader, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
        MeasureTextDirection, MeasureTextOverflow, MeasureTextWrap, PaintCorners, PaintEdges,
        PaintLengthPercentage, ProtocolVersion, TextMeasurePayload, TextMeasureStyle, TextPaint,
    };

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
        let element_type = ElementTypeId::new(1).unwrap();
        let mut scene = DesktopScene::new(SurfaceId::new(1).unwrap());
        let snapshot = packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: root,
                    element_type,
                },
                Operation::CreateNode {
                    node: child,
                    element_type,
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
        let element_type = ElementTypeId::new(1).unwrap();
        let mut scene = DesktopScene::new(SurfaceId::new(1).unwrap());
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
            Err(ValidationError::NonFiniteNumber)
        );
        assert_eq!(scene.paint_commands().len(), before_len);
        assert!(
            matches!(scene.paint_commands()[0], PaintCommand::Box { rect, .. } if rect.width == 10.0)
        );
    }
}
