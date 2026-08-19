//! Runtime ownership of one retained semantic surface.

use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::rc::Rc;

use crate::runtime::element::ElementTag;
use crate::runtime::value::WhiskerValue;
use crate::runtime::view::{BindType, DynRenderer, Element};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{ElementTypeId, NodeId, SurfaceId};
use whisker_engine::whisker_style::{
    ResolvedNodeStyle, SpecifiedStyle, StyleEnvironment, StyleResolutionError, resolve_style,
};
use whisker_engine::{
    FrameSink, HostLayoutError, HostLayoutOptions, LayoutProgress, MeasurementHost, PlainTextInput,
    SurfaceEngine, SurfaceError, SurfacePresentError,
};

/// A mutation emitted by `render!` that could not enter the retained surface.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeBindingError {
    /// The runtime named an element handle unknown to this surface.
    UnknownElement {
        /// Missing runtime handle.
        element: Element,
    },
    /// A custom element has no registered provider on this surface.
    UnsupportedCustomElement {
        /// Requested element name.
        name: String,
    },
    /// A compatibility-only CSS string reached the typed renderer.
    UnsupportedRawStyle {
        /// Styled runtime handle.
        element: Element,
        /// Original CSS retained for diagnostics.
        css: String,
    },
    /// An attribute has not yet been mapped to a typed element property.
    UnsupportedAttribute {
        /// Target runtime handle.
        element: Element,
        /// Attribute name.
        name: String,
    },
    /// A raw-text helper was attached outside a Text element.
    InvalidRawTextParent {
        /// Raw-text runtime handle.
        element: Element,
        /// Requested parent.
        parent: Element,
    },
    /// An event has not yet been assigned a protocol bit.
    UnsupportedEvent {
        /// Target runtime handle.
        element: Element,
        /// Runtime event name.
        name: String,
    },
    /// The runtime selected a virtual raw-text node as the surface root.
    InvalidRoot {
        /// Invalid runtime handle.
        element: Element,
    },
    /// Runtime element handles exhausted their reserved `u32` range.
    ElementIdExhausted,
    /// Typed style resolution failed.
    Style(StyleResolutionError),
    /// The retained scene or layout engine rejected the mutation.
    Surface(SurfaceError),
}

impl fmt::Display for RuntimeBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker render binding error: {self:?}")
    }
}

impl Error for RuntimeBindingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Style(error) => Some(error),
            Self::Surface(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StyleResolutionError> for RuntimeBindingError {
    fn from(error: StyleResolutionError) -> Self {
        Self::Style(error)
    }
}

impl From<SurfaceError> for RuntimeBindingError {
    fn from(error: SurfaceError) -> Self {
        Self::Surface(error)
    }
}

/// Failure while driving layout for a surface populated through `render!`.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeLayoutError<HostError> {
    /// A prior runtime mutation was rejected.
    Binding(RuntimeBindingError),
    /// The runtime has not called `set_root` yet.
    MissingRoot,
    /// Host measurement or retained layout failed.
    Host(HostLayoutError<HostError>),
}

/// Failure while presenting a surface populated through `render!`.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimePresentError<SinkError> {
    /// A prior runtime mutation was rejected.
    Binding(RuntimeBindingError),
    /// Frame preparation, Host presentation, or acknowledgement failed.
    Present(SurfacePresentError<SinkError>),
}

impl<SinkError: fmt::Debug> fmt::Display for RuntimePresentError<SinkError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker render presentation error: {self:?}")
    }
}

impl<SinkError: Error + 'static> Error for RuntimePresentError<SinkError> {}

/// Output of one complete runtime frame.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeFrame {
    /// Measurement and layout progress for this frame.
    pub layout: LayoutProgress,
    /// Host acknowledgement, absent when blocking measurement withheld paint.
    pub presentation: Option<whisker_engine::whisker_protocol::ApplyResult>,
}

