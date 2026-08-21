//! Browser DOM Host for Whisker.
//!
//! Rust remains authoritative for style resolution and Taffy layout. This Host
//! synchronously measures browser text and applies the resulting semantic frame
//! transaction to DOM nodes using explicit geometry.

#![warn(missing_docs)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use whisker::runtime::RuntimeWakeHandle;
use whisker::{Element, RuntimeInstance, SurfaceRuntime};
use whisker_engine::{FrameSink, HostLayoutOptions, MeasurementHost};
use whisker_protocol::{
    ApplyResult, AvailableSpace, BorderLineStyle, FrameMode, FramePacket, MeasureFontFamily,
    MeasureFontStyle, MeasureLineHeight, MeasureTextDirection, MeasureTextWrap, MeasuredSize,
    MeasurementMetrics, MeasurementPayload, MeasurementRequest, MeasurementResponse, NodeId,
    Operation, OverflowClip, PaintColor, PaintLengthPercentage, PreparedContentId, SceneProjection,
    SurfaceId,
};
use whisker_style::StyleEnvironment;

thread_local! {
    static APPLICATION: RefCell<Option<WebApplication>> = const { RefCell::new(None) };
    static FRAME_SCHEDULED: Cell<bool> = const { Cell::new(false) };
}

/// Configuration for one browser surface.
#[derive(Clone, Debug)]
pub struct WebAppConfig {
    /// Document title.
    pub title: String,
    /// DOM element id used as the surface root.
    pub root_id: String,
}

impl WebAppConfig {
    /// Creates a browser configuration rooted at `#whisker-root`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            root_id: "whisker-root".to_string(),
        }
    }
}

/// Failure while creating or driving the browser Host.
#[derive(Clone, Debug)]
pub struct WebHostError(String);

impl fmt::Display for WebHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for WebHostError {}

/// Mounts a Whisker application into the current browser document.
///
/// The generated `gen/web` crate calls this once from its WASM start
/// function. Subsequent work is driven by `requestAnimationFrame`.
pub fn run(config: WebAppConfig, application: fn() -> Element) -> Result<(), WebHostError> {
    APPLICATION.with(|slot| {
        if slot.borrow().is_some() {
            return Err(WebHostError("a Web application is already mounted".into()));
        }
        *slot.borrow_mut() = Some(WebApplication::new(config)?);
        Ok(())
    })?;

    let mount = APPLICATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.as_mut()
            .expect("application was installed")
            .runtime
            .mount(application)
            .map(|_| ())
            .map_err(|error| WebHostError(format!("mount Whisker application: {error}")))
    });
    if let Err(error) = mount {
        APPLICATION.with(|slot| *slot.borrow_mut() = None);
        return Err(error);
    }

    let resize = Closure::<dyn FnMut()>::new(request_frame);
    browser_window()?
        .add_event_listener_with_callback("resize", resize.as_ref().unchecked_ref())
        .map_err(|error| js_error("register resize listener", error))?;
    resize.forget();
    request_frame();
    Ok(())
}

struct WebApplication {
    runtime: RuntimeInstance,
    measurements: DomMeasurementHost,
    frames: DomFrameSink,
    viewport: (f32, f32, f32),
    viewport_epoch: u32,
    environment_epoch: u64,
}

impl WebApplication {
    fn new(config: WebAppConfig) -> Result<Self, WebHostError> {
        let window = browser_window()?;
        let document = window
            .document()
            .ok_or_else(|| WebHostError("browser document is unavailable".into()))?;
        document.set_title(&config.title);
        let root = document
            .get_element_by_id(&config.root_id)
            .ok_or_else(|| WebHostError(format!("missing Web Host root #{}", config.root_id)))?;
        set_style(&root, "position", "relative")?;
        set_style(&root, "width", "100vw")?;
        set_style(&root, "height", "100vh")?;
        set_style(&root, "overflow", "hidden")?;

        let viewport = viewport(&window)?;
        let surface_id = SurfaceId::new(1).expect("the browser surface id is non-zero");
        let surface = SurfaceRuntime::new(
            surface_id,
            StyleEnvironment::new(viewport.0, viewport.1, viewport.2, 16.0),
        );
        let wake = RuntimeWakeHandle::new(request_frame);
        Ok(Self {
            runtime: RuntimeInstance::new(surface, wake),
            measurements: DomMeasurementHost::new(document.clone()),
            frames: DomFrameSink::new(document, root, surface_id),
            viewport,
            viewport_epoch: 1,
            environment_epoch: 1,
        })
    }

