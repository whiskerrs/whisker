use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use whisker_engine::FrameSink;
use whisker_protocol::{
    ApplyResult, ElementRegistration, ElementTypeId, FrameMode, FramePacket, NodeId, Operation,
    SceneProjection, SurfaceId,
};

use crate::application::request_frame;
use crate::scene::element_registry::DomElementRegistry;
use crate::{
    WebElementFactory, WebElementFactoryKind, WebError, WebEventEmitter, WebNativeElement,
    WebNativeEvent, WhiskerValue, js_error, paint, px, set_style,
};

pub(crate) struct DomFrameSink {
    document: web_sys::Document,
    root: web_sys::Element,
    projection: SceneProjection,
    elements: DomElementRegistry,
    nodes: HashMap<NodeId, web_sys::Element>,
    node_types: HashMap<NodeId, ElementTypeId>,
    parents: HashMap<NodeId, NodeId>,
    layouts: HashMap<NodeId, whisker_protocol::LayoutGeometry>,
    text_nodes: HashMap<NodeId, web_sys::Element>,
    native_nodes: HashMap<NodeId, Box<dyn WebNativeElement>>,
    event_masks: HashMap<NodeId, Rc<Cell<u64>>>,
    pending_events: Rc<RefCell<VecDeque<WebProviderEvent>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WebProviderEvent {
    pub(crate) target: NodeId,
    pub(crate) name: String,
    pub(crate) detail: WhiskerValue,
}

impl DomFrameSink {
    pub(crate) fn new(
        document: web_sys::Document,
        root: web_sys::Element,
        surface: SurfaceId,
        registrations: &[ElementRegistration],
        factories: &[WebElementFactory],
    ) -> Result<Self, WebError> {
        Ok(Self {
            document,
            root,
            projection: SceneProjection::new(surface),
            elements: DomElementRegistry::bind(registrations, factories)?,
            nodes: HashMap::new(),
            node_types: HashMap::new(),
            parents: HashMap::new(),
            layouts: HashMap::new(),
            text_nodes: HashMap::new(),
            native_nodes: HashMap::new(),
            event_masks: HashMap::new(),
            pending_events: Rc::new(RefCell::new(VecDeque::new())),
        })
    }

    pub(crate) fn take_events(&self) -> Vec<WebProviderEvent> {
        self.pending_events.borrow_mut().drain(..).collect()
    }

    fn apply(&mut self, packet: &FramePacket) -> Result<(), WebError> {
        if let Some(feature) = packet
            .operations
            .iter()
            .find_map(|operation| match operation {
                Operation::SetBackgroundLayers { .. } => Some("background-layers"),
                Operation::SetVisualEffects { .. } => Some("visual-effects"),
                Operation::SetImage { .. } => Some("image-content"),
                Operation::SetCursor { .. } => Some("cursor"),
                Operation::SetText { content, .. } if content.paint.uses_extended_features() => {
                    Some("text-effects")
                }
                Operation::SetText { content, .. }
                    if content.payload.style.uses_extended_typography() =>
                {
                    Some("text-typography")
                }
                _ => None,
            })
        {
            return Err(WebError(format!(
                "DOM Host does not implement protocol feature {feature}"
            )));
        }
        if packet.header.mode == FrameMode::Snapshot {
            self.root.set_inner_html("");
            self.nodes.clear();
            self.node_types.clear();
            self.parents.clear();
            self.layouts.clear();
            self.text_nodes.clear();
            self.native_nodes.clear();
            self.event_masks.clear();
            self.pending_events.borrow_mut().clear();
        }
        for operation in &packet.operations {
            self.apply_operation(operation)?;
        }
        Ok(())
    }