/// Failure while measuring, laying out, or presenting one runtime frame.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeFrameError<HostError, SinkError> {
    /// Measurement or layout failed.
    Layout(RuntimeLayoutError<HostError>),
    /// Frame presentation failed.
    Present(RuntimePresentError<SinkError>),
}

impl<HostError: fmt::Debug, SinkError: fmt::Debug> fmt::Display
    for RuntimeFrameError<HostError, SinkError>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker runtime frame error: {self:?}")
    }
}

impl<HostError, SinkError> Error for RuntimeFrameError<HostError, SinkError>
where
    HostError: Error + 'static,
    SinkError: Error + 'static,
{
}

impl<HostError: fmt::Debug> fmt::Display for RuntimeLayoutError<HostError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker render layout error: {self:?}")
    }
}

impl<HostError: Error + 'static> Error for RuntimeLayoutError<HostError> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Binding(error) => Some(error),
            Self::Host(error) => Some(error),
            Self::MissingRoot => None,
        }
    }
}

/// First-class runtime for one retained surface populated by [`render!`](crate::render).
///
/// Clones share one single-threaded surface. Install [`Self::renderer`] through
/// `whisker_runtime::view::with_installed_renderer`, build the declarative tree,
/// call `set_root`, and then drive Host measurement through this handle.
#[derive(Clone)]
pub struct SurfaceRuntime {
    state: Rc<RefCell<BindingState>>,
}

impl SurfaceRuntime {
    /// Creates an empty renderer-backed surface for one style environment.
    pub fn new(surface: SurfaceId, environment: StyleEnvironment) -> Self {
        Self {
            state: Rc::new(RefCell::new(BindingState {
                surface: SurfaceEngine::new(surface),
                environment,
                next_element: 0,
                elements: HashMap::new(),
                root: None,
                error: None,
            })),
        }
    }

    /// Returns a renderer sharing this surface, ready for runtime installation.
    pub fn renderer(&self) -> Box<dyn DynRenderer> {
        Box::new(self.clone())
    }

    /// Returns the semantic surface identifier.
    pub fn surface(&self) -> SurfaceId {
        self.state.borrow().surface.surface()
    }

    /// Returns the root selected by the runtime, when available.
    pub fn root(&self) -> Option<NodeId> {
        self.state.borrow().root
    }

    /// Returns the first rejected runtime mutation without clearing it.
    pub fn binding_error(&self) -> Option<RuntimeBindingError> {
        self.state.borrow().error.clone()
    }

    /// Runs Taffy and all synchronously available Host measurements.
    pub fn drive_layout_with_host<Host: MeasurementHost>(
        &self,
        viewport: LayoutSize,
        environment_epoch: u64,
        host: &mut Host,
        options: HostLayoutOptions,
    ) -> Result<LayoutProgress, RuntimeLayoutError<Host::Error>> {
        let mut state = self.state.borrow_mut();
        if let Some(error) = state.error.clone() {
            return Err(RuntimeLayoutError::Binding(error));
        }
        let root = state.root.ok_or(RuntimeLayoutError::MissingRoot)?;
        state
            .surface
            .drive_layout_with_host(root, viewport, environment_epoch, host, options)
            .map_err(RuntimeLayoutError::Host)
    }

    /// Presents the next transaction and records the Host acknowledgement.
    pub fn present<Sink: FrameSink>(
        &self,
        viewport_epoch: u32,
        sink: &mut Sink,
    ) -> Result<
        Option<whisker_engine::whisker_protocol::ApplyResult>,
        RuntimePresentError<Sink::Error>,
    > {
        let mut state = self.state.borrow_mut();
        state.ensure_valid().map_err(RuntimePresentError::Binding)?;
        state
            .surface
            .present(viewport_epoch, sink)
            .map_err(RuntimePresentError::Present)
    }