    fn drive_frame(&mut self, timestamp_ms: f64) -> Result<(), WebHostError> {
        let current = viewport(&browser_window()?)?;
        if current != self.viewport {
            self.viewport = current;
            self.viewport_epoch = self.viewport_epoch.wrapping_add(1).max(1);
            self.environment_epoch = self.environment_epoch.wrapping_add(1).max(1);
        }
        let drive = self
            .runtime
            .drive_frame(
                timestamp_ms,
                StyleEnvironment::new(self.viewport.0, self.viewport.1, self.viewport.2, 16.0),
                self.environment_epoch,
                self.viewport_epoch,
                &mut self.measurements,
                &mut self.frames,
                HostLayoutOptions::default(),
            )
            .map_err(|error| WebHostError(format!("drive Web frame: {error}")))?;
        if drive.needs_frame {
            request_frame();
        }
        Ok(())
    }
}

fn request_frame() {
    FRAME_SCHEDULED.with(|scheduled| {
        if scheduled.replace(true) {
            return;
        }
        let callback = Closure::once(move |timestamp_ms: f64| {
            FRAME_SCHEDULED.with(|scheduled| scheduled.set(false));
            let result = APPLICATION.with(|slot| {
                let mut slot = slot.borrow_mut();
                slot.as_mut()
                    .ok_or_else(|| WebHostError("Web application is not mounted".into()))?
                    .drive_frame(timestamp_ms)
            });
            if let Err(error) = result {
                web_sys::console::error_1(&error.to_string().into());
            }
        });
        match web_sys::window().and_then(|window| {
            window
                .request_animation_frame(callback.as_ref().unchecked_ref())
                .ok()
        }) {
            Some(_) => callback.forget(),
            None => scheduled.set(false),
        }
    });
}

fn browser_window() -> Result<web_sys::Window, WebHostError> {
    web_sys::window().ok_or_else(|| WebHostError("browser window is unavailable".into()))
}

fn viewport(window: &web_sys::Window) -> Result<(f32, f32, f32), WebHostError> {
    let width = window
        .inner_width()
        .map_err(|error| js_error("read viewport width", error))?
        .as_f64()
        .ok_or_else(|| WebHostError("viewport width was not numeric".into()))?
        as f32;
    let height = window
        .inner_height()
        .map_err(|error| js_error("read viewport height", error))?
        .as_f64()
        .ok_or_else(|| WebHostError("viewport height was not numeric".into()))?
        as f32;
    Ok((width, height, window.device_pixel_ratio() as f32))
}

struct DomMeasurementHost {
    document: web_sys::Document,
}

impl DomMeasurementHost {
    fn new(document: web_sys::Document) -> Self {
        Self { document }
    }
}

impl MeasurementHost for DomMeasurementHost {
    type Error = WebHostError;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        let body = self
            .document
            .body()
            .ok_or_else(|| WebHostError("document body is unavailable".into()))?;
        for request in requests {
            let MeasurementPayload::Text(text) = &request.payload else {
                responses.push(MeasurementResponse::Ready {
                    key: request.key,
                    environment_epoch: request.environment_epoch,
                    metrics: MeasurementMetrics {
                        size: MeasuredSize::new(0.0, 0.0),
                        first_baseline: None,
                        last_baseline: None,
                        overflow: None,
                        prepared_content: None,
                    },
                });
                continue;
            };
            let probe = self
                .document
                .create_element("div")
                .map_err(|error| js_error("create text measurement probe", error))?;
            set_style(&probe, "position", "absolute")?;
            set_style(&probe, "visibility", "hidden")?;
            set_style(&probe, "pointer-events", "none")?;
            set_style(&probe, "left", "-100000px")?;
            set_style(&probe, "top", "0")?;
            set_style(&probe, "box-sizing", "border-box")?;
            apply_text_metrics_style(&probe, text)?;
            match request.constraints.available_space[0] {
                AvailableSpace::Definite(width) => {
                    set_style(&probe, "width", &px(width.max(0.0)))?;
                }
                AvailableSpace::MinContent => set_style(&probe, "width", "min-content")?,
                AvailableSpace::MaxContent => set_style(&probe, "width", "max-content")?,
            }
            if let Some(width) = request.constraints.known_dimensions[0] {
                set_style(&probe, "width", &px(width))?;
            }
            if let Some(height) = request.constraints.known_dimensions[1] {
                set_style(&probe, "height", &px(height))?;
            }
            probe.set_text_content(Some(&text.text));
            body.append_child(&probe)
                .map_err(|error| js_error("attach text measurement probe", error))?;
            let rect = probe.get_bounding_client_rect();
            probe.remove();
            let baseline = text.style.font_size * 0.8;
            responses.push(MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics: MeasurementMetrics {
                    size: MeasuredSize::new(rect.width() as f32, rect.height() as f32),
                    first_baseline: Some(baseline),
                    last_baseline: Some(baseline),
                    overflow: None,
                    prepared_content: PreparedContentId::new(request.key.get()),
                },
            });
        }
        Ok(())
    }
}