    fn apply_operation(&mut self, operation: &Operation) -> Result<(), WebError> {
        match operation {
            Operation::CreateNode { node, element_type } => {
                let binding = self.elements.binding(*element_type)?.clone();
                let event_mask = Rc::new(Cell::new(0));
                let emitter = WebEventEmitter({
                    let registration = binding.registration.clone();
                    let event_mask = Rc::clone(&event_mask);
                    let pending = Rc::clone(&self.pending_events);
                    let node = *node;
                    Rc::new(move |event: WebNativeEvent| {
                        let Some(schema) = registration.event_named(&event.event) else {
                            web_sys::console::error_1(
                                &format!(
                                    "DOM element {} emitted unknown event {}",
                                    registration.name, event.event
                                )
                                .into(),
                            );
                            return;
                        };
                        if !schema.accepts_detail(&event.detail) {
                            web_sys::console::error_1(
                                &format!(
                                    "DOM element {} emitted invalid detail for {}",
                                    registration.name, schema.name
                                )
                                .into(),
                            );
                            return;
                        }
                        let mask = schema
                            .mask()
                            .expect("registration validation checked event ID");
                        if event_mask.get() & mask == 0 {
                            return;
                        }
                        pending.borrow_mut().push_back(WebProviderEvent {
                            target: node,
                            name: schema.name.clone(),
                            detail: event.detail,
                        });
                        request_frame();
                    })
                });
                let (element, native) = match &binding.factory {
                    WebElementFactoryKind::Tag(tag_name) => (
                        self.document
                            .create_element(tag_name)
                            .map_err(|error| js_error("create Whisker DOM node", error))?,
                        None,
                    ),
                    WebElementFactoryKind::Native(create) => {
                        let native = create(&self.document, emitter)
                            .map_err(|error| js_error("create native Whisker DOM node", error))?;
                        (native.element(), Some(native))
                    }
                    WebElementFactoryKind::Declared(_) => {
                        unreachable!("DOM declared factory was not bound at bootstrap")
                    }
                };
                element
                    .set_attribute("data-whisker-node", &node.get().to_string())
                    .map_err(|error| js_error("mark Whisker DOM node", error))?;
                element
                    .set_attribute("data-whisker-content", binding.factory.name())
                    .map_err(|error| js_error("mark Whisker DOM element content", error))?;
                set_style(&element, "position", "absolute")?;
                set_style(&element, "box-sizing", "border-box")?;
                if binding.scroll_content {
                    set_style(&element, "overflow-x", "hidden")?;
                    set_style(&element, "overflow-y", "auto")?;
                }
                self.root
                    .append_child(&element)
                    .map_err(|error| js_error("attach Whisker DOM node", error))?;
                self.nodes.insert(*node, element);
                self.node_types.insert(*node, *element_type);
                self.event_masks.insert(*node, event_mask);
                if let Some(native) = native {
                    self.native_nodes.insert(*node, native);
                }
            }
            Operation::DeleteNode { node } => self.delete_subtree(*node),
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
                let parent_element = self.node(*parent)?;
                let child_element = self.node(*child)?;
                let reference = parent_element.children().item(*index);
                parent_element
                    .insert_before(&child_element, reference.as_ref().map(AsRef::as_ref))
                    .map_err(|error| js_error("insert Whisker DOM child", error))?;
                self.parents.insert(*child, *parent);
            }
            Operation::RemoveChild { parent: _, child } => {
                if let Some(element) = self.nodes.get(child) {
                    element.remove();
                }
                self.parents.remove(child);
            }
            Operation::SetLayout { node, geometry } => {
                let element = self.node(*node)?;
                let rect = geometry.border_box;
                set_style(&element, "left", &px(rect.x))?;
                set_style(&element, "top", &px(rect.y))?;
                set_style(&element, "width", &px(rect.width))?;
                set_style(&element, "height", &px(rect.height))?;
                self.layouts.insert(*node, *geometry);
                if let Some(text) = self.text_nodes.get(node) {
                    position_text(text, geometry.content_box)?;
                }
            }
            Operation::SetBoxPaint { node, paint } => {
                let element = self.node(*node)?;
                paint::box_paint::apply(&element, paint)?;
            }
            Operation::SetClip { node, clip } => {
                let element = self.node(*node)?;
                let element_type = *self
                    .node_types
                    .get(node)
                    .ok_or_else(|| WebError(format!("missing DOM element type for {node:?}")))?;
                let scroll_content = self.elements.binding(element_type)?.scroll_content;
                paint::clip::apply(&element, *clip, scroll_content)?;
            }
            Operation::SetTransform { node, transform } => {
                paint::transform::apply(&self.node(*node)?, *transform)?;
            }
            Operation::SetOpacity { node, opacity } => {
                paint::compositing::apply_opacity(&self.node(*node)?, *opacity)?;
            }
            Operation::SetVisibility { node, visibility } => {
                paint::compositing::apply_visibility(&self.node(*node)?, *visibility)?;
            }
            Operation::SetZOrder { node, z_order } => {
                paint::compositing::apply_z_order(&self.node(*node)?, *z_order)?;
            }
            Operation::SetText { node, content } => {
                let element_type = self.node_types.get(node).copied().ok_or_else(|| {
                    WebError(format!("DOM projection is missing node {}", node.get()))
                })?;
                if !self.elements.binding(element_type)?.text_content {
                    return Err(WebError(format!(
                        "DOM Host received text for non-text node {}",
                        node.get()
                    )));
                }
                let text = if let Some(text) = self.text_nodes.get(node) {
                    text.clone()
                } else {
                    let text = self
                        .document
                        .create_element("span")
                        .map_err(|error| js_error("create Whisker DOM text", error))?;
                    text.set_attribute("data-whisker-text", "")
                        .map_err(|error| js_error("mark Whisker DOM text", error))?;
                    set_style(&text, "position", "absolute")?;
                    self.node(*node)?
                        .append_child(&text)
                        .map_err(|error| js_error("attach Whisker DOM text", error))?;
                    self.text_nodes.insert(*node, text.clone());
                    text
                };
                if let Some(geometry) = self.layouts.get(node) {
                    position_text(&text, geometry.content_box)?;
                }
                paint::text::apply(&text, content)?;
            }
            Operation::SetHitTest { node, behavior } => {
                let disabled = matches!(
                    behavior,
                    whisker_protocol::HitTestBehavior::None
                        | whisker_protocol::HitTestBehavior::DescendantsOnly
                );
                set_style(
                    &self.node(*node)?,
                    "pointer-events",
                    if disabled { "none" } else { "auto" },
                )?;
            }
            Operation::SetProperty {
                node,
                property,
                value,
            } => {
                let element_type = *self.node_types.get(node).ok_or_else(|| {
                    WebError(format!("DOM projection is missing node {}", node.get()))
                })?;
                let registration = &self.elements.binding(element_type)?.registration;
                let schema = registration.property(*property).ok_or_else(|| {
                    WebError(format!(
                        "DOM element {} has no property {}",
                        registration.name,
                        property.get()
                    ))
                })?;
                if !schema.value.accepts(value) {
                    return Err(WebError(format!(
                        "DOM property {} expected {:?}",
                        schema.name, schema.value
                    )));
                }
                self.native_nodes
                    .get_mut(node)
                    .ok_or_else(|| WebError(format!("DOM node {} is not native", node.get())))?
                    .set_property(*property, value)
                    .map_err(|error| js_error("set native DOM property", error))?;
            }
            Operation::ClearProperty { node, property } => {
                let element_type = *self.node_types.get(node).ok_or_else(|| {
                    WebError(format!("DOM projection is missing node {}", node.get()))
                })?;
                let registration = &self.elements.binding(element_type)?.registration;
                registration.property(*property).ok_or_else(|| {
                    WebError(format!(
                        "DOM element {} has no property {}",
                        registration.name,
                        property.get()
                    ))
                })?;
                self.native_nodes
                    .get_mut(node)
                    .ok_or_else(|| WebError(format!("DOM node {} is not native", node.get())))?
                    .clear_property(*property)
                    .map_err(|error| js_error("clear native DOM property", error))?;
            }
            Operation::SetEventMask { node, event_mask } => {
                self.event_masks
                    .get(node)
                    .ok_or_else(|| {
                        WebError(format!("DOM projection is missing node {}", node.get()))
                    })?
                    .set(*event_mask);
            }
            Operation::InvokeCommand {
                node,
                command,
                arguments,
                ..
            } => {
                let element_type = *self.node_types.get(node).ok_or_else(|| {
                    WebError(format!("DOM projection is missing node {}", node.get()))
                })?;
                let registration = &self.elements.binding(element_type)?.registration;
                let schema = registration.command(*command).ok_or_else(|| {
                    WebError(format!(
                        "DOM element {} has no command {}",
                        registration.name,
                        command.get()
                    ))
                })?;
                if !schema.arguments.accepts(arguments) {
                    return Err(WebError(format!(
                        "DOM command {} expected {:?}",
                        schema.name, schema.arguments
                    )));
                }
                self.native_nodes
                    .get_mut(node)
                    .ok_or_else(|| WebError(format!("DOM node {} is not native", node.get())))?
                    .invoke_command(*command, arguments)
                    .map_err(|error| js_error("invoke native DOM command", error))?;
            }
            Operation::SetPointerCapture { .. } | Operation::ReleasePointerCapture { .. } => {}
            Operation::SetBackgroundLayers { .. }
            | Operation::SetVisualEffects { .. }
            | Operation::SetImage { .. }
            | Operation::SetCursor { .. } => {
                unreachable!("unsupported operations are rejected before DOM mutation")
            }
        }
        Ok(())
    }