    /// Runs Host measurement, final layout, and transactional presentation.
    pub fn render_frame<Host: MeasurementHost, Sink: FrameSink>(
        &self,
        viewport: LayoutSize,
        environment_epoch: u64,
        viewport_epoch: u32,
        host: &mut Host,
        sink: &mut Sink,
        options: HostLayoutOptions,
    ) -> Result<RuntimeFrame, RuntimeFrameError<Host::Error, Sink::Error>> {
        let layout = self
            .drive_layout_with_host(viewport, environment_epoch, host, options)
            .map_err(RuntimeFrameError::Layout)?;
        let presentation = if layout.has_layout() {
            self.present(viewport_epoch, sink)
                .map_err(RuntimeFrameError::Present)?
        } else {
            None
        };
        Ok(RuntimeFrame {
            layout,
            presentation,
        })
    }
}

#[derive(Clone)]
struct BoundElement {
    tag: ElementTag,
    node: Option<NodeId>,
    parent: Option<Element>,
    children: Vec<Element>,
    specified: SpecifiedStyle,
    resolved: Option<ResolvedNodeStyle>,
    text: Option<PlainTextInput>,
    raw_text: String,
}

struct BindingState {
    surface: SurfaceEngine,
    environment: StyleEnvironment,
    next_element: u32,
    elements: HashMap<Element, BoundElement>,
    root: Option<NodeId>,
    error: Option<RuntimeBindingError>,
}

