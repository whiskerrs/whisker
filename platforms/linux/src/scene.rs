use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use gtk::pango;
use gtk::prelude::*;
use gtk4 as gtk;
use whisker::{SCROLL_BY_COMMAND, SCROLL_TO_COMMAND, SCROLL_VIEW_ELEMENT_NAME, TEXT_ELEMENT_NAME};
use whisker_engine::FrameSink;
use whisker_protocol::{
    ApplyResult, CursorKeyword, ElementRegistration, ElementTypeId, FrameMode, FramePacket,
    HostPresentationUpdate, InputPoint, MeasureTextAlignment, MeasureTextWrap, NodeId, Operation,
    RenderCapabilities, SceneProjection, SurfaceId, TextContent, WhiskerValue,
};

use crate::LinuxError;
use crate::node::{NodeWidget, PaintedText, ScrollState};
use crate::text::{self, PreparedLayouts};

/// A native element event waiting for the next runtime turn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProviderEvent {
    pub(crate) target: NodeId,
    pub(crate) name: String,
    pub(crate) detail: WhiskerValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ElementKind {
    View,
    Text,
    ScrollView,
}

struct ElementBinding {
    registration: ElementRegistration,
    kind: ElementKind,
}

/// Shared Host state that input handlers mutate between frames.
#[derive(Default)]
pub(crate) struct ScrollFeedback {
    offsets: HashMap<NodeId, InputPoint>,
    events: VecDeque<ProviderEvent>,
}

pub(crate) type SharedScrollFeedback = Rc<RefCell<ScrollFeedback>>;

impl ScrollFeedback {
    pub(crate) fn take_presentation_updates(&mut self) -> Vec<HostPresentationUpdate> {
        self.offsets
            .drain()
            .map(|(node, offset)| HostPresentationUpdate::ScrollOffset { node, offset })
            .collect()
    }

    pub(crate) fn take_events(&mut self) -> Vec<ProviderEvent> {
        self.events.drain(..).collect()
    }
}

/// Projects accepted frame packets onto a retained GTK widget tree.
pub(crate) struct GtkFrameSink {
    capabilities: RenderCapabilities,
    projection: SceneProjection,
    surface: NodeWidget,
    elements: HashMap<ElementTypeId, ElementBinding>,
    nodes: HashMap<NodeId, NodeWidget>,
    node_types: HashMap<NodeId, ElementTypeId>,
    node_order: Vec<NodeId>,
    parents: HashMap<NodeId, NodeId>,
    texts: HashMap<NodeId, TextContent>,
    event_masks: HashMap<NodeId, u64>,
    prepared: PreparedLayouts,
    context: pango::Context,
    feedback: SharedScrollFeedback,
}

impl GtkFrameSink {
    pub(crate) fn new(
        surface: NodeWidget,
        surface_id: SurfaceId,
        registrations: &[ElementRegistration],
        capabilities: RenderCapabilities,
        prepared: PreparedLayouts,
        context: pango::Context,
    ) -> Self {
        let elements = registrations
            .iter()
            .map(|registration| {
                let kind = match registration.name.as_str() {
                    TEXT_ELEMENT_NAME => ElementKind::Text,
                    SCROLL_VIEW_ELEMENT_NAME => ElementKind::ScrollView,
                    _ => ElementKind::View,
                };
                (
                    registration.element_type,
                    ElementBinding {
                        registration: registration.clone(),
                        kind,
                    },
                )
            })
            .collect();
        Self {
            capabilities,
            projection: SceneProjection::new(surface_id),
            surface,
            elements,
            nodes: HashMap::new(),
            node_types: HashMap::new(),
            node_order: Vec::new(),
            parents: HashMap::new(),
            texts: HashMap::new(),
            event_masks: HashMap::new(),
            prepared,
            context,
            feedback: SharedScrollFeedback::default(),
        }
    }

    pub(crate) fn feedback(&self) -> SharedScrollFeedback {
        Rc::clone(&self.feedback)
    }

