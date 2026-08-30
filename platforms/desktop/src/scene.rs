use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

use whisker::runtime::RuntimeWakeHandle;
use whisker_engine::FrameSink;
use whisker_protocol::{
    Accessibility, ApplyResult, BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode,
    BoxClip, BoxPaint, ClipShape, Cursor, ElementTypeId, FillRule, FrameMode, FramePacket,
    HitTestBehavior, ImageRepeat, LayoutGeometry, LayoutRect, NodeId, Operation, OverflowClip,
    PaintBox, PaintColor, PaintCoordinate, PaintImage, PaintPosition, PathCommand,
    RadialGradientExtent, ResourceId, SceneProjection, SurfaceId, TextContent, Transform,
    ValidationError, Visibility, VisualEffects, WhiskerValue,
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
    accessibility: Accessibility,
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
            accessibility: Accessibility::default(),
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
    scroll_sequence_start: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug)]
struct SmoothScroll {
    start: [f32; 2],
    target: [f32; 2],
    elapsed_ms: f32,
    duration_ms: f32,
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
    smooth_scrolls: HashMap<NodeId, SmoothScroll>,
    presentation_pool: HashMap<ElementTypeId, Vec<DesktopElementContent>>,
    pending_events: Arc<Mutex<Vec<DesktopProviderEvent>>>,
    event_wake: RuntimeWakeHandle,
    raster_resources: HashSet<ResourceId>,
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

#[cfg(test)]
mod tests;

mod transaction;
pub(crate) use transaction::DesktopPresentError;

mod paint_support;

pub(crate) use paint_support::is_transparent;
use paint_support::*;