impl BindingState {
    fn ensure_valid(&self) -> Result<(), RuntimeBindingError> {
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    fn record(&mut self, result: Result<(), RuntimeBindingError>) {
        if let Err(error) = result
            && self.error.is_none()
        {
            self.error = Some(error);
        }
    }

    fn allocate(&mut self, tag: ElementTag) -> Result<Element, RuntimeBindingError> {
        if self.next_element == u32::MAX {
            return Err(RuntimeBindingError::ElementIdExhausted);
        }
        let handle = Element::from_raw(self.next_element);
        self.next_element += 1;
        let (node, resolved, text) = if tag == ElementTag::RawText {
            (None, None, None)
        } else {
            let resolved = resolve_style(&SpecifiedStyle::new(), None, self.environment)?;
            let element_type =
                ElementTypeId::new(tag as u32).expect("built-in element tags are non-zero");
            let node = self
                .surface
                .create_node(element_type, resolved.computed().layout().clone())?;
            let text = (tag == ElementTag::Text).then(|| PlainTextInput::new(""));
            (Some(node), Some(resolved), text)
        };
        self.elements.insert(
            handle,
            BoundElement {
                tag,
                node,
                parent: None,
                children: Vec::new(),
                specified: SpecifiedStyle::new(),
                resolved,
                text,
                raw_text: String::new(),
            },
        );
        Ok(handle)
    }

    fn element(&self, element: Element) -> Result<&BoundElement, RuntimeBindingError> {
        self.elements
            .get(&element)
            .ok_or(RuntimeBindingError::UnknownElement { element })
    }

    fn element_mut(&mut self, element: Element) -> Result<&mut BoundElement, RuntimeBindingError> {
        self.elements
            .get_mut(&element)
            .ok_or(RuntimeBindingError::UnknownElement { element })
    }

    fn apply_subtree(&mut self, element: Element) -> Result<(), RuntimeBindingError> {
        let parent_style = self
            .element(element)?
            .parent
            .and_then(|parent| self.elements.get(&parent))
            .and_then(|parent| parent.resolved.as_ref())
            .map(|resolved| resolved.inherited_for_children().clone());
        let specified = self.element(element)?.specified.clone();
        let Some(node) = self.element(element)?.node else {
            return Ok(());
        };
        let resolved = resolve_style(&specified, parent_style.as_ref(), self.environment)?;
        self.surface
            .update_computed_style(node, resolved.computed())?;
        let text = self.element(element)?.text.clone();
        if let Some(input) = text {
            self.surface
                .set_plain_text(node, &input, resolved.computed().inherited_text())?;
        }
        let children = self.element(element)?.children.clone();
        self.element_mut(element)?.resolved = Some(resolved);
        for child in children {
            if self.element(child)?.node.is_some() {
                self.apply_subtree(child)?;
            }
        }
        Ok(())
    }

    fn refresh_text(&mut self, text_element: Element) -> Result<(), RuntimeBindingError> {
        if self.element(text_element)?.tag != ElementTag::Text {
            return Err(RuntimeBindingError::InvalidRawTextParent {
                element: text_element,
                parent: text_element,
            });
        }
        let children = self.element(text_element)?.children.clone();
        let mut value = String::new();
        for child in children {
            let child = self.element(child)?;
            if child.tag == ElementTag::RawText {
                value.push_str(&child.raw_text);
            }
        }
        self.element_mut(text_element)?
            .text
            .as_mut()
            .expect("Text elements always retain plain-text input")
            .text = value;
        self.apply_subtree(text_element)
    }

    fn insert(
        &mut self,
        parent: Element,
        child: Element,
        before: Option<Element>,
    ) -> Result<(), RuntimeBindingError> {
        let parent_entry = self.element(parent)?;
        let child_entry = self.element(child)?;
        if parent_entry.node.is_none() || child_entry.parent.is_some() {
            return Err(RuntimeBindingError::InvalidRawTextParent {
                element: child,
                parent,
            });
        }
        if child_entry.tag == ElementTag::RawText && parent_entry.tag != ElementTag::Text {
            return Err(RuntimeBindingError::InvalidRawTextParent {
                element: child,
                parent,
            });
        }
        let position = match before {
            Some(reference) => self
                .element(parent)?
                .children
                .iter()
                .position(|candidate| *candidate == reference)
                .ok_or(RuntimeBindingError::UnknownElement { element: reference })?,
            None => self.element(parent)?.children.len(),
        };
        let scene_index = self.element(parent)?.children[..position]
            .iter()
            .filter(|candidate| {
                self.elements
                    .get(candidate)
                    .is_some_and(|entry| entry.node.is_some())
            })
            .count() as u32;
        self.element_mut(parent)?.children.insert(position, child);
        self.element_mut(child)?.parent = Some(parent);
        if let Some(child_node) = self.element(child)?.node {
            let parent_node = self
                .element(parent)?
                .node
                .expect("validated scene parent has a node");
            self.surface
                .insert_child(parent_node, child_node, scene_index)?;
            self.apply_subtree(child)?;
        } else {
            self.refresh_text(parent)?;
        }
        Ok(())
    }

    fn detach(&mut self, parent: Element, child: Element) -> Result<(), RuntimeBindingError> {
        let position = self
            .element(parent)?
            .children
            .iter()
            .position(|candidate| *candidate == child)
            .ok_or(RuntimeBindingError::UnknownElement { element: child })?;
        self.element_mut(parent)?.children.remove(position);
        self.element_mut(child)?.parent = None;
        if let Some(child_node) = self.element(child)?.node {
            let parent_node = self
                .element(parent)?
                .node
                .expect("validated scene parent has a node");
            self.surface.remove_child(parent_node, child_node)?;
            self.apply_subtree(child)?;
        } else {
            self.refresh_text(parent)?;
        }
        Ok(())
    }

    fn set_attribute(
        &mut self,
        element: Element,
        name: &str,
        value: &str,
    ) -> Result<(), RuntimeBindingError> {
        let tag = self.element(element)?.tag;
        if tag == ElementTag::RawText && name == "text" {
            self.element_mut(element)?.raw_text = value.to_owned();
            if let Some(parent) = self.element(element)?.parent {
                self.refresh_text(parent)?;
            }
            return Ok(());
        }
        if tag == ElementTag::Text && name == "text-maxline" {
            let max_lines = value.parse::<i32>().ok().and_then(|value| {
                if value > 0 {
                    u32::try_from(value).ok()
                } else {
                    None
                }
            });
            self.element_mut(element)?
                .text
                .as_mut()
                .expect("Text elements always retain plain-text input")
                .max_lines = max_lines;
            self.apply_subtree(element)?;
            return Ok(());
        }
        Err(RuntimeBindingError::UnsupportedAttribute {
            element,
            name: name.to_owned(),
        })
    }
}

impl DynRenderer for SurfaceRuntime {
    fn create_element(&self, tag: ElementTag) -> Element {
        let mut state = self.state.borrow_mut();
        match state.allocate(tag) {
            Ok(element) => element,
            Err(error) => {
                state.record(Err(error));
                Element::from_raw(u32::MAX)
            }
        }
    }