struct DomFrameSink {
    document: web_sys::Document,
    root: web_sys::Element,
    projection: SceneProjection,
    nodes: HashMap<NodeId, web_sys::Element>,
    parents: HashMap<NodeId, NodeId>,
    layouts: HashMap<NodeId, whisker_protocol::LayoutGeometry>,
    text_nodes: HashMap<NodeId, web_sys::Element>,
}

impl DomFrameSink {
    fn new(document: web_sys::Document, root: web_sys::Element, surface: SurfaceId) -> Self {
        Self {
            document,
            root,
            projection: SceneProjection::new(surface),
            nodes: HashMap::new(),
            parents: HashMap::new(),
            layouts: HashMap::new(),
            text_nodes: HashMap::new(),
        }
    }

    fn apply(&mut self, packet: &FramePacket) -> Result<(), WebHostError> {
        if packet.header.mode == FrameMode::Snapshot {
            self.root.set_inner_html("");
            self.nodes.clear();
            self.parents.clear();
            self.layouts.clear();
            self.text_nodes.clear();
        }
        for operation in &packet.operations {
            self.apply_operation(operation)?;
        }
        Ok(())
    }

    fn apply_operation(&mut self, operation: &Operation) -> Result<(), WebHostError> {
        match operation {
            Operation::CreateNode { node, .. } => {
                let element = self
                    .document
                    .create_element("div")
                    .map_err(|error| js_error("create Whisker DOM node", error))?;
                element
                    .set_attribute("data-whisker-node", &node.get().to_string())
                    .map_err(|error| js_error("mark Whisker DOM node", error))?;
                set_style(&element, "position", "absolute")?;
                set_style(&element, "box-sizing", "border-box")?;
                self.root
                    .append_child(&element)
                    .map_err(|error| js_error("attach Whisker DOM node", error))?;
                self.nodes.insert(*node, element);
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
                set_style(
                    &element,
                    "background-color",
                    &color(&paint.background_color),
                )?;
                let widths = &paint.border_widths;
                set_style(&element, "border-top-width", &length(widths.top))?;
                set_style(&element, "border-right-width", &length(widths.right))?;
                set_style(&element, "border-bottom-width", &length(widths.bottom))?;
                set_style(&element, "border-left-width", &length(widths.left))?;
                let colors = &paint.border_colors;
                set_style(&element, "border-top-color", &color(&colors.top))?;
                set_style(&element, "border-right-color", &color(&colors.right))?;
                set_style(&element, "border-bottom-color", &color(&colors.bottom))?;
                set_style(&element, "border-left-color", &color(&colors.left))?;
                let styles = &paint.border_styles;
                set_style(&element, "border-top-style", border_style(styles.top))?;
                set_style(&element, "border-right-style", border_style(styles.right))?;
                set_style(&element, "border-bottom-style", border_style(styles.bottom))?;
                set_style(&element, "border-left-style", border_style(styles.left))?;
                let radii = &paint.border_radii;
                set_style(&element, "border-top-left-radius", &length(radii.top_left))?;
                set_style(
                    &element,
                    "border-top-right-radius",
                    &length(radii.top_right),
                )?;
                set_style(
                    &element,
                    "border-bottom-right-radius",
                    &length(radii.bottom_right),
                )?;
                set_style(
                    &element,
                    "border-bottom-left-radius",
                    &length(radii.bottom_left),
                )?;
            }
            Operation::SetClip { node, clip } => {
                let element = self.node(*node)?;
                set_style(
                    &element,
                    "overflow-x",
                    if clip.horizontal == OverflowClip::Hidden {
                        "hidden"
                    } else {
                        "visible"
                    },
                )?;
                set_style(
                    &element,
                    "overflow-y",
                    if clip.vertical == OverflowClip::Hidden {
                        "hidden"
                    } else {
                        "visible"
                    },
                )?;
            }
            Operation::SetTransform { node, transform } => {
                let value = transform
                    .0
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                set_style(
                    &self.node(*node)?,
                    "transform",
                    &format!("matrix3d({value})"),
                )?;
            }
            Operation::SetOpacity { node, opacity } => {
                set_style(&self.node(*node)?, "opacity", &opacity.to_string())?;
            }
            Operation::SetVisibility { node, visibility } => {
                set_style(
                    &self.node(*node)?,
                    "visibility",
                    if matches!(visibility, whisker_protocol::Visibility::Visible) {
                        "visible"
                    } else {
                        "hidden"
                    },
                )?;
            }
            Operation::SetZOrder { node, z_order } => {
                set_style(&self.node(*node)?, "z-index", &z_order.to_string())?;
            }
            Operation::SetText { node, content } => {
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
                apply_text_metrics_style(&text, &content.payload)?;
                set_style(&text, "color", &color(&content.paint.foreground))?;
                text.set_text_content(Some(&content.payload.text));
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
            Operation::SetProperty { .. }
            | Operation::ClearProperty { .. }
            | Operation::SetEventMask { .. }
            | Operation::SetPointerCapture { .. }
            | Operation::ReleasePointerCapture { .. }
            | Operation::InvokeCommand { .. } => {}
        }
        Ok(())
    }

    fn node(&self, node: NodeId) -> Result<web_sys::Element, WebHostError> {
        self.nodes
            .get(&node)
            .cloned()
            .ok_or_else(|| WebHostError(format!("DOM projection is missing node {}", node.get())))
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
            self.parents.remove(&node);
            self.layouts.remove(&node);
            self.text_nodes.remove(&node);
        }
    }
}