    fn node(&self, node: NodeId) -> Result<web_sys::Element, WebError> {
        self.nodes
            .get(&node)
            .cloned()
            .ok_or_else(|| WebError(format!("DOM projection is missing node {}", node.get())))
    }

    fn delete_subtree(&mut self, root: NodeId) {
        if let Some(element) = self.nodes.get(&root) {
            element.remove();
        }
        let mut deleted = vec![root];
        let mut cursor = 0;
        while cursor < deleted.len() {
            let parent = deleted[cursor];
            deleted.extend(
                self.parents
                    .iter()
                    .filter_map(|(child, candidate)| (*candidate == parent).then_some(*child)),
            );
            cursor += 1;
        }
        for node in deleted {
            self.nodes.remove(&node);
            self.node_types.remove(&node);
            self.parents.remove(&node);
            self.layouts.remove(&node);
            self.text_nodes.remove(&node);
            self.native_nodes.remove(&node);
            self.event_masks.remove(&node);
        }
    }
}

fn position_text(
    element: &web_sys::Element,
    rect: whisker_protocol::LayoutRect,
) -> Result<(), WebError> {
    set_style(element, "left", &px(rect.x))?;
    set_style(element, "top", &px(rect.y))?;
    set_style(element, "width", &px(rect.width))?;
    set_style(element, "height", &px(rect.height))?;
    set_style(element, "overflow", "hidden")
}

impl FrameSink for DomFrameSink {
    type Error = WebError;

    fn capabilities(&self) -> whisker_protocol::RenderCapabilities {
        whisker_protocol::RenderCapabilities::new(
            whisker_protocol::ProtocolVersion::CURRENT,
            [whisker_protocol::CapabilityEntry {
                capability: whisker_protocol::RenderCapability::EllipticalBorderRadius,
                support: whisker_protocol::CapabilitySupport::Native,
            }],
        )
        .expect("Web capability profile is unique")
    }

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        if let Some(capability) = self.capabilities().first_unsupported(packet) {
            return Err(WebError(format!(
                "DOM Host does not implement protocol feature {}",
                capability.as_str()
            )));
        }
        let mut next = self.projection.clone();
        let result = next
            .apply(packet)
            .map_err(|error| WebError(error.to_string()))?;
        if matches!(result, ApplyResult::Accepted { .. }) {
            self.apply(packet)?;
            self.projection = next;
        }
        Ok(result)
    }
}
