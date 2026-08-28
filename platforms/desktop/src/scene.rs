use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

use whisker::runtime::RuntimeWakeHandle;
use whisker_engine::FrameSink;
use whisker_protocol::{
    ApplyResult, BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, BoxClip,
    BoxPaint, ClipShape, Cursor, ElementTypeId, FillRule, FrameMode, FramePacket, HitTestBehavior,
    ImageRepeat, LayoutGeometry, LayoutRect, NodeId, Operation, OverflowClip, PaintBox, PaintColor,
    PaintCoordinate, PaintImage, PaintPosition, PathCommand, RadialGradientExtent, ResourceId,
    SceneProjection, SurfaceId, TextContent, Transform, ValidationError, Visibility, VisualEffects,
    WhiskerValue,
};

use crate::element::{
    DesktopElementContent, DesktopElementError, DesktopElementRegistry, DesktopEventEmitter,
};
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
    hit_test: HitTestBehavior,
    cursor: Cursor,
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
            hit_test: HitTestBehavior::Auto,
            cursor: Cursor::default(),
        }
    }
}

#[derive(Debug)]
struct RenderNode {
    element_type: ElementTypeId,
    presentation: CommonPresentation,
    content: DesktopElementContent,
    event_mask: u64,
    scroll_offset: [f32; 2],
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
    pub(crate) fn intersect(self, rect: LayoutRect, horizontal: bool, vertical: bool) -> Self {
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
pub(crate) struct PathSegment {
    pub(crate) from: [f32; 2],
    pub(crate) to: [f32; 2],
}

#[derive(Clone, Debug)]
pub(crate) struct ShapeClip {
    pub(crate) rect: LayoutRect,
    pub(crate) radii: ResolvedRadii,
    pub(crate) inverse_transform: Transform,
    pub(crate) horizontal: bool,
    pub(crate) vertical: bool,
    pub(crate) path: Option<Arc<[PathSegment]>>,
    pub(crate) fill_rule: FillRule,
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
            .map(|node| node.clip.clone())
    }
}

#[derive(Clone, Debug)]
struct PresentationContext {
    origin: [f32; 2],
    transform: Transform,
    clip: LogicalClip,
    shape_clips: ShapeClipStack,
}

impl Default for PresentationContext {
    fn default() -> Self {
        Self {
            origin: [0.0; 2],
            transform: Transform::IDENTITY,
            clip: LogicalClip::default(),
            shape_clips: ShapeClipStack::default(),
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
    BeginOpacityGroup {
        node: NodeId,
        opacity: f32,
    },
    EndOpacityGroup {
        node: NodeId,
    },
    BackdropBlur {
        rect: LayoutRect,
        radius: f32,
        clip: LogicalClip,
    },
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
    Raster {
        node: NodeId,
        rect: LayoutRect,
        rasterizer: &'a dyn crate::DesktopNativeElement,
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
    pending_events: Arc<Mutex<Vec<DesktopProviderEvent>>>,
    event_wake: RuntimeWakeHandle,
    raster_resources: HashSet<ResourceId>,
}

impl DesktopScene {
    #[cfg(test)]
    pub(crate) fn new(surface: SurfaceId, elements: DesktopElementRegistry) -> Self {
        Self::new_with_wake(surface, elements, RuntimeWakeHandle::new(|| {}))
    }

    pub(crate) fn new_with_wake(
        surface: SurfaceId,
        elements: DesktopElementRegistry,
        event_wake: RuntimeWakeHandle,
    ) -> Self {
        Self {
            validation: SceneProjection::new(surface),
            elements,
            nodes: HashMap::new(),
            pending_events: Arc::new(Mutex::new(Vec::new())),
            event_wake,
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
        let pending = std::mem::take(&mut *self.pending_events.lock().unwrap());
        pending
            .into_iter()
            .filter_map(|event| {
                let state = self.nodes.get(&event.target)?;
                let resolved = self.elements.event(
                    state.element_type,
                    event.target,
                    &event.name,
                    &event.detail,
                );
                debug_assert!(resolved.is_ok(), "native element emitted invalid event");
                let (name, mask) = resolved.ok()?;
                (state.event_mask & mask != 0).then_some(DesktopProviderEvent {
                    target: event.target,
                    name,
                    detail: event.detail,
                })
            })
            .collect()
    }

    pub(crate) fn cursor_at(&self, point: [f32; 2]) -> Option<whisker_protocol::CursorKeyword> {
        let node = self.hit_test(point)?;
        Some(
            self.nodes
                .get(&node)
                .expect("hit-tested Desktop node remains live")
                .presentation
                .cursor
                .fallback,
        )
    }

    pub(crate) fn hit_test(&self, point: [f32; 2]) -> Option<NodeId> {
        let mut roots = self
            .nodes
            .iter()
            .filter_map(|(node, state)| {
                state
                    .presentation
                    .parent
                    .is_none()
                    .then_some((*node, state.presentation.z_order))
            })
            .collect::<Vec<_>>();
        roots.sort_by_key(|(node, z_order)| (*z_order, node.get()));
        roots
            .into_iter()
            .rev()
            .find_map(|(node, _)| self.hit_test_node(node, point, [0.0, 0.0]))
    }

    fn hit_test_node(
        &self,
        node: NodeId,
        point: [f32; 2],
        parent_origin: [f32; 2],
    ) -> Option<NodeId> {
        let state = self.nodes.get(&node)?;
        let presentation = &state.presentation;
        if presentation.hit_test == HitTestBehavior::None {
            return None;
        }
        let visible = presentation.visibility == Visibility::Visible;
        let rect = presentation.layout.border_box;
        let origin = [parent_origin[0] + rect.x, parent_origin[1] + rect.y];
        let contains_x = point[0] >= origin[0] && point[0] <= origin[0] + rect.width;
        let contains_y = point[1] >= origin[1] && point[1] <= origin[1] + rect.height;
        let clipped = (presentation.clip.horizontal == OverflowClip::Hidden && !contains_x)
            || (presentation.clip.vertical == OverflowClip::Hidden && !contains_y);
        if presentation.hit_test != HitTestBehavior::BoxOnly && !clipped {
            let child_origin = if state.content.is_scroll_container() {
                [
                    origin[0] - state.scroll_offset[0],
                    origin[1] - state.scroll_offset[1],
                ]
            } else {
                origin
            };
            let mut children = presentation
                .children
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    (
                        *child,
                        self.nodes
                            .get(child)
                            .map_or(0, |child| child.presentation.z_order),
                        index,
                    )
                })
                .collect::<Vec<_>>();
            children.sort_by_key(|(_, z_order, index)| (*z_order, *index));
            for (child, _, _) in children.into_iter().rev() {
                if let Some(target) = self.hit_test_node(child, point, child_origin) {
                    return Some(target);
                }
            }
        }
        (visible
            && contains_x
            && contains_y
            && presentation.hit_test != HitTestBehavior::DescendantsOnly)
            .then_some(node)
    }

    pub(crate) fn scroll_at(&mut self, point: [f32; 2], delta: [f32; 2]) -> bool {
        let Some(hit) = self.hit_test(point) else {
            return false;
        };
        let mut current = Some(hit);
        while let Some(node) = current {
            let is_scroll = self
                .nodes
                .get(&node)
                .is_some_and(|state| state.content.is_scroll_container());
            if is_scroll {
                let max = self.max_scroll_offset(node);
                let state = self.nodes.get_mut(&node).expect("scroll hit remains live");
                let next = [
                    (state.scroll_offset[0] + delta[0]).clamp(0.0, max[0]),
                    (state.scroll_offset[1] + delta[1]).clamp(0.0, max[1]),
                ];
                let changed = next != state.scroll_offset;
                state.scroll_offset = next;
                if changed {
                    let viewport = state.presentation.layout.content_box;
                    self.pending_events
                        .lock()
                        .unwrap()
                        .push(DesktopProviderEvent {
                            target: node,
                            name: "scroll".to_owned(),
                            detail: WhiskerValue::map([
                                ("scrollLeft", WhiskerValue::Float(f64::from(next[0]))),
                                ("scrollTop", WhiskerValue::Float(f64::from(next[1]))),
                                (
                                    "scrollWidth",
                                    WhiskerValue::Float(f64::from(viewport.width + max[0])),
                                ),
                                (
                                    "scrollHeight",
                                    WhiskerValue::Float(f64::from(viewport.height + max[1])),
                                ),
                                (
                                    "viewportWidth",
                                    WhiskerValue::Float(f64::from(viewport.width)),
                                ),
                                (
                                    "viewportHeight",
                                    WhiskerValue::Float(f64::from(viewport.height)),
                                ),
                            ]),
                        });
                    self.event_wake.wake();
                }
                return changed;
            }
            current = self
                .nodes
                .get(&node)
                .and_then(|state| state.presentation.parent);
        }
        false
    }

    fn max_scroll_offset(&self, node: NodeId) -> [f32; 2] {
        let state = self.nodes.get(&node).expect("scroll node remains live");
        let viewport = state.presentation.layout.content_box;
        let (content_width, content_height) = state
            .presentation
            .children
            .iter()
            .filter_map(|child| self.nodes.get(child))
            .fold((viewport.width, viewport.height), |extent, child| {
                let rect = child.presentation.layout.border_box;
                (
                    extent.0.max(rect.x + rect.width),
                    extent.1.max(rect.y + rect.height),
                )
            });
        [
            (content_width - viewport.width).max(0.0),
            (content_height - viewport.height).max(0.0),
        ]
    }

    fn clamp_scroll_offsets(&mut self) {
        let scroll_nodes = self
            .nodes
            .iter()
            .filter_map(|(node, state)| state.content.is_scroll_container().then_some(*node))
            .collect::<Vec<_>>();
        for node in scroll_nodes {
            let max = self.max_scroll_offset(node);
            let state = self.nodes.get_mut(&node).expect("scroll node remains live");
            state.scroll_offset[0] = state.scroll_offset[0].clamp(0.0, max[0]);
            state.scroll_offset[1] = state.scroll_offset[1].clamp(0.0, max[1]);
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
        let visible = presentation.visibility == Visibility::Visible;
        let opacity_group = presentation.opacity < 1.0;
        if opacity_group {
            commands.push(PaintCommand::BeginOpacityGroup {
                node: id,
                opacity: presentation.opacity,
            });
        }
        let border = LayoutRect {
            x: context.origin[0] + presentation.layout.border_box.x,
            y: context.origin[1] + presentation.layout.border_box.y,
            width: presentation.layout.border_box.width,
            height: presentation.layout.border_box.height,
        };
        let opacity = 1.0;
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
            let (clip_rect, clip_radii, path, fill_rule) = clip_shape_geometry(reference, shape);
            node_shape_clips = node_shape_clips.push(ShapeClip {
                rect: clip_rect,
                radii: clip_radii,
                inverse_transform: inverse_transform(transform).unwrap_or(Transform::IDENTITY),
                horizontal: true,
                vertical: true,
                path,
                fill_rule,
            });
            node_clip_bounds = transform_rect_aabb(clip_rect, transform);
        }
        if visible
            && let Some(radius) = presentation
                .visual_effects
                .backdrop_blur
                .filter(|value| *value > 0.0)
            && let Some(rect) = transform_rect_aabb(border, transform)
        {
            commands.push(PaintCommand::BackdropBlur {
                rect,
                radius,
                clip: context.clip,
            });
        }
        if visible
            && (presentation.paint.is_some()
                || !presentation.background_layers.is_empty()
                || !presentation.visual_effects.box_shadows.is_empty())
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
                path: None,
                fill_rule: FillRule::NonZero,
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
        if visible && let Some(content) = node.content.text() {
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
        if visible && let Some(rasterizer) = node.content.rasterizer() {
            commands.push(PaintCommand::Raster {
                node: id,
                rect: content,
                rasterizer,
                clip: descendant_clip.intersect(content, true, true),
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
            let child_origin = if node.content.is_scroll_container() {
                [
                    border.x - node.scroll_offset[0],
                    border.y - node.scroll_offset[1],
                ]
            } else {
                [border.x, border.y]
            };
            self.collect_commands(
                child,
                PresentationContext {
                    origin: child_origin,
                    transform,
                    clip: descendant_clip,
                    shape_clips: descendant_shape_clips.clone(),
                },
                commands,
            );
        }
        if opacity_group {
            commands.push(PaintCommand::EndOpacityGroup { node: id });
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
                    self.elements
                        .create(*element_type, DesktopEventEmitter::default())?;
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
                    if content.paint.decoration.lines.overline
                        || (content.paint.decoration.lines.underline
                            && content.paint.decoration.lines.line_through)
                        || !matches!(
                            content.paint.decoration.thickness,
                            whisker_protocol::TextDecorationThickness::Auto
                        )
                    {
                        return Err(DesktopPresentError::Unsupported("text-decoration"));
                    }
                    if let Some(element_type) = types.get(node).copied() {
                        self.elements
                            .create(element_type, DesktopEventEmitter::default())?
                            .set_text(*node, content.clone())?;
                    }
                }
                Operation::SetTextStyle { node, style } => {
                    if let Some(element_type) = types.get(node).copied() {
                        if !self.elements.receives_text_style(element_type)? {
                            return Err(DesktopPresentError::Unsupported("text-style"));
                        }
                        self.elements
                            .create(element_type, DesktopEventEmitter::default())?
                            .set_text_style(*node, style)?;
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
                Operation::SetCursor { cursor, .. } if !cursor.resources.is_empty() => {
                    return Err(DesktopPresentError::Unsupported("resource-backed cursor"));
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
                | Operation::SetCursor { .. }
                | Operation::SetPointerCapture { .. }
                | Operation::ReleasePointerCapture { .. } => {}
            }
        }
        Ok(())
    }

    fn apply_operations(&mut self, packet: &FramePacket) {
        if packet.header.mode == FrameMode::Snapshot {
            self.nodes.clear();
            self.pending_events.lock().unwrap().clear();
        }
        for operation in &packet.operations {
            match operation {
                Operation::CreateNode { node, element_type } => {
                    let pending_events = Arc::clone(&self.pending_events);
                    let event_wake = self.event_wake.clone();
                    let target = *node;
                    let events = DesktopEventEmitter::new(move |event| {
                        pending_events.lock().unwrap().push(DesktopProviderEvent {
                            target,
                            name: event.event,
                            detail: event.detail,
                        });
                        event_wake.wake();
                    });
                    let content = self
                        .elements
                        .create(*element_type, events)
                        .expect("element operations were validated before commit");
                    self.nodes.insert(
                        *node,
                        RenderNode {
                            element_type: *element_type,
                            presentation: CommonPresentation::default(),
                            content,
                            event_mask: 0,
                            scroll_offset: [0.0; 2],
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
                Operation::SetTextStyle { node, style } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .content
                        .set_text_style(*node, style)
                        .expect("text-style operation was validated before commit");
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
                    state
                        .content
                        .invoke_command(*node, *command, arguments)
                        .expect("element command was validated before commit");
                }
                Operation::SetHitTest { node, behavior } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .hit_test = *behavior;
                }
                Operation::SetCursor { node, cursor } => {
                    self.nodes
                        .get_mut(node)
                        .expect("validated node")
                        .presentation
                        .cursor = cursor.clone();
                }
                Operation::SetPointerCapture { .. } | Operation::ReleasePointerCapture { .. } => {}
                Operation::SetImage { .. } => {
                    unreachable!("unsupported operations are rejected before commit")
                }
            }
        }
        self.clamp_scroll_offsets();
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
                    capability: whisker_protocol::RenderCapability::TextEffects,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::TextTypography,
                    support: whisker_protocol::CapabilitySupport::Native,
                },
                whisker_protocol::CapabilityEntry {
                    capability: whisker_protocol::RenderCapability::Cursor,
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
    remainder.backdrop_blur = None;
    remainder.image_rendering = whisker_protocol::ImageRendering::Auto;
    remainder == VisualEffects::default()
        && matches!(
            effects.image_rendering,
            whisker_protocol::ImageRendering::Auto
                | whisker_protocol::ImageRendering::Pixelated
                | whisker_protocol::ImageRendering::CrispEdges
        )
        && effects.clip_path.as_ref().is_none_or(|(reference, shape)| {
            matches!(
                reference,
                PaintBox::Border | PaintBox::Padding | PaintBox::Content
            ) && matches!(
                shape,
                ClipShape::Inset { .. }
                    | ClipShape::Circle { .. }
                    | ClipShape::Ellipse { .. }
                    | ClipShape::Path { .. }
            )
        })
}

fn clip_shape_geometry(
    reference: LayoutRect,
    shape: &ClipShape,
) -> (
    LayoutRect,
    ResolvedRadii,
    Option<Arc<[PathSegment]>>,
    FillRule,
) {
    let zero_radii = || ResolvedRadii {
        horizontal: [0.0; 4],
        vertical: [0.0; 4],
    };
    match shape {
        ClipShape::Inset { edges, radii } => {
            let rect = inset_clip_rect(reference, edges);
            let radii = resolve_radii(radii, rect);
            (rect, radii, None, FillRule::NonZero)
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
                None,
                FillRule::NonZero,
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
                None,
                FillRule::NonZero,
            )
        }
        ClipShape::Path {
            fill_rule,
            commands,
        } => {
            let segments = flatten_path(reference, commands);
            let bounds = path_bounds(&segments).unwrap_or(reference);
            (bounds, zero_radii(), Some(segments.into()), *fill_rule)
        }
        _ => unreachable!("unsupported clip-path shape passed validation"),
    }
}

fn resolve_path_position(reference: LayoutRect, position: PaintPosition) -> [f32; 2] {
    [
        reference.x + resolve_coordinate(position.x, reference.width),
        reference.y + resolve_coordinate(position.y, reference.height),
    ]
}

fn add_path_segment(segments: &mut Vec<PathSegment>, from: [f32; 2], to: [f32; 2]) {
    if from != to {
        segments.push(PathSegment { from, to });
    }
}

fn close_path_subpath(
    segments: &mut Vec<PathSegment>,
    current: &mut Option<[f32; 2]>,
    start: Option<[f32; 2]>,
) {
    if let (Some(from), Some(to)) = (*current, start) {
        add_path_segment(segments, from, to);
        *current = Some(to);
    }
}

fn flatten_path(reference: LayoutRect, commands: &[PathCommand]) -> Vec<PathSegment> {
    const CURVE_STEPS: usize = 16;
    let mut segments = Vec::new();
    let mut current = None;
    let mut start = None;
    for command in commands {
        match command {
            PathCommand::MoveTo(point) => {
                close_path_subpath(&mut segments, &mut current, start);
                let point = resolve_path_position(reference, *point);
                current = Some(point);
                start = Some(point);
            }
            PathCommand::LineTo(point) => {
                let to = resolve_path_position(reference, *point);
                if let Some(from) = current {
                    add_path_segment(&mut segments, from, to);
                }
                current = Some(to);
            }
            PathCommand::QuadraticTo { control, end } => {
                let Some(from) = current else { continue };
                let control = resolve_path_position(reference, *control);
                let end = resolve_path_position(reference, *end);
                let mut previous = from;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let inverse = 1.0 - t;
                    let to = [
                        inverse * inverse * from[0]
                            + 2.0 * inverse * t * control[0]
                            + t * t * end[0],
                        inverse * inverse * from[1]
                            + 2.0 * inverse * t * control[1]
                            + t * t * end[1],
                    ];
                    add_path_segment(&mut segments, previous, to);
                    previous = to;
                }
                current = Some(end);
            }
            PathCommand::CubicTo {
                control_1,
                control_2,
                end,
            } => {
                let Some(from) = current else { continue };
                let control_1 = resolve_path_position(reference, *control_1);
                let control_2 = resolve_path_position(reference, *control_2);
                let end = resolve_path_position(reference, *end);
                let mut previous = from;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let inverse = 1.0 - t;
                    let to = [
                        inverse.powi(3) * from[0]
                            + 3.0 * inverse * inverse * t * control_1[0]
                            + 3.0 * inverse * t * t * control_2[0]
                            + t.powi(3) * end[0],
                        inverse.powi(3) * from[1]
                            + 3.0 * inverse * inverse * t * control_1[1]
                            + 3.0 * inverse * t * t * control_2[1]
                            + t.powi(3) * end[1],
                    ];
                    add_path_segment(&mut segments, previous, to);
                    previous = to;
                }
                current = Some(end);
            }
            PathCommand::Close => close_path_subpath(&mut segments, &mut current, start),
        }
    }
    close_path_subpath(&mut segments, &mut current, start);
    segments
}

fn path_bounds(segments: &[PathSegment]) -> Option<LayoutRect> {
    let first = segments.first()?;
    let mut left = first.from[0].min(first.to[0]);
    let mut top = first.from[1].min(first.to[1]);
    let mut right = first.from[0].max(first.to[0]);
    let mut bottom = first.from[1].max(first.to[1]);
    for segment in &segments[1..] {
        left = left.min(segment.from[0]).min(segment.to[0]);
        top = top.min(segment.from[1]).min(segment.to[1]);
        right = right.max(segment.from[0]).max(segment.to[0]);
        bottom = bottom.max(segment.from[1]).max(segment.to[1]);
    }
    Some(LayoutRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
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
    use std::sync::atomic::{AtomicUsize, Ordering};

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
                alignment: whisker_protocol::MeasureTextAlignment::Start,
                indent: Default::default(),
                wrap: MeasureTextWrap::Wrap,
                word_break: Default::default(),
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

    #[derive(Debug)]
    struct ToggleNative {
        checked: bool,
        disabled: bool,
        events: DesktopEventEmitter,
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

        fn invoke_command(&mut self, command: CommandId, _arguments: &WhiskerValue) {
            assert_eq!(command, TOGGLE);
            if self.disabled {
                return;
            }
            self.checked = !self.checked;
            self.events.emit(DesktopNativeEvent {
                event: "change".into(),
                detail: WhiskerValue::map([("checked", WhiskerValue::Bool(self.checked))]),
            });
        }
    }

    fn toggle_scene_with_wake(event_wake: RuntimeWakeHandle) -> (DesktopScene, ElementTypeId) {
        let element_type = ElementTypeId::new(20).unwrap();
        let mut registrations = standard_element_registrations();
        registrations.push(ElementRegistration {
            element_type,
            name: "whisker.test/Toggle".into(),
            child_policy: whisker_protocol::ChildPolicy::None,
            measurement: ElementMeasurement::None,
            text_style: false,
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
        factories.push(DesktopElementFactory::native(
            "whisker.test/Toggle",
            |events| {
                Box::new(ToggleNative {
                    checked: false,
                    disabled: false,
                    events,
                })
            },
        ));
        (
            DesktopScene::new_with_wake(
                SurfaceId::new(1).unwrap(),
                DesktopElementRegistry::bind(&registrations, &factories).unwrap(),
                event_wake,
            ),
            element_type,
        )
    }

    fn toggle_scene() -> (DesktopScene, ElementTypeId) {
        toggle_scene_with_wake(RuntimeWakeHandle::new(|| {}))
    }

    #[test]
    fn native_toggle_applies_properties_invokes_command_and_routes_change() {
        let node = id(1);
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        let (mut scene, element_type) = toggle_scene_with_wake(RuntimeWakeHandle::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }));
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
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
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
    fn scroll_container_offsets_paint_and_hit_testing_inside_its_viewport() {
        let scroll = id(1);
        let content = id(2);
        let target = id(3);
        let mut scene = scene(SurfaceId::new(1).unwrap());
        scene
            .present(&packet(
                FrameMode::Snapshot,
                0,
                1,
                vec![
                    Operation::CreateNode {
                        node: scroll,
                        element_type: element_type(whisker::SCROLL_VIEW_ELEMENT_NAME),
                    },
                    Operation::SetEventMask {
                        node: scroll,
                        event_mask: 1,
                    },
                    Operation::CreateNode {
                        node: content,
                        element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                    },
                    Operation::CreateNode {
                        node: target,
                        element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                    },
                    Operation::InsertChild {
                        parent: scroll,
                        child: content,
                        index: 0,
                    },
                    Operation::InsertChild {
                        parent: content,
                        child: target,
                        index: 0,
                    },
                    Operation::SetLayout {
                        node: scroll,
                        geometry: geometry(0.0, 0.0, 100.0, 100.0),
                    },
                    Operation::SetLayout {
                        node: content,
                        geometry: geometry(0.0, 0.0, 100.0, 300.0),
                    },
                    Operation::SetLayout {
                        node: target,
                        geometry: geometry(0.0, 150.0, 100.0, 40.0),
                    },
                    Operation::SetClip {
                        node: scroll,
                        clip: BoxClip {
                            horizontal: OverflowClip::Hidden,
                            vertical: OverflowClip::Hidden,
                        },
                    },
                    Operation::SetBoxPaint {
                        node: target,
                        paint: paint(PaintColor::Srgba {
                            red: 255,
                            green: 0,
                            blue: 0,
                            alpha: 1.0,
                        }),
                    },
                ],
            ))
            .unwrap();

        assert_eq!(scene.hit_test([10.0, 60.0]), Some(content));
        assert!(scene.scroll_at([10.0, 60.0], [0.0, 120.0]));
        assert_eq!(scene.nodes[&scroll].scroll_offset, [0.0, 120.0]);
        assert_eq!(
            scene.take_events(),
            vec![DesktopProviderEvent {
                target: scroll,
                name: "scroll".to_owned(),
                detail: WhiskerValue::map([
                    ("scrollLeft", WhiskerValue::Float(0.0)),
                    ("scrollTop", WhiskerValue::Float(120.0)),
                    ("scrollWidth", WhiskerValue::Float(100.0)),
                    ("scrollHeight", WhiskerValue::Float(300.0)),
                    ("viewportWidth", WhiskerValue::Float(98.0)),
                    ("viewportHeight", WhiskerValue::Float(96.0)),
                ]),
            }]
        );
        assert_eq!(scene.hit_test([10.0, 60.0]), Some(target));
        assert!(scene.paint_commands().iter().any(|command| {
            matches!(
                command,
                PaintCommand::Box { rect, .. }
                    if rect.x == 0.0 && rect.y == 30.0 && rect.width == 100.0
            )
        }));
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
        assert_eq!(commands.len(), 6);
        assert!(matches!(
            &commands[0],
            PaintCommand::BeginOpacityGroup { node, opacity }
                if *node == root && *opacity == 0.5
        ));
        assert!(matches!(
            &commands[1],
            PaintCommand::Box { rect, opacity, .. }
                if *rect == LayoutRect { x: 4.0, y: 5.0, width: 100.0, height: 80.0 }
                    && *opacity == 1.0
        ));
        assert!(matches!(
            &commands[2],
            PaintCommand::BeginOpacityGroup { node, opacity }
                if *node == child && *opacity == 0.5
        ));
        assert!(matches!(
            &commands[3],
            PaintCommand::Text { rect, clip, opacity, .. }
                if *rect == LayoutRect { x: 7.0, y: 10.0, width: 18.0, height: 6.0 }
                    && clip.left == Some(7.0)
                    && clip.right == Some(25.0)
                    && *opacity == 1.0
        ));
        assert!(matches!(
            &commands[4],
            PaintCommand::EndOpacityGroup { node } if *node == child
        ));
        assert!(matches!(
            &commands[5],
            PaintCommand::EndOpacityGroup { node } if *node == root
        ));
    }

    #[test]
    fn cursor_hit_testing_respects_pointer_events_and_child_z_order() {
        let root = id(1);
        let ignored_child = id(2);
        let active_child = id(3);
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
                    Operation::CreateNode {
                        node: ignored_child,
                        element_type,
                    },
                    Operation::CreateNode {
                        node: active_child,
                        element_type,
                    },
                    Operation::InsertChild {
                        parent: root,
                        child: ignored_child,
                        index: 0,
                    },
                    Operation::InsertChild {
                        parent: root,
                        child: active_child,
                        index: 1,
                    },
                    Operation::SetLayout {
                        node: root,
                        geometry: geometry(0.0, 0.0, 180.0, 90.0),
                    },
                    Operation::SetLayout {
                        node: ignored_child,
                        geometry: geometry(10.0, 10.0, 70.0, 70.0),
                    },
                    Operation::SetLayout {
                        node: active_child,
                        geometry: geometry(100.0, 10.0, 70.0, 70.0),
                    },
                    Operation::SetCursor {
                        node: root,
                        cursor: Cursor {
                            resources: Vec::new(),
                            fallback: whisker_protocol::CursorKeyword::Pointer,
                        },
                    },
                    Operation::SetCursor {
                        node: ignored_child,
                        cursor: Cursor {
                            resources: Vec::new(),
                            fallback: whisker_protocol::CursorKeyword::Text,
                        },
                    },
                    Operation::SetHitTest {
                        node: ignored_child,
                        behavior: HitTestBehavior::None,
                    },
                    Operation::SetCursor {
                        node: active_child,
                        cursor: Cursor {
                            resources: Vec::new(),
                            fallback: whisker_protocol::CursorKeyword::Grab,
                        },
                    },
                ],
            ))
            .unwrap();

        assert_eq!(
            scene.cursor_at([20.0, 20.0]),
            Some(whisker_protocol::CursorKeyword::Pointer)
        );
        assert_eq!(
            scene.cursor_at([120.0, 20.0]),
            Some(whisker_protocol::CursorKeyword::Grab)
        );
        assert_eq!(scene.cursor_at([200.0, 20.0]), None);
    }

    #[test]
    fn visible_descendant_paints_and_hit_tests_through_hidden_parent() {
        let root = id(1);
        let child = id(2);
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
                        geometry: geometry(10.0, 10.0, 80.0, 80.0),
                    },
                    Operation::SetLayout {
                        node: child,
                        geometry: geometry(10.0, 10.0, 30.0, 30.0),
                    },
                    Operation::SetBoxPaint {
                        node: root,
                        paint: paint(PaintColor::Named("red".into())),
                    },
                    Operation::SetBoxPaint {
                        node: child,
                        paint: paint(PaintColor::Named("green".into())),
                    },
                    Operation::SetVisibility {
                        node: root,
                        visibility: Visibility::Hidden,
                    },
                    Operation::SetVisibility {
                        node: child,
                        visibility: Visibility::Visible,
                    },
                    Operation::SetCursor {
                        node: root,
                        cursor: Cursor {
                            resources: Vec::new(),
                            fallback: whisker_protocol::CursorKeyword::Pointer,
                        },
                    },
                    Operation::SetCursor {
                        node: child,
                        cursor: Cursor {
                            resources: Vec::new(),
                            fallback: whisker_protocol::CursorKeyword::Text,
                        },
                    },
                ],
            ))
            .unwrap();

        let commands = scene.paint_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(&commands[0], PaintCommand::Box { rect, .. }
            if *rect == LayoutRect { x: 20.0, y: 20.0, width: 30.0, height: 30.0 }));
        assert_eq!(
            scene.cursor_at([25.0, 25.0]),
            Some(whisker_protocol::CursorKeyword::Text)
        );
        assert_eq!(scene.cursor_at([70.0, 70.0]), None);
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