fn position_text(
    element: &web_sys::Element,
    rect: whisker_protocol::LayoutRect,
) -> Result<(), WebHostError> {
    set_style(element, "left", &px(rect.x))?;
    set_style(element, "top", &px(rect.y))?;
    set_style(element, "width", &px(rect.width))?;
    set_style(element, "height", &px(rect.height))?;
    set_style(element, "overflow", "hidden")
}

impl FrameSink for DomFrameSink {
    type Error = WebHostError;

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        let mut next = self.projection.clone();
        let result = next
            .apply(packet)
            .map_err(|error| WebHostError(error.to_string()))?;
        if matches!(result, ApplyResult::Accepted { .. }) {
            self.apply(packet)?;
            self.projection = next;
        }
        Ok(result)
    }
}

fn apply_text_metrics_style(
    element: &web_sys::Element,
    text: &whisker_protocol::TextMeasurePayload,
) -> Result<(), WebHostError> {
    let families = text
        .style
        .font_families
        .iter()
        .map(|family| match family {
            MeasureFontFamily::System => "system-ui".to_string(),
            MeasureFontFamily::Named(name) => format!("{name:?}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    set_style(element, "font-family", &families)?;
    set_style(element, "font-size", &px(text.style.font_size))?;
    set_style(element, "font-weight", &text.style.font_weight.to_string())?;
    set_style(
        element,
        "font-style",
        match text.style.font_style {
            MeasureFontStyle::Normal => "normal",
            MeasureFontStyle::Italic => "italic",
            MeasureFontStyle::Oblique => "oblique",
        },
    )?;
    set_style(
        element,
        "line-height",
        &match text.style.line_height {
            MeasureLineHeight::Normal => "normal".to_string(),
            MeasureLineHeight::LogicalPixels(value) => px(value),
        },
    )?;
    set_style(element, "letter-spacing", &px(text.style.letter_spacing))?;
    set_style(
        element,
        "white-space",
        if text.wrap == MeasureTextWrap::NoWrap {
            "nowrap"
        } else {
            "normal"
        },
    )?;
    set_style(
        element,
        "direction",
        match text.direction {
            MeasureTextDirection::Auto => "initial",
            MeasureTextDirection::LeftToRight => "ltr",
            MeasureTextDirection::RightToLeft => "rtl",
        },
    )?;
    set_style(element, "overflow-wrap", "anywhere")?;
    Ok(())
}

fn set_style(element: &web_sys::Element, property: &str, value: &str) -> Result<(), WebHostError> {
    let html = element
        .dyn_ref::<web_sys::HtmlElement>()
        .ok_or_else(|| WebHostError("Whisker DOM node is not an HtmlElement".into()))?;
    html.style()
        .set_property(property, value)
        .map_err(|error| js_error(&format!("set CSS property {property}"), error))
}

fn px(value: f32) -> String {
    format!("{value}px")
}

fn length(value: PaintLengthPercentage) -> String {
    if value.fraction == 0.0 {
        px(value.length)
    } else {
        format!("calc({}px + {}%)", value.length, value.fraction * 100.0)
    }
}

fn color(value: &PaintColor) -> String {
    match value {
        PaintColor::Named(name) => name.clone(),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => format!("rgba({red}, {green}, {blue}, {alpha})"),
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => format!("hsla({hue_degrees}, {saturation}%, {lightness}%, {alpha})"),
    }
}

fn border_style(value: BorderLineStyle) -> &'static str {
    match value {
        BorderLineStyle::None => "none",
        BorderLineStyle::Hidden => "hidden",
        BorderLineStyle::Solid => "solid",
        BorderLineStyle::Dashed => "dashed",
        BorderLineStyle::Dotted => "dotted",
        BorderLineStyle::Double => "double",
        BorderLineStyle::Groove => "groove",
        BorderLineStyle::Ridge => "ridge",
        BorderLineStyle::Inset => "inset",
        BorderLineStyle::Outset => "outset",
    }
}

fn js_error(context: &str, value: wasm_bindgen::JsValue) -> WebHostError {
    WebHostError(format!(
        "{context}: {}",
        value.as_string().unwrap_or_else(|| format!("{value:?}"))
    ))
}
