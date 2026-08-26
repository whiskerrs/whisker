use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use whisker_engine::FrameSink;
use whisker_protocol::{
    ApplyResult, BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, BoxClip,
    BoxPaint, ClipShape, ElementTypeId, FrameMode, FramePacket, ImageRepeat, LayoutGeometry,
    LayoutRect, NodeId, Operation, OverflowClip, PaintBox, PaintColor, PaintCoordinate, PaintImage,
    RadialGradientExtent, ResourceId, SceneProjection, SurfaceId, TextContent, Transform,
    ValidationError, Visibility, VisualEffects, WhiskerValue,
};

use crate::element::{DesktopElementContent, DesktopElementError, DesktopElementRegistry};
use crate::paint::box_paint::{ResolvedRadii, resolve_box_geometry, resolve_radii};

#[derive(Clone, Debug)]
struct CommonPresentation {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    layout: LayoutGeometry,
    paint: Option<BoxPaint>,
    background_layers: Vec<BackgroundLayer>,
    visual_effects: VisualEffects,
    clip: BoxClip,
    transform: Transform,
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
            background_layers: Vec::new(),
            visual_effects: VisualEffects::default(),
            clip: BoxClip {
                horizontal: OverflowClip::Visible,
                vertical: OverflowClip::Visible,
            },
            transform: Transform::IDENTITY,
            opacity: 1.0,
            visibility: Visibility::Visible,
            z_order: 0,
        }
    }
}

#[derive(Debug)]
struct RenderNode {
    element_type: ElementTypeId,
    presentation: CommonPresentation,
    content: DesktopElementContent,
    event_mask: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DesktopProviderEvent {
    pub(crate) target: NodeId,
    pub(crate) name: String,
    pub(crate) detail: WhiskerValue,
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapeClip {
    pub(crate) rect: LayoutRect,
    pub(crate) radii: ResolvedRadii,
    pub(crate) inverse_transform: Transform,
    pub(crate) horizontal: bool,
    pub(crate) vertical: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ShapeClipStack(Option<Arc<ShapeClipNode>>);

#[derive(Debug)]
struct ShapeClipNode {
    parent: ShapeClipStack,
    clip: ShapeClip,
}

impl ShapeClipStack {
    fn push(&self, clip: ShapeClip) -> Self {
        Self(Some(Arc::new(ShapeClipNode {
            parent: self.clone(),
            clip,
        })))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = ShapeClip> + '_ {
        std::iter::successors(self.0.as_deref(), |node| node.parent.0.as_deref())
            .map(|node| node.clip)
    }
}

#[derive(Clone, Debug)]
struct PresentationContext {
    origin: [f32; 2],
    transform: Transform,
    clip: LogicalClip,
    shape_clips: ShapeClipStack,
    opacity: f32,
}

impl Default for PresentationContext {
    fn default() -> Self {
        Self {
            origin: [0.0; 2],
            transform: Transform::IDENTITY,
            clip: LogicalClip::default(),
            shape_clips: ShapeClipStack::default(),
            opacity: 1.0,
        }
    }
}

fn multiply_transform(left: Transform, right: Transform) -> Transform {
    let mut result = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            result[column * 4 + row] = (0..4)
                .map(|index| left.0[index * 4 + row] * right.0[column * 4 + index])
                .sum();
        }
    }
    Transform(result)
}

fn translation(x: f32, y: f32) -> Transform {
    let mut result = Transform::IDENTITY;
    result.0[12] = x;
    result.0[13] = y;
    result
}

fn transform_around(transform: Transform, x: f32, y: f32) -> Transform {
    multiply_transform(
        multiply_transform(translation(x, y), transform),
        translation(-x, -y),
    )
}

fn inverse_transform(transform: Transform) -> Option<Transform> {
    let mut rows = [[0.0_f32; 8]; 4];
    for (row, values) in rows.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().take(4).enumerate() {
            *value = transform.0[column * 4 + row];
        }
        values[4 + row] = 1.0;
    }
    for column in 0..4 {
        let pivot = (column..4).max_by(|left, right| {
            rows[*left][column]
                .abs()
                .total_cmp(&rows[*right][column].abs())
        })?;
        if rows[pivot][column].abs() <= f32::EPSILON {
            return None;
        }
        rows.swap(column, pivot);
        let scale = rows[column][column];
        for value in &mut rows[column] {
            *value /= scale;
        }
        let pivot_row = rows[column];
        for (row, values) in rows.iter_mut().enumerate() {
            if row == column {
                continue;
            }
            let factor = values[column];
            for index in 0..8 {
                values[index] -= factor * pivot_row[index];
            }
        }
    }
    let mut inverse = [0.0; 16];
    for (row, values) in rows.iter().enumerate() {
        for column in 0..4 {
            inverse[column * 4 + row] = values[4 + column];
        }
    }
    Some(Transform(inverse))
}