    fn create_element_by_name(&self, tag_name: &str) -> Element {
        let mut state = self.state.borrow_mut();
        state.record(Err(RuntimeBindingError::UnsupportedCustomElement {
            name: tag_name.to_owned(),
        }));
        Element::from_raw(u32::MAX)
    }

    fn release_element(&self, handle: Element) {
        let mut state = self.state.borrow_mut();
        let result = (|| {
            let Some(entry) = state.elements.remove(&handle) else {
                return Ok(());
            };
            if let Some(parent) = entry.parent
                && let Some(parent_entry) = state.elements.get_mut(&parent)
            {
                parent_entry.children.retain(|child| *child != handle);
            }
            if let Some(node) = entry.node
                && state.surface.node(node).is_some()
            {
                state.surface.delete_node(node)?;
            }
            Ok(())
        })();
        state.record(result);
    }

    fn set_attribute(&self, handle: Element, key: &str, value: &str) {
        let mut state = self.state.borrow_mut();
        let result = state.set_attribute(handle, key, value);
        state.record(result);
    }

    fn set_inline_styles(&self, handle: Element, css: &str) {
        if css.trim().is_empty() {
            return;
        }
        let mut state = self.state.borrow_mut();
        state.record(Err(RuntimeBindingError::UnsupportedRawStyle {
            element: handle,
            css: css.to_owned(),
        }));
    }

    fn set_specified_style(&self, handle: Element, style: &SpecifiedStyle) -> bool {
        let mut state = self.state.borrow_mut();
        let result = (|| {
            state.element_mut(handle)?.specified = style.clone();
            state.apply_subtree(handle)
        })();
        let accepted = result.is_ok();
        state.record(result);
        accepted
    }

    fn append_child(&self, parent: Element, child: Element) {
        let mut state = self.state.borrow_mut();
        let result = state.insert(parent, child, None);
        state.record(result);
    }

    fn remove_child(&self, parent: Element, child: Element) {
        let mut state = self.state.borrow_mut();
        let result = state.detach(parent, child);
        state.record(result);
    }

    fn supports_insert_before(&self) -> bool {
        true
    }

    fn insert_child_before(&self, parent: Element, child: Element, reference: Option<Element>) {
        let mut state = self.state.borrow_mut();
        let result = state.insert(parent, child, reference);
        state.record(result);
    }

    fn set_event_listener(
        &self,
        handle: Element,
        event_name: &str,
        _bind_type: BindType,
        callback: Box<dyn Fn(WhiskerValue) + 'static>,
    ) {
        drop(callback);
        let mut state = self.state.borrow_mut();
        state.record(Err(RuntimeBindingError::UnsupportedEvent {
            element: handle,
            name: event_name.to_owned(),
        }));
    }

    fn set_root(&self, page: Element) {
        let mut state = self.state.borrow_mut();
        let result = match state.element(page) {
            Ok(entry) => match entry.node {
                Some(node) => {
                    state.root = Some(node);
                    Ok(())
                }
                None => Err(RuntimeBindingError::InvalidRoot { element: page }),
            },
            Err(error) => Err(error),
        };
        state.record(result);
    }

    fn flush(&self) {}
}