    /// Finds the node whose widget is `widget` or its nearest node ancestor.
    pub(crate) fn node_for_widget(&self, widget: &gtk::Widget) -> Option<NodeId> {
        let mut current = Some(widget.clone());
        while let Some(widget) = current {
            if let Some(id) = widget
                .downcast_ref::<NodeWidget>()
                .and_then(|node| node.state().id)
            {
                return Some(id);
            }
            current = widget.parent();
        }
        None
    }

    /// The nearest explicit cursor on `widget` or its node ancestors.
    pub(crate) fn cursor_for_widget(&self, widget: &gtk::Widget) -> Option<&'static str> {
        let mut node = self.node_for_widget(widget);
        while let Some(id) = node {
            let cursor = self.nodes.get(&id)?.state().cursor;
            if cursor != CursorKeyword::Auto {
                return Some(crate::cursor::name(cursor));
            }
            node = self.parent(id);
        }
        None
    }

    pub(crate) fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.parents.get(&node).copied()
    }

    pub(crate) fn widget(&self, node: NodeId) -> Option<&NodeWidget> {
        self.nodes.get(&node)
    }

    /// Moves a ScrollView and records the offset and `scroll` event for the runtime.
    pub(crate) fn scroll_to(&self, node: NodeId, offset: [f32; 2]) -> bool {
        let Some(widget) = self.nodes.get(&node) else {
            return false;
        };
        let max = max_scroll_offset(widget);
        let clamped = [offset[0].clamp(0.0, max[0]), offset[1].clamp(0.0, max[1])];
        {
            let mut state = widget.state_mut();
            let Some(scroll) = state.scroll.as_mut() else {
                return false;
            };
            if scroll.offset == clamped {
                return false;
            }
            scroll.offset = clamped;
        }
        widget.queue_allocate();
        let mut feedback = self.feedback.borrow_mut();
        feedback.offsets.insert(
            node,
            InputPoint {
                x: clamped[0],
                y: clamped[1],
            },
        );
        if self.subscribes(node, "scroll") {
            let state = widget.state();
            let viewport = state.geometry.border_box;
            let extent = content_extent(widget);
            feedback.events.push_back(ProviderEvent {
                target: node,
                name: "scroll".into(),
                detail: WhiskerValue::map([
                    ("scrollLeft", WhiskerValue::Int(clamped[0].round() as i64)),
                    ("scrollTop", WhiskerValue::Int(clamped[1].round() as i64)),
                    ("scrollWidth", WhiskerValue::Int(extent[0].round() as i64)),
                    ("scrollHeight", WhiskerValue::Int(extent[1].round() as i64)),
                    (
                        "viewportWidth",
                        WhiskerValue::Int(viewport.width.round() as i64),
                    ),
                    (
                        "viewportHeight",
                        WhiskerValue::Int(viewport.height.round() as i64),
                    ),
                ]),
            });
        }
        true
    }

    fn subscribes(&self, node: NodeId, event: &str) -> bool {
        let mask = self.event_masks.get(&node).copied().unwrap_or(0);
        self.node_types
            .get(&node)
            .and_then(|element_type| self.elements.get(element_type))
            .and_then(|binding| binding.registration.event_named(event))
            .and_then(|schema| schema.mask())
            .is_some_and(|bit| mask & bit != 0)
    }

    fn node(&self, node: NodeId) -> Result<&NodeWidget, LinuxError> {
        self.nodes
            .get(&node)
            .ok_or_else(|| LinuxError(format!("GTK Host is missing node {}", node.get())))
    }

    fn binding(&self, node: NodeId) -> Result<&ElementBinding, LinuxError> {
        self.node_types
            .get(&node)
            .and_then(|element_type| self.elements.get(element_type))
            .ok_or_else(|| LinuxError(format!("GTK Host has no element for node {}", node.get())))
    }

    fn reset(&mut self) {
        for widget in self.nodes.values() {
            widget.unparent();
        }
        self.surface.state_mut().children.clear();
        self.nodes.clear();
        self.node_types.clear();
        self.node_order.clear();
        self.parents.clear();
        self.texts.clear();
        self.event_masks.clear();
        *self.feedback.borrow_mut() = ScrollFeedback::default();
    }

    fn detach(&mut self, child: NodeId) {
        let Some(widget) = self.nodes.get(&child) else {
            return;
        };
        if let Some(parent) = widget.parent().and_downcast::<NodeWidget>() {
            parent
                .state_mut()
                .children
                .retain(|candidate| candidate != widget);
            parent.queue_allocate();
            parent.queue_draw();
        }
        widget.unparent();
        self.parents.remove(&child);
    }

    fn delete(&mut self, node: NodeId) {
        let children = self
            .nodes
            .get(&node)
            .map(|widget| widget.state().children.clone())
            .unwrap_or_default();
        for id in children.iter().filter_map(|child| child.state().id) {
            self.delete(id);
        }
        self.detach(node);
        self.nodes.remove(&node);
        self.node_types.remove(&node);
        self.node_order.retain(|candidate| *candidate != node);
        self.texts.remove(&node);
        self.event_masks.remove(&node);
        self.feedback.borrow_mut().offsets.remove(&node);
    }

    fn attach_roots(&mut self) {
        for id in &self.node_order {
            if self.parents.contains_key(id) {
                continue;
            }
            let widget = &self.nodes[id];
            if widget.parent().as_ref() == Some(self.surface.upcast_ref()) {
                continue;
            }
            widget.unparent();
            widget.set_parent(&self.surface);
            self.surface.state_mut().children.push(widget.clone());
        }
        self.surface.queue_allocate();
        self.surface.queue_draw();
    }

    fn sync_text(&self, node: NodeId) -> Result<(), LinuxError> {
        let Some(content) = self.texts.get(&node) else {
            return Ok(());
        };
        let widget = self.node(node)?;
        let width = widget.state().geometry.content_box.width;
        let layout = content
            .prepared_content
            .and_then(|id| self.prepared.borrow().get(&id).map(pango::Layout::copy))
            .unwrap_or_else(|| text::build_layout(&self.context, &content.payload, Some(width)));
        let payload = &content.payload;
        let wraps = payload.wrap != MeasureTextWrap::NoWrap;
        text::set_layout_width(layout.clone(), payload, Some(width));
        let mut offset = [0.0, 0.0];
        if !wraps {
            let (natural, _) = text::layout_size(&layout, None);
            let factor = match payload.alignment {
                MeasureTextAlignment::Center => 0.5,
                MeasureTextAlignment::End | MeasureTextAlignment::Right => 1.0,
                MeasureTextAlignment::Start | MeasureTextAlignment::Left => 0.0,
            };
            offset[0] = ((width - natural) * factor).max(0.0);
        }
        text::apply_decoration(&layout, &content.paint.decoration);
        widget.state_mut().text = Some(PaintedText {
            layout,
            color: crate::color::rgba(&content.paint.foreground),
            offset,
            shadows: content
                .paint
                .shadows
                .iter()
                .rev()
                .map(|shadow| {
                    gtk::gsk::Shadow::new(
                        crate::color::rgba(&shadow.color),
                        shadow.offset_x,
                        shadow.offset_y,
                        shadow.blur_radius,
                    )
                })
                .collect(),
        });
        widget.queue_draw();
        Ok(())
    }

    fn set_property(
        &self,
        node: NodeId,
        property: whisker_protocol::PropertyId,
        value: Option<&WhiskerValue>,
    ) -> Result<(), LinuxError> {
        let binding = self.binding(node)?;
        if binding.kind != ElementKind::ScrollView {
            return Ok(());
        }
        let Some(schema) = binding.registration.property(property) else {
            return Err(LinuxError(format!(
                "{} has no property {}",
                binding.registration.name,
                property.get()
            )));
        };
        let widget = self.node(node)?;
        let mut state = widget.state_mut();
        let scroll = state.scroll.get_or_insert_with(ScrollState::default);
        match (schema.name.as_str(), value) {
            ("scroll-orientation", Some(WhiskerValue::String(value))) => {
                scroll.horizontal = value == "horizontal";
            }
            ("scroll-orientation", None) => scroll.horizontal = false,
            ("enable-scroll", Some(WhiskerValue::Bool(enabled))) => scroll.enabled = *enabled,
            ("enable-scroll", None) => scroll.enabled = true,
            _ => {}
        }
        Ok(())
    }

    fn invoke(
        &self,
        node: NodeId,
        command: whisker_protocol::CommandId,
        arguments: &WhiskerValue,
    ) -> Result<(), LinuxError> {
        if self.binding(node)?.kind != ElementKind::ScrollView
            || (command != SCROLL_TO_COMMAND && command != SCROLL_BY_COMMAND)
        {
            return Ok(());
        }
        let WhiskerValue::Map(arguments) = arguments else {
            return Err(LinuxError("scroll command arguments must be a map".into()));
        };
        let distance = match arguments.get("offset") {
            Some(WhiskerValue::Float(value)) => *value as f32,
            Some(WhiskerValue::Int(value)) => *value as f32,
            _ => 0.0,
        };
        let widget = self.node(node)?;
        let Some(scroll) = widget.state().scroll else {
            return Ok(());
        };
        let axis = usize::from(!scroll.horizontal);
        let mut offset = scroll.offset;
        offset[axis] = if command == SCROLL_BY_COMMAND {
            offset[axis] + distance
        } else {
            distance
        };
        self.scroll_to(node, offset);
        Ok(())
    }

    fn apply_operation(&mut self, operation: &Operation) -> Result<(), LinuxError> {
        match operation {
            Operation::CreateNode { node, element_type } => {
                let binding = self.elements.get(element_type).ok_or_else(|| {
                    LinuxError(format!("unknown element type {}", element_type.get()))
                })?;
                let widget = NodeWidget::new();
                widget.state_mut().id = Some(*node);
                if binding.kind == ElementKind::ScrollView {
                    widget.state_mut().scroll = Some(ScrollState::default());
                }
                self.nodes.insert(*node, widget);
                self.node_types.insert(*node, *element_type);
                self.node_order.push(*node);
            }
            Operation::DeleteNode { node } => self.delete(*node),
            Operation::InsertChild {
                parent,
                child,
                index,
            }
            | Operation::MoveChild {
                parent,
                child,
                index,
            } => {
                let parent_widget = self.node(*parent)?.clone();
                let child_widget = self.node(*child)?.clone();
                if child_widget.parent().as_ref() == Some(parent_widget.upcast_ref()) {
                    parent_widget
                        .state_mut()
                        .children
                        .retain(|candidate| candidate != &child_widget);
                } else {
                    self.detach(*child);
                    child_widget.set_parent(&parent_widget);
                }
                {
                    let mut state = parent_widget.state_mut();
                    let index = (*index as usize).min(state.children.len());
                    state.children.insert(index, child_widget);
                }
                self.parents.insert(*child, *parent);
                parent_widget.queue_allocate();
                parent_widget.queue_draw();
            }
            Operation::RemoveChild { child, .. } => self.detach(*child),
            Operation::SetLayout { node, geometry } => {
                let widget = self.node(*node)?;
                widget.state_mut().geometry = *geometry;
                if let Some(parent) = widget.parent() {
                    parent.queue_allocate();
                }
                widget.queue_allocate();
                widget.queue_draw();
                self.sync_text(*node)?;
            }
            Operation::SetBoxPaint { node, paint } => {
                let widget = self.node(*node)?;
                widget.state_mut().paint = Some(paint.clone());
                widget.queue_draw();
            }
            Operation::SetClip { node, clip } => {
                let widget = self.node(*node)?;
                widget.state_mut().clip = *clip;
                widget.queue_draw();
            }
            Operation::SetTransform { node, transform } => {
                let widget = self.node(*node)?;
                widget.state_mut().transform = Some(*transform);
                if let Some(parent) = widget.parent() {
                    parent.queue_allocate();
                }
            }
            Operation::SetOpacity { node, opacity } => {
                let widget = self.node(*node)?;
                widget.state_mut().opacity = *opacity;
                widget.queue_draw();
            }
            Operation::SetVisibility { node, visibility } => {
                let widget = self.node(*node)?;
                widget.state_mut().visibility = *visibility;
                widget.queue_draw();
            }
            Operation::SetZOrder { node, z_order } => {
                let widget = self.node(*node)?;
                widget.state_mut().z_order = *z_order;
                if let Some(parent) = widget.parent() {
                    parent.queue_draw();
                }
            }
            Operation::SetText { node, content } => {
                if self.binding(*node)?.kind != ElementKind::Text {
                    return Err(LinuxError(format!(
                        "node {} does not accept text content",
                        node.get()
                    )));
                }
                self.texts.insert(*node, content.clone());
                self.sync_text(*node)?;
            }
            Operation::SetProperty {
                node,
                property,
                value,
            } => self.set_property(*node, *property, Some(value))?,
            Operation::ClearProperty { node, property } => {
                self.set_property(*node, *property, None)?
            }
            Operation::SetEventMask { node, event_mask } => {
                self.event_masks.insert(*node, *event_mask);
            }
            Operation::InvokeCommand {
                node,
                command,
                arguments,
            } => self.invoke(*node, *command, arguments)?,
            Operation::SetCursor { node, cursor } => {
                self.node(*node)?.state_mut().cursor = cursor.fallback;
            }
            Operation::SetBackgroundLayers { node, layers } => {
                let widget = self.node(*node)?;
                widget.state_mut().layers = layers.clone();
                widget.queue_draw();
            }
            Operation::SetVisualEffects { node, effects } => {
                let widget = self.node(*node)?;
                widget.state_mut().effects = effects.clone();
                widget.queue_draw();
            }
            Operation::SetTextStyle { .. }
            | Operation::SetAccessibility { .. }
            | Operation::SetHitTest { .. }
            | Operation::SetPointerCapture { .. }
            | Operation::ReleasePointerCapture { .. } => {}
        }
        Ok(())
    }

    fn apply(&mut self, packet: &FramePacket) -> Result<(), LinuxError> {
        if packet.header.mode == FrameMode::Snapshot {
            self.reset();
        }
        for operation in &packet.operations {
            self.apply_operation(operation)?;
        }
        self.attach_roots();
        Ok(())
    }
}