fn transform_rect_aabb(rect: LayoutRect, transform: Transform) -> Option<LayoutRect> {
    let mut minimum = [f32::INFINITY; 2];
    let mut maximum = [f32::NEG_INFINITY; 2];
    for [x, y] in [
        [rect.x, rect.y],
        [rect.x + rect.width, rect.y],
        [rect.x + rect.width, rect.y + rect.height],
        [rect.x, rect.y + rect.height],
    ] {
        let transformed_x = transform.0[0] * x + transform.0[4] * y + transform.0[12];
        let transformed_y = transform.0[1] * x + transform.0[5] * y + transform.0[13];
        let transformed_w = transform.0[3] * x + transform.0[7] * y + transform.0[15];
        if transformed_w.abs() <= f32::EPSILON {
            return None;
        }
        let point = [transformed_x / transformed_w, transformed_y / transformed_w];
        if !point.into_iter().all(f32::is_finite) {
            return None;
        }
        for axis in 0..2 {
            minimum[axis] = minimum[axis].min(point[axis]);
            maximum[axis] = maximum[axis].max(point[axis]);
        }
    }
    Some(LayoutRect {
        x: minimum[0],
        y: minimum[1],
        width: maximum[0] - minimum[0],
        height: maximum[1] - minimum[1],
    })
}

fn preserves_screen_axes(transform: Transform) -> bool {
    transform.0[1].abs() <= f32::EPSILON
        && transform.0[4].abs() <= f32::EPSILON
        && transform.0[3].abs() <= f32::EPSILON
        && transform.0[7].abs() <= f32::EPSILON
}

#[derive(Clone, Debug)]
pub(crate) enum PaintCommand<'a> {
    Box {
        rect: LayoutRect,
        content_rect: LayoutRect,
        paint: Option<&'a BoxPaint>,
        background_layers: &'a [BackgroundLayer],
        visual_effects: &'a VisualEffects,
        clip: LogicalClip,
        shape_clips: ShapeClipStack,
        transform: Transform,
        opacity: f32,
    },
    Text {
        node: NodeId,
        rect: LayoutRect,
        content: &'a TextContent,
        clip: LogicalClip,
        shape_clips: ShapeClipStack,
        transform: Transform,
        opacity: f32,
    },
}

#[derive(Debug)]
pub(crate) struct DesktopScene {
    validation: SceneProjection,
    elements: DesktopElementRegistry,
    nodes: HashMap<NodeId, RenderNode>,
    pending_events: Vec<DesktopProviderEvent>,
    raster_resources: HashSet<ResourceId>,
}

impl DesktopScene {
    pub(crate) fn new(surface: SurfaceId, elements: DesktopElementRegistry) -> Self {
        Self {
            validation: SceneProjection::new(surface),
            elements,
            nodes: HashMap::new(),
            pending_events: Vec::new(),
            raster_resources: HashSet::new(),
        }
    }

    pub(crate) fn register_raster_resource(&mut self, resource: ResourceId) {
        self.raster_resources.insert(resource);
    }

    pub(crate) fn release_raster_resource(&mut self, resource: ResourceId) {
        self.raster_resources.remove(&resource);
    }

    pub(crate) fn take_events(&mut self) -> Vec<DesktopProviderEvent> {
        std::mem::take(&mut self.pending_events)
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
            self.collect_commands(root, PresentationContext::default(), &mut commands);
        }
        commands
    }

    fn collect_commands<'a>(
        &'a self,
        id: NodeId,
        context: PresentationContext,
        commands: &mut Vec<PaintCommand<'a>>,
    ) {
        let node = self.nodes.get(&id).expect("retained child remains live");
        let presentation = &node.presentation;
        if presentation.visibility == Visibility::Hidden {
            return;
        }
        let border = LayoutRect {
            x: context.origin[0] + presentation.layout.border_box.x,
            y: context.origin[1] + presentation.layout.border_box.y,
            width: presentation.layout.border_box.width,
            height: presentation.layout.border_box.height,
        };
        let opacity = context.opacity * presentation.opacity;
        let content = LayoutRect {
            x: border.x + presentation.layout.content_box.x,
            y: border.y + presentation.layout.content_box.y,
            width: presentation.layout.content_box.width,
            height: presentation.layout.content_box.height,
        };
        let transform = multiply_transform(
            context.transform,
            transform_around(presentation.transform, border.x, border.y),
        );
        let mut node_shape_clips = context.shape_clips.clone();
        let mut node_clip_bounds = None;
        if let Some((reference_box, shape)) = presentation.visual_effects.clip_path.as_ref() {
            let reference = match reference_box {
                PaintBox::Border => border,
                PaintBox::Padding => presentation.paint.as_ref().map_or(border, |paint| {
                    resolve_box_geometry(border, paint).inner_rect
                }),
                PaintBox::Content => content,
                _ => unreachable!("unsupported clip-path reference box passed validation"),
            };
            let (clip_rect, clip_radii) = clip_shape_geometry(reference, shape);
            node_shape_clips = node_shape_clips.push(ShapeClip {
                rect: clip_rect,
                radii: clip_radii,
                inverse_transform: inverse_transform(transform).unwrap_or(Transform::IDENTITY),
                horizontal: true,
                vertical: true,
            });
            node_clip_bounds = transform_rect_aabb(clip_rect, transform);
        }
        if presentation.paint.is_some()
            || !presentation.background_layers.is_empty()
            || !presentation.visual_effects.box_shadows.is_empty()
        {
            commands.push(PaintCommand::Box {
                rect: border,
                content_rect: content,
                paint: presentation.paint.as_ref(),
                background_layers: &presentation.background_layers,
                visual_effects: &presentation.visual_effects,
                clip: context.clip,
                shape_clips: node_shape_clips.clone(),
                transform,
                opacity,
            });
        }

        let clip_horizontal = presentation.clip.horizontal == OverflowClip::Hidden;
        let clip_vertical = presentation.clip.vertical == OverflowClip::Hidden;
        let mut descendant_clip = context.clip;
        if let Some(bounds) = node_clip_bounds {
            descendant_clip = descendant_clip.intersect(bounds, true, true);
        }
        let mut descendant_shape_clips = node_shape_clips;
        if clip_horizontal || clip_vertical {
            let geometry = presentation.paint.as_ref().map_or_else(
                || crate::paint::box_paint::BoxGeometry {
                    outer_rect: border,
                    outer_radii: ResolvedRadii {
                        horizontal: [0.0; 4],
                        vertical: [0.0; 4],
                    },
                    inner_rect: border,
                    inner_radii: ResolvedRadii {
                        horizontal: [0.0; 4],
                        vertical: [0.0; 4],
                    },
                    border_widths: [0.0; 4],
                },
                |paint| resolve_box_geometry(border, paint),
            );
            descendant_shape_clips = descendant_shape_clips.push(ShapeClip {
                rect: geometry.inner_rect,
                radii: geometry.inner_radii,
                inverse_transform: inverse_transform(transform).unwrap_or(Transform::IDENTITY),
                horizontal: clip_horizontal,
                vertical: clip_vertical,
            });
            if let Some(bounds) = transform_rect_aabb(geometry.inner_rect, transform) {
                let axis_aligned = preserves_screen_axes(transform);
                descendant_clip = descendant_clip.intersect(
                    bounds,
                    clip_horizontal && (clip_vertical || axis_aligned),
                    clip_vertical && (clip_horizontal || axis_aligned),
                );
            }
        }
        if let Some(content) = node.content.text() {
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
                shape_clips: descendant_shape_clips.clone(),
                transform,
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
                PresentationContext {
                    origin: [border.x, border.y],
                    transform,
                    clip: descendant_clip,
                    shape_clips: descendant_shape_clips.clone(),
                    opacity,
                },
                commands,
            );
        }
    }

    fn validate_element_operations(&self, packet: &FramePacket) -> Result<(), DesktopPresentError> {
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
                        && !self.elements.child_policy(element_type)?.accepts_elements()
                    {
                        return Err(
                            DesktopElementError::ChildrenNotAllowed { parent: *parent }.into()
                        );
                    }
                }
                Operation::SetText { node, content } => {
                    if content.paint.uses_extended_features() {
                        return Err(DesktopPresentError::Unsupported("text-effects"));
                    }
                    if content.payload.style.uses_extended_typography() {
                        return Err(DesktopPresentError::Unsupported("text-typography"));
                    }
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements
                            .create(element_type)?
                            .set_text(*node, content.clone())?;
                    }
                }
                Operation::SetProperty {
                    node,
                    property,
                    value,
                } => {
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements.validate_property(
                            element_type,
                            *node,
                            *property,
                            Some(value),
                        )?;
                    }
                }
                Operation::ClearProperty { node, property } => {
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements
                            .validate_property(element_type, *node, *property, None)?;
                    }
                }
                Operation::InvokeCommand {
                    node,
                    command,
                    arguments,
                    ..
                } => {
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements
                            .validate_command(element_type, *node, *command, arguments)?;
                    }
                }
                Operation::SetBackgroundLayers { layers, .. } => {
                    if !layers.iter().all(|layer| {
                        supports_basic_background_layer(layer)
                            && match &layer.image {
                                PaintImage::Resource(resource) => {
                                    self.raster_resources.contains(resource)
                                }
                                _ => true,
                            }
                    }) {
                        return Err(DesktopPresentError::Unsupported("background-layers"));
                    }
                }
                Operation::SetVisualEffects { effects, .. } => {
                    if !supports_visual_effects(effects) {
                        return Err(DesktopPresentError::Unsupported("visual-effects"));
                    }
                }
                Operation::SetImage { .. } => {
                    return Err(DesktopPresentError::Unsupported("image-content"));
                }
                Operation::SetCursor { .. } => {
                    return Err(DesktopPresentError::Unsupported("cursor"));
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
            self.pending_events.clear();
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
                            event_mask: 0,
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
                Operation::SetBackgroundLayers { node, layers } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .background_layers = layers.clone();
                }
                Operation::SetVisualEffects { node, effects } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .visual_effects = effects.clone();
                }
                Operation::SetClip { node, clip } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .clip = *clip;
                }
                Operation::SetTransform { node, transform } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .transform = *transform;
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
                Operation::SetProperty {
                    node,
                    property,
                    value,
                } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .content
                        .set_property(*node, *property, value)
                        .expect("element property was validated before commit");
                }
                Operation::ClearProperty { node, property } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .content
                        .clear_property(*node, *property)
                        .expect("element property was validated before commit");
                }
                Operation::SetEventMask { node, event_mask } => {
                    self.nodes.get_mut(node).expect("validated node").event_mask = *event_mask;
                }
                Operation::InvokeCommand {
                    node,
                    command,
                    arguments,
                    ..
                } => {
                    let state = self.nodes.get_mut(node).expect("validated node");
                    let element_type = state.element_type;
                    let event_mask = state.event_mask;
                    let event = state
                        .content
                        .invoke_command(*node, *command, arguments)
                        .expect("element command was validated before commit");
                    if let Some(event) = event {
                        let resolved =
                            self.elements
                                .event(element_type, *node, &event.event, &event.detail);
                        debug_assert!(resolved.is_ok(), "native element emitted invalid event");
                        if let Ok((name, mask)) = resolved
                            && event_mask & mask != 0
                        {
                            self.pending_events.push(DesktopProviderEvent {
                                target: *node,
                                name,
                                detail: event.detail,
                            });
                        }
                    }
                }
                Operation::SetHitTest { .. }
                | Operation::SetPointerCapture { .. }
                | Operation::ReleasePointerCapture { .. } => {}
                Operation::SetImage { .. } | Operation::SetCursor { .. } => {
                    unreachable!("unsupported operations are rejected before commit")
                }
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

    fn capabilities(&self) -> whisker_protocol::RenderCapabilities {
        whisker_protocol::RenderCapabilities::new(
            whisker_protocol::ProtocolVersion::CURRENT,
            [
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::EllipticalBorderRadius,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::VisualEffects,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::LinearGradients,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::RadialGradients,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::ConicGradients,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::BackgroundGeometry,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::BackgroundLayerStacking,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::BackgroundImageResources,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
            ],
        )
        .expect("Desktop capability profile is unique")
    }

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        if let Some(capability) = self.capabilities().first_unsupported(packet) {
            return Err(DesktopPresentError::Unsupported(capability.as_str()));
        }
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
    Unsupported(&'static str),
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
            Self::Unsupported(_) => None,
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

fn supports_basic_background_layer(layer: &BackgroundLayer) -> bool {
    let resource_image = matches!(&layer.image, PaintImage::Resource(_));
    let supported_image = matches!(
        &layer.image,
        PaintImage::LinearGradient {
            repeating: false,
            stops,
            ..
        } if stops.iter().all(|stop| stop.position.is_some())
    ) || matches!(
        &layer.image,
        PaintImage::RadialGradient {
            shape: whisker_protocol::RadialGradientShape::Ellipse,
            extent: RadialGradientExtent::Explicit,
            radii: Some(_),
            repeating: false,
            stops,
            ..
        } if stops.iter().all(|stop| stop.position.is_some())
    ) || matches!(
        &layer.image,
        PaintImage::ConicGradient {
            repeating: false,
            stops,
            ..
        } if stops.iter().all(|stop| {
            stop.position.is_some_and(|position| position.length == 0.0)
        })
    ) || resource_image;
    let initial_geometry = layer.position == Default::default()
        && layer.size == BackgroundSize::Auto
        && layer.repeat_x == ImageRepeat::Repeat
        && layer.repeat_y == ImageRepeat::Repeat
        && layer.origin == PaintBox::Padding
        && layer.clip == PaintBox::Border;
    let supported_geometry = match layer.size {
        BackgroundSize::Auto => resource_image || initial_geometry,
        BackgroundSize::Cover | BackgroundSize::Contain => resource_image,
        BackgroundSize::Explicit { width, height } => {
            width.is_some() && height.is_some()
                || resource_image && (width.is_some() || height.is_some())
        }
    };
    supported_image
        && supported_geometry
        && layer.attachment == BackgroundAttachment::Scroll
        && layer.blend_mode == BlendMode::Normal
}

fn supports_visual_effects(effects: &VisualEffects) -> bool {
    let mut remainder = effects.clone();
    remainder.box_shadows.clear();
    remainder.clip_path = None;
    remainder == VisualEffects::default()
        && effects.clip_path.as_ref().is_none_or(|(reference, shape)| {
            matches!(
                reference,
                PaintBox::Border | PaintBox::Padding | PaintBox::Content
            ) && matches!(
                shape,
                ClipShape::Inset { .. } | ClipShape::Circle { .. } | ClipShape::Ellipse { .. }
            )
        })
}

fn clip_shape_geometry(reference: LayoutRect, shape: &ClipShape) -> (LayoutRect, ResolvedRadii) {
    match shape {
        ClipShape::Inset { edges, radii } => {
            let rect = inset_clip_rect(reference, edges);
            let radii = resolve_radii(radii, rect);
            (rect, radii)
        }
        ClipShape::Circle { radius, center } => {
            let center_x = reference.x + resolve_coordinate(center.x, reference.width);
            let center_y = reference.y + resolve_coordinate(center.y, reference.height);
            let normalized_diagonal = reference.width.hypot(reference.height) / 2.0_f32.sqrt();
            let radius = resolve_length_percentage(*radius, normalized_diagonal);
            let rect = LayoutRect {
                x: center_x - radius,
                y: center_y - radius,
                width: radius * 2.0,
                height: radius * 2.0,
            };
            (
                rect,
                ResolvedRadii {
                    horizontal: [radius; 4],
                    vertical: [radius; 4],
                },
            )
        }
        ClipShape::Ellipse {
            radius_x,
            radius_y,
            center,
        } => {
            let center_x = reference.x + resolve_coordinate(center.x, reference.width);
            let center_y = reference.y + resolve_coordinate(center.y, reference.height);
            let radius_x = resolve_length_percentage(*radius_x, reference.width);
            let radius_y = resolve_length_percentage(*radius_y, reference.height);
            (
                LayoutRect {
                    x: center_x - radius_x,
                    y: center_y - radius_y,
                    width: radius_x * 2.0,
                    height: radius_y * 2.0,
                },
                ResolvedRadii {
                    horizontal: [radius_x; 4],
                    vertical: [radius_y; 4],
                },
            )
        }
        _ => unreachable!("unsupported clip-path shape passed validation"),
    }
}

fn inset_clip_rect(
    reference: LayoutRect,
    edges: &whisker_protocol::PaintEdges<PaintCoordinate>,
) -> LayoutRect {
    let top = resolve_coordinate(edges.top, reference.height);
    let right = resolve_coordinate(edges.right, reference.width);
    let bottom = resolve_coordinate(edges.bottom, reference.height);
    let left = resolve_coordinate(edges.left, reference.width);
    LayoutRect {
        x: reference.x + left,
        y: reference.y + top,
        width: (reference.width - left - right).max(0.0),
        height: (reference.height - top - bottom).max(0.0),
    }
}

fn resolve_coordinate(value: PaintCoordinate, available: f32) -> f32 {
    value.length + value.fraction * available
}

fn resolve_length_percentage(
    value: whisker_protocol::PaintLengthPercentage,
    available: f32,
) -> f32 {
    value.length + value.fraction * available
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
    use crate::element::{
        DesktopElementFactory, DesktopNativeElement, DesktopNativeEvent, built_in_element_factories,
    };
    use whisker::standard_element_registrations;
    use whisker_protocol::{
        CommandId, ElementCommandSchema, ElementEventSchema, ElementMeasurement,
        ElementPropertySchema, ElementRegistration, ElementValueKind, EventId, FrameHeader,
        MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection,
        MeasureTextOverflow, MeasureTextWrap, PaintCorners, PaintEdges, PaintLengthPercentage,
        PropertyId, ProtocolVersion, TextMeasurePayload, TextMeasureStyle, TextPaint,
    };

    fn element_type(name: &str) -> ElementTypeId {
        standard_element_registrations()
            .into_iter()
            .find(|registration| registration.name == name)
            .expect("standard element registration")
            .element_type
    }

    fn scene(surface: SurfaceId) -> DesktopScene {
        DesktopScene::new(
            surface,
            DesktopElementRegistry::bind(
                &standard_element_registrations(),
                &crate::element::built_in_element_factories(),
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
                top_left: whisker_protocol::PaintCornerRadius::circular(zero),
                top_right: whisker_protocol::PaintCornerRadius::circular(zero),
                bottom_right: whisker_protocol::PaintCornerRadius::circular(zero),
                bottom_left: whisker_protocol::PaintCornerRadius::circular(zero),
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
                    ..TextMeasureStyle::default()
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

    const CHECKED: PropertyId = PropertyId::new(1).unwrap();
    const DISABLED: PropertyId = PropertyId::new(2).unwrap();
    const CHANGE: EventId = EventId::new(1).unwrap();
    const TOGGLE: CommandId = CommandId::new(1).unwrap();

    #[derive(Debug, Default)]
    struct ToggleNative {
        checked: bool,
        disabled: bool,
    }

    impl DesktopNativeElement for ToggleNative {
        fn set_property(&mut self, property: PropertyId, value: &WhiskerValue) {
            let WhiskerValue::Bool(value) = value else {
                unreachable!()
            };
            match property {
                CHECKED => self.checked = *value,
                DISABLED => self.disabled = *value,
                _ => unreachable!(),
            }
        }

        fn clear_property(&mut self, property: PropertyId) {
            match property {
                CHECKED => self.checked = false,
                DISABLED => self.disabled = false,
                _ => unreachable!(),
            }
        }

        fn invoke_command(
            &mut self,
            command: CommandId,
            _arguments: &WhiskerValue,
        ) -> Option<DesktopNativeEvent> {
            assert_eq!(command, TOGGLE);
            if self.disabled {
                return None;
            }
            self.checked = !self.checked;
            Some(DesktopNativeEvent {
                event: "change".into(),
                detail: WhiskerValue::map([("checked", WhiskerValue::Bool(self.checked))]),
            })
        }
    }

    fn toggle_scene() -> (DesktopScene, ElementTypeId) {
        let element_type = ElementTypeId::new(20).unwrap();
        let mut registrations = standard_element_registrations();
        registrations.push(ElementRegistration {
            element_type,
            name: "whisker.test/Toggle".into(),
            child_policy: whisker_protocol::ChildPolicy::None,
            measurement: ElementMeasurement::None,
            properties: vec![
                ElementPropertySchema {
                    property: CHECKED,
                    name: "checked".into(),
                    value: ElementValueKind::Bool,
                },
                ElementPropertySchema {
                    property: DISABLED,
                    name: "disabled".into(),
                    value: ElementValueKind::Bool,
                },
            ],
            events: vec![ElementEventSchema {
                event: CHANGE,
                name: "change".into(),
                detail: Some(ElementValueKind::Map),
            }],
            commands: vec![ElementCommandSchema {
                command: TOGGLE,
                name: "toggle".into(),
                arguments: ElementValueKind::Null,
            }],
        });
        let mut factories = built_in_element_factories();
        factories.push(DesktopElementFactory::native("whisker.test/Toggle", || {
            Box::<ToggleNative>::default()
        }));
        (
            DesktopScene::new(
                SurfaceId::new(1).unwrap(),
                DesktopElementRegistry::bind(&registrations, &factories).unwrap(),
            ),
            element_type,
        )
    }

    #[test]
    fn native_toggle_applies_properties_invokes_command_and_routes_change() {
        let node = id(1);
        let (mut scene, element_type) = toggle_scene();
        assert_eq!(
            scene.present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![
                    Operation::CreateNode { node, element_type },
                    Operation::SetEventMask {
                        node,
                        event_mask: 1,
                    },
                    Operation::SetProperty {
                        node,
                        property: CHECKED,
                        value: WhiskerValue::Bool(true),
                    },
                    Operation::InvokeCommand {
                        node,
                        command: TOGGLE,
                        arguments: WhiskerValue::Null,
                        result: None,
                    },
                ],
            )),
            Ok(ApplyResult::Accepted { revision: 1 })
        );
        assert_eq!(
            scene.take_events(),
            vec![DesktopProviderEvent {
                target: node,
                name: "change".into(),
                detail: WhiskerValue::map([("checked", WhiskerValue::Bool(false),)]),
            }]
        );
    }

    #[test]
    fn native_toggle_rejects_wrong_property_shape_before_commit() {
        let node = id(1);
        let (mut scene, element_type) = toggle_scene();
        assert_eq!(
            scene.present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![
                    Operation::CreateNode { node, element_type },
                    Operation::SetProperty {
                        node,
                        property: CHECKED,
                        value: WhiskerValue::String("true".into()),
                    },
                ],
            )),
            Err(DesktopPresentError::Element(
                DesktopElementError::InvalidPropertyValue {
                    node,
                    property: CHECKED,
                    expected: ElementValueKind::Bool,
                }
            ))
        );
        assert!(scene.nodes.is_empty());
    }

    #[test]
    fn unregistered_background_resource_rejects_the_whole_frame() {
        let node = id(1);
        let mut scene = scene(SurfaceId::new(1).unwrap());
        let resource = ResourceId::new(u64::MAX).unwrap();
        let layer = BackgroundLayer {
            image: PaintImage::Resource(resource),
            position: Default::default(),
            size: BackgroundSize::Auto,
            repeat_x: ImageRepeat::Repeat,
            repeat_y: ImageRepeat::Repeat,
            origin: PaintBox::Padding,
            clip: PaintBox::Border,
            attachment: BackgroundAttachment::Scroll,
            blend_mode: BlendMode::Normal,
        };
        let frame = packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node,
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                },
                Operation::SetBackgroundLayers {
                    node,
                    layers: vec![layer.clone()],
                },
            ],
        );

        assert_eq!(
            scene.present(&frame),
            Err(DesktopPresentError::Unsupported("background-layers"))
        );
        assert!(scene.nodes.is_empty());

        scene.register_raster_resource(resource);
        assert_eq!(
            scene.present(&frame),
            Ok(ApplyResult::Accepted { revision: 1 })
        );
        assert_eq!(
            scene.nodes[&node].presentation.background_layers,
            vec![layer]
        );
    }

    #[test]
    fn accepted_projection_lowers_content_geometry_clip_and_opacity() {
        let root = id(1);
        let child = id(2);
        let box_type = element_type(whisker::VIEW_ELEMENT_NAME);
        let text_type = element_type(whisker::TEXT_ELEMENT_NAME);
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
        let element_type = element_type(whisker::VIEW_ELEMENT_NAME);
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
    fn unsupported_visual_payload_is_rejected_before_desktop_commit() {
        let root = id(1);
        let mut scene = scene(SurfaceId::new(1).unwrap());
        scene
            .present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![Operation::CreateNode {
                    node: root,
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                }],
            ))
            .unwrap();

        assert_eq!(
            scene.present(&packet(
                FrameMode::Delta,
                1,
                2,
                vec![Operation::SetVisualEffects {
                    node: root,
                    effects: whisker_protocol::VisualEffects {
                        blend_mode: whisker_protocol::BlendMode::Multiply,
                        ..Default::default()
                    },
                }],
            )),
            Err(DesktopPresentError::Unsupported("visual-effects"))
        );
        assert_eq!(scene.validation.revision(), 1);
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
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
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
                        element_type: element_type(whisker::VIEW_ELEMENT_NAME),
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
                        element_type: element_type(whisker::TEXT_ELEMENT_NAME),
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
                        element_type: element_type(whisker::TEXT_ELEMENT_NAME),
                    },
                    Operation::CreateNode {
                        node: child,
                        element_type: element_type(whisker::VIEW_ELEMENT_NAME),
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
                    element_type: element_type(whisker::TEXT_ELEMENT_NAME),
                }],
            )),
            Ok(ApplyResult::Accepted { revision: 1 })
        );
    }
}