impl FrameSink for GtkFrameSink {
    type Error = LinuxError;

    fn capabilities(&self) -> RenderCapabilities {
        self.capabilities
    }

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        if let Some(capability) = self.capabilities.first_unsupported(packet) {
            return Err(LinuxError(format!(
                "GTK Host does not implement protocol feature {}",
                capability.as_str()
            )));
        }
        let result = self
            .projection
            .apply(packet)
            .map_err(|error| LinuxError(error.to_string()))?;
        if matches!(result, ApplyResult::Accepted { .. })
            && let Err(error) = self.apply(packet)
        {
            eprintln!("whisker: GTK Host discarded a partially applied frame: {error}");
            self.reset();
            self.projection = SceneProjection::new(self.projection.surface());
            return Ok(ApplyResult::NeedSnapshot {
                receiver_revision: 0,
            });
        }
        Ok(result)
    }
}

/// Scrollable content size: the furthest child edge plus trailing padding.
pub(crate) fn content_extent(widget: &NodeWidget) -> [f32; 2] {
    let state = widget.state();
    let border = state.geometry.border_box;
    let content = state.geometry.content_box;
    let trailing = [
        border.width - content.x - content.width,
        border.height - content.y - content.height,
    ];
    let mut extent = [border.width, border.height];
    for child in &state.children {
        let rect = child.state().geometry.border_box;
        extent[0] = extent[0].max(rect.x + rect.width + trailing[0]);
        extent[1] = extent[1].max(rect.y + rect.height + trailing[1]);
    }
    extent
}

pub(crate) fn max_scroll_offset(widget: &NodeWidget) -> [f32; 2] {
    let extent = content_extent(widget);
    let state = widget.state();
    let border = state.geometry.border_box;
    let Some(scroll) = state.scroll else {
        return [0.0, 0.0];
    };
    let mut max = [
        (extent[0] - border.width).max(0.0),
        (extent[1] - border.height).max(0.0),
    ];
    max[usize::from(scroll.horizontal)] = 0.0;
    max
}
