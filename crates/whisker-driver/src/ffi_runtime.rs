//! Safe retained runtime behind the Android and iOS C entry points.

#[cfg(target_os = "android")]
use std::ffi::CString;
use std::ffi::c_void;

use crate::abi::*;
use crate::ffi_module::module_host;
use crate::value_codec::{RawValueArena, decode_value};
use whisker_driver_sys::{InvokeModuleCallback, ObserveModuleCallback};
use whisker_engine::whisker_protocol::{
    ApplyResult, AvailableSpace, BackgroundAttachment, BackgroundSize, BlendMode, BorderLineStyle,
    ChildPolicy, ClipShape, ElementMeasurement, ElementRegistration, ElementValueKind, FillRule,
    FrameMode, FramePacket, ImageRendering, ImageRepeat, InputEvent, InputEventKind, InputPoint,
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection,
    MeasureTextOverflow, MeasureTextWordBreak, MeasureTextWrap, MeasuredSize, MeasurementMetrics,
    MeasurementPayload, MeasurementRequest, MeasurementRequestId, MeasurementResponse, NodeId,
    Operation, PaintBox, PaintColor, PaintImage, PaintLengthPercentage, PaintPosition, PathCommand,
    PointerId, PointerInput, PointerKind, PreparedContentId, RadialGradientExtent,
    RadialGradientShape, RenderCapabilities, ResourceCommand, ResourceDimensions, ResourceEvent,
    ResourceFailureCode, ResourceId, ResourceKind, ResourceSource, SurfaceId, TextContent,
    TextMeasurePayload, UnsupportedMeasurementReason, VisualEffects, WhiskerValue,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{FrameSink, LayoutOptions, MeasurementProvider};
use whisker_runtime::module::{
    ModuleHost, RustModuleRuntime, linked_rust_modules, with_module_host,
};
use whisker_runtime::view::Element;
use whisker_runtime::{ElementRegistry, RuntimeInstance, RuntimeWakeHandle, SurfaceRuntime};

mod frame;
#[cfg(feature = "host-conformance")]
#[doc(hidden)]
pub mod host_conformance;
mod measurement;
mod paragraph;
mod paragraph_response;
mod resource;

use frame::MobileFrameSink;
#[cfg(test)]
use frame::{MobileFrameOwned, hsl_to_rgb, mobile_paint};
#[cfg(test)]
use measurement::MobileMeasureBatch;
use measurement::MobileMeasurementHost;
use resource::{MobileBootstrapOwned, MobileResourceHost, decode_resource_event};

#[cfg(target_os = "android")]
fn mobile_error(message: impl std::fmt::Display) {
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(
            priority: std::os::raw::c_int,
            tag: *const std::os::raw::c_char,
            text: *const std::os::raw::c_char,
        ) -> std::os::raw::c_int;
    }
    const ANDROID_LOG_ERROR: std::os::raw::c_int = 6;
    let tag = c"WhiskerRust";
    let Ok(text) = CString::new(message.to_string()) else {
        return;
    };
    unsafe {
        __android_log_write(ANDROID_LOG_ERROR, tag.as_ptr(), text.as_ptr());
    }
}

#[cfg(not(target_os = "android"))]
fn mobile_error(message: impl std::fmt::Display) {
    eprintln!("{message}");
}

pub use whisker_driver_sys::RequestFrameCallback;

struct MobileRuntime {
    runtime: RuntimeInstance,
    hot_reload: MobileHotReload,
    modules: std::rc::Rc<ModuleHost>,
    rust_modules: RustModuleRuntime,
    measurement: MobileMeasurementHost,
    sink: MobileFrameSink,
    resources: MobileResourceHost,
    environment_epoch: u64,
    viewport_epoch: u32,
    viewport: Viewport,
    retrying_failed_frame: bool,
}

#[cfg(feature = "hot-reload")]
type MobileHotReload = whisker_dev_runtime::NativeHotReload;

#[cfg(not(feature = "hot-reload"))]
struct MobileHotReload;

#[cfg(not(feature = "hot-reload"))]
impl MobileHotReload {
    fn new(
        _wake: RuntimeWakeHandle,
        _application: fn() -> Element,
        _application_hash: fn() -> u64,
    ) -> Self {
        Self
    }

    fn apply(&mut self, _runtime: &mut RuntimeInstance) -> Result<bool, String> {
        Ok(false)
    }

    fn record_frame(&self, _timing: Option<whisker_runtime::FrameTiming>) {}
}

#[derive(Clone, Copy, PartialEq)]
struct Viewport {
    width: f32,
    height: f32,
    scale: f32,
}

impl Viewport {
    fn new(width: f32, height: f32, scale: f32) -> Option<Self> {
        (width.is_finite()
            && width >= 0.0
            && height.is_finite()
            && height >= 0.0
            && scale.is_finite()
            && scale > 0.0)
            .then_some(Self {
                width,
                height,
                scale,
            })
    }
    fn environment(self) -> StyleEnvironment {
        StyleEnvironment::new(self.width, self.height, self.scale, 14.0)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum MobileCapabilityError {
    Abi { host_major: u16, host_minor: u16 },
    Masks(whisker_engine::whisker_protocol::InvalidCapabilityMasks),
    Protocol(whisker_engine::whisker_protocol::CapabilityNegotiationError),
}

impl std::fmt::Display for MobileCapabilityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Abi {
                host_major,
                host_minor,
            } => write!(
                formatter,
                "incompatible mobile ABI: Rust {MOBILE_ABI_MAJOR}.{MOBILE_ABI_MINOR}, Host {host_major}.{host_minor}"
            ),
            Self::Masks(error) => error.fmt(formatter),
            Self::Protocol(error) => error.fmt(formatter),
        }
    }
}

fn decode_host_capabilities(
    raw: &MobileHostCapabilities,
) -> Result<RenderCapabilities, MobileCapabilityError> {
    if raw.abi_major != MOBILE_ABI_MAJOR || raw.abi_minor < MOBILE_ABI_MINOR {
        return Err(MobileCapabilityError::Abi {
            host_major: raw.abi_major,
            host_minor: raw.abi_minor,
        });
    }
    RenderCapabilities::from_masks(
        whisker_engine::whisker_protocol::ProtocolVersion {
            major: raw.protocol_major,
            minor: raw.protocol_minor,
        },
        raw.native,
        raw.emulated,
    )
    .map_err(MobileCapabilityError::Masks)?
    .negotiate(whisker_engine::whisker_protocol::ProtocolVersion::CURRENT)
    .map_err(MobileCapabilityError::Protocol)
}

/// Mounts Rust, negotiates all element registrations with the Host, then
/// enables measurement and frame production. All callbacks run on the caller.
///
/// # Safety
///
/// `capabilities` must point to a readable [`MobileHostCapabilities`] for the
/// duration of this call. Callback data pointers must satisfy the contracts of
/// their corresponding callbacks for the lifetime of the returned runtime.
#[allow(clippy::too_many_arguments)]
pub unsafe fn create(
    width: f32,
    height: f32,
    scale: f32,
    capabilities: *const MobileHostCapabilities,
    request_frame: RequestFrameCallback,
    request_data: *mut c_void,
    bootstrap: BootstrapCallback,
    bootstrap_data: *mut c_void,
    measure: MeasureCallback,
    measure_data: *mut c_void,
    present_frame: PresentFrameCallback,
    present_data: *mut c_void,
    resource_command: ResourceCommandCallback,
    resource_data: *mut c_void,
    invoke_module: InvokeModuleCallback,
    observe_module: ObserveModuleCallback,
    module_data: *mut c_void,
    application: fn() -> Element,
    application_hash: fn() -> u64,
) -> *mut c_void {
    #[cfg(target_os = "android")]
    crate::ensure_mobile_bridge_linked();
    let Some(viewport) = Viewport::new(width, height, scale) else {
        return std::ptr::null_mut();
    };
    let Some(capabilities) = (unsafe { capabilities.as_ref() }) else {
        mobile_error("Whisker mobile Host did not provide a capability profile");
        return std::ptr::null_mut();
    };
    let capabilities = match decode_host_capabilities(capabilities) {
        Ok(capabilities) => capabilities,
        Err(error) => {
            mobile_error(format_args!(
                "Whisker mobile capability negotiation failed: {error}"
            ));
            return std::ptr::null_mut();
        }
    };
    let wake_data = request_data as usize;
    let wake = RuntimeWakeHandle::new(move || request_frame(wake_data as *mut c_void));
    let registry = match ElementRegistry::standard_with_linked_providers() {
        Ok(registry) => registry,
        Err(error) => {
            mobile_error(format_args!(
                "Whisker mobile element bootstrap failed: {error}"
            ));
            return std::ptr::null_mut();
        }
    };
    let surface = SurfaceRuntime::with_element_registry_and_protocol(
        SurfaceId::new(1).expect("mobile surface ID is non-zero"),
        viewport.environment(),
        registry,
        capabilities.protocol(),
    );
    let mut runtime = RuntimeInstance::new(surface, wake.clone());
    let rust_modules = match RustModuleRuntime::with_fallback(
        linked_rust_modules(),
        move || request_frame(wake_data as *mut c_void),
        module_host(module_data, invoke_module, observe_module),
    ) {
        Ok(modules) => modules,
        Err(error) => {
            mobile_error(format_args!(
                "Whisker mobile module bootstrap failed: {error}"
            ));
            return std::ptr::null_mut();
        }
    };
    let modules = std::rc::Rc::clone(rust_modules.host());
    if let Err(error) = with_module_host(&modules, || runtime.mount(application)) {
        mobile_error(format_args!("Whisker mobile mount failed: {error}"));
        return std::ptr::null_mut();
    }
    let registrations = runtime.surface().element_registrations();
    let owned_bootstrap = MobileBootstrapOwned::new(&registrations);
    if !bootstrap(bootstrap_data, &owned_bootstrap.value) {
        let _ = with_module_host(&modules, || runtime.unmount());
        return std::ptr::null_mut();
    }
    let mut mobile = Box::new(MobileRuntime {
        runtime,
        hot_reload: MobileHotReload::new(wake, application, application_hash),
        modules,
        rust_modules,
        measurement: MobileMeasurementHost {
            prepared: std::collections::HashMap::new(),
            callback: measure,
            data: measure_data,
        },
        sink: MobileFrameSink {
            present: present_frame,
            data: present_data,
            capabilities,
        },
        resources: MobileResourceHost {
            callback: resource_command,
            data: resource_data,
        },
        environment_epoch: 1,
        viewport_epoch: 1,
        viewport,
        retrying_failed_frame: false,
    });
    mobile.drain_resource_commands();
    request_frame(request_data);
    Box::into_raw(mobile).cast()
}

/// # Safety
/// `handle` must be a live pointer returned by [`create`] on this UI thread.
pub unsafe fn tick(
    handle: *mut c_void,
    timestamp_ms: f64,
    width: f32,
    height: f32,
    scale: f32,
) -> bool {
    let Some(viewport) = Viewport::new(width, height, scale) else {
        return true;
    };
    let Some(mobile) = (unsafe { handle.cast::<MobileRuntime>().as_mut() }) else {
        return true;
    };
    if mobile.viewport != viewport {
        mobile.viewport = viewport;
        mobile.environment_epoch = mobile.environment_epoch.saturating_add(1);
        mobile.viewport_epoch = mobile.viewport_epoch.saturating_add(1);
    }
    let modules = std::rc::Rc::clone(&mobile.modules);
    if let Err(error) = with_module_host(&modules, || mobile.hot_reload.apply(&mut mobile.runtime))
    {
        mobile_error(format_args!("Whisker mobile hot reload failed: {error}"));
    }
    if let Err(error) = mobile.rust_modules.dispatch_pending_events(&mobile.runtime) {
        mobile_error(format_args!("Whisker mobile module event failed: {error}"));
    }
    let frame_result = with_module_host(&modules, || {
        mobile.runtime.drive_frame(
            timestamp_ms,
            viewport.environment(),
            mobile.environment_epoch,
            mobile.viewport_epoch,
            &mut mobile.measurement,
            &mut mobile.sink,
            LayoutOptions::default(),
        )
    });
    mobile.drain_resource_commands();
    match frame_result {
        Ok(drive) => {
            mobile.hot_reload.record_frame(drive.timing);
            mobile.retrying_failed_frame = false;
            !drive.needs_frame
        }
        Err(error) => {
            mobile_error(format_args!("Whisker mobile frame failed: {error}"));
            if mobile.retrying_failed_frame {
                true
            } else {
                mobile.retrying_failed_frame = true;
                false
            }
        }
    }
}

/// Suspends one mounted runtime while retaining its application and scene.
///
/// # Safety
/// `handle` must be a live pointer returned by [`create`] on this UI thread.
pub unsafe fn pause(handle: *mut c_void) -> bool {
    let Some(mobile) = (unsafe { handle.cast::<MobileRuntime>().as_mut() }) else {
        return false;
    };
    let modules = std::rc::Rc::clone(&mobile.modules);
    with_module_host(&modules, || mobile.runtime.pause())
        .map(|()| true)
        .unwrap_or_else(|error| {
            mobile_error(format_args!("Whisker mobile pause failed: {error}"));
            false
        })
}

/// Resumes one retained runtime and schedules its deferred work.
///
/// # Safety
/// `handle` must be a live pointer returned by [`create`] on this UI thread.
pub unsafe fn resume(handle: *mut c_void) -> bool {
    let Some(mobile) = (unsafe { handle.cast::<MobileRuntime>().as_mut() }) else {
        return false;
    };
    let modules = std::rc::Rc::clone(&mobile.modules);
    with_module_host(&modules, || mobile.runtime.resume())
        .map(|()| true)
        .unwrap_or_else(|error| {
            mobile_error(format_args!("Whisker mobile resume failed: {error}"));
            false
        })
}

/// # Safety
/// `handle` must be live and must not be used after this call.
pub unsafe fn destroy(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    let mut mobile = unsafe { Box::from_raw(handle.cast::<MobileRuntime>()) };
    let modules = std::rc::Rc::clone(&mobile.modules);
    let _ = with_module_host(&modules, || mobile.runtime.unmount());
}

/// # Safety
/// All borrowed values must remain valid for this call.
pub unsafe fn dispatch_event(
    handle: *mut c_void,
    timestamp_ms: f64,
    node: u64,
    name: *const u8,
    name_len: usize,
    detail: *const WhiskerValueRaw,
) -> bool {
    let Some(mobile) = (unsafe { handle.cast::<MobileRuntime>().as_ref() }) else {
        return false;
    };
    let Some(node) = NodeId::new(node) else {
        return false;
    };
    if name.is_null() || name_len == 0 || !timestamp_ms.is_finite() {
        return false;
    }
    let Ok(name) = std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) })
    else {
        return false;
    };
    let event = whisker_engine::whisker_protocol::InputEvent {
        presentation_revision: None,
        surface: SurfaceId::new(1).unwrap(),
        timestamp_ms,
        kind: whisker_engine::whisker_protocol::InputEventKind::Named(name.to_owned()),
        pointer: None,
        target: Some(node),
        detail: unsafe { decode_value(detail) },
    };
    let modules = std::rc::Rc::clone(&mobile.modules);
    with_module_host(&modules, || mobile.runtime.dispatch_input(&event))
        .map(|value| value.consumed)
        .unwrap_or_else(|error| {
            eprintln!("Whisker mobile event failed: {error}");
            false
        })
}

/// Dispatches one Host-normalized pointer event. The target intentionally
/// remains empty so the retained Rust scene performs hit testing and pointer
/// capture resolution consistently across Hosts.
///
/// # Safety
/// `handle` must identify a live mobile runtime for the duration of this call.
#[allow(clippy::too_many_arguments)]
pub unsafe fn dispatch_pointer(
    handle: *mut c_void,
    timestamp_ms: f64,
    event: u32,
    pointer_id: u64,
    pointer_kind: u32,
    x: f32,
    y: f32,
    buttons: u32,
    changed_button: i16,
    scroll_nodes: *const u64,
    scroll_offsets: *const f32,
    scroll_count: usize,
) -> bool {
    let Some(mobile) = (unsafe { handle.cast::<MobileRuntime>().as_ref() }) else {
        return false;
    };
    let Some(event) = mobile_pointer_event(
        timestamp_ms,
        event,
        pointer_id,
        pointer_kind,
        x,
        y,
        buttons,
        changed_button,
    ) else {
        return false;
    };
    let Some(presentation) =
        (unsafe { scroll_presentation(scroll_nodes, scroll_offsets, scroll_count) })
    else {
        return false;
    };
    let modules = std::rc::Rc::clone(&mobile.modules);
    with_module_host(&modules, || {
        mobile
            .runtime
            .dispatch_input_with_presentation(&event, &presentation)
    })
    .map(|value| value.consumed)
    .unwrap_or_else(|error| {
        eprintln!("Whisker mobile pointer input failed: {error}");
        false
    })
}

/// Dispatches one Host-normalized wheel at a surface position, with deltas in logical pixels
/// positive to the right and down, and returns whether a catching listener took it. `scroller` is
/// the node of the innermost native scroll container under the pointer that can still scroll by
/// the delta, or 0.
///
/// # Safety
/// `handle` must identify a live mobile runtime for the duration of this call, and the scroll
/// arrays must hold `scroll_count` nodes and twice as many offsets.
#[allow(clippy::too_many_arguments)]
pub unsafe fn dispatch_wheel(
    handle: *mut c_void,
    timestamp_ms: f64,
    x: f32,
    y: f32,
    delta_x: f64,
    delta_y: f64,
    ctrl_key: bool,
    scroller: u64,
    scroll_nodes: *const u64,
    scroll_offsets: *const f32,
    scroll_count: usize,
) -> bool {
    let Some(mobile) = (unsafe { handle.cast::<MobileRuntime>().as_ref() }) else {
        return false;
    };
    let Some(presentation) =
        (unsafe { scroll_presentation(scroll_nodes, scroll_offsets, scroll_count) })
    else {
        return false;
    };
    let mut detail = vec![
        ("deltaX", WhiskerValue::Float(delta_x)),
        ("deltaY", WhiskerValue::Float(delta_y)),
        ("ctrlKey", WhiskerValue::Bool(ctrl_key)),
    ];
    if scroller != 0 {
        detail.push(("scroller", WhiskerValue::Int(scroller as i64)));
    }
    let event = InputEvent {
        presentation_revision: None,
        surface: SurfaceId::new(1).unwrap(),
        timestamp_ms,
        kind: InputEventKind::Wheel,
        pointer: Some(PointerInput {
            id: PointerId::new(1).unwrap(),
            kind: PointerKind::Mouse,
            position: InputPoint { x, y },
            buttons: 0,
            changed_button: -1,
        }),
        target: None,
        detail: WhiskerValue::map(detail),
    };
    let modules = std::rc::Rc::clone(&mobile.modules);
    with_module_host(&modules, || {
        mobile
            .runtime
            .dispatch_input_with_presentation(&event, &presentation)
    })
    .map(|value| value.caught)
    .unwrap_or_else(|error| {
        eprintln!("Whisker mobile wheel input failed: {error}");
        false
    })
}

/// # Safety
/// The arrays must hold `scroll_count` nodes and twice as many offsets.
unsafe fn scroll_presentation(
    scroll_nodes: *const u64,
    scroll_offsets: *const f32,
    scroll_count: usize,
) -> Option<Vec<whisker_engine::whisker_protocol::HostPresentationUpdate>> {
    if scroll_count == 0 {
        return Some(Vec::new());
    }
    if scroll_nodes.is_null() || scroll_offsets.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees both arrays' lengths, and neither pointer is null.
    let nodes = unsafe { std::slice::from_raw_parts(scroll_nodes, scroll_count) };
    // SAFETY: as above, with two offsets per node.
    let offsets =
        unsafe { std::slice::from_raw_parts(scroll_offsets, scroll_count.checked_mul(2)?) };
    nodes
        .iter()
        .enumerate()
        .map(|(index, raw_node)| {
            Some(
                whisker_engine::whisker_protocol::HostPresentationUpdate::ScrollOffset {
                    node: NodeId::new(*raw_node)?,
                    offset: InputPoint {
                        x: offsets[index * 2],
                        y: offsets[index * 2 + 1],
                    },
                },
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn mobile_pointer_event(
    timestamp_ms: f64,
    event: u32,
    pointer_id: u64,
    pointer_kind: u32,
    x: f32,
    y: f32,
    buttons: u32,
    changed_button: i16,
) -> Option<InputEvent> {
    let kind = match event {
        POINTER_DOWN => InputEventKind::PointerDown,
        POINTER_MOVE => InputEventKind::PointerMove,
        POINTER_UP => InputEventKind::PointerUp,
        POINTER_CANCEL => InputEventKind::PointerCancel,
        _ => return None,
    };
    let pointer_kind = match pointer_kind {
        POINTER_MOUSE => PointerKind::Mouse,
        POINTER_TOUCH => PointerKind::Touch,
        POINTER_PEN => PointerKind::Pen,
        POINTER_UNKNOWN => PointerKind::Unknown,
        _ => return None,
    };
    let pointer_id = PointerId::new(pointer_id)?;
    let event = InputEvent {
        presentation_revision: None,
        surface: SurfaceId::new(1).unwrap(),
        timestamp_ms,
        kind,
        pointer: Some(PointerInput {
            id: pointer_id,
            kind: pointer_kind,
            position: InputPoint { x, y },
            buttons,
            changed_button,
        }),
        target: None,
        detail: WhiskerValue::Null,
    };
    event.validate().ok()?;
    Some(event)
}

/// # Safety
/// All borrowed values must remain valid for this call.
pub unsafe fn dispatch_module_event(
    handle: *mut c_void,
    module: *const u8,
    module_len: usize,
    event: *const u8,
    event_len: usize,
    payload: *const WhiskerValueRaw,
) -> bool {
    let Some(mobile) = (unsafe { handle.cast::<MobileRuntime>().as_ref() }) else {
        return false;
    };
    if module.is_null() || module_len == 0 || event.is_null() || event_len == 0 {
        return false;
    }
    let Ok(module) = std::str::from_utf8(unsafe { std::slice::from_raw_parts(module, module_len) })
    else {
        return false;
    };
    let Ok(event) = std::str::from_utf8(unsafe { std::slice::from_raw_parts(event, event_len) })
    else {
        return false;
    };
    let payload = unsafe { decode_value(payload) };
    mobile
        .runtime
        .dispatch_module_event(&mobile.modules, module, event, payload)
        .unwrap_or_else(|error| {
            mobile_error(format_args!("Whisker mobile module event failed: {error}"));
            false
        })
}

/// # Safety
/// `event` and all borrowed members must remain valid for this call.
pub unsafe fn dispatch_resource_event(
    handle: *mut c_void,
    event: *const MobileResourceEvent,
) -> bool {
    let Some(mobile) = (unsafe { handle.cast::<MobileRuntime>().as_ref() }) else {
        return false;
    };
    let Some(event) = (unsafe { event.as_ref() }).and_then(decode_resource_event) else {
        return false;
    };
    mobile
        .runtime
        .dispatch_resource_event(&event)
        .map(|_| true)
        .unwrap_or_else(|error| {
            eprintln!("Whisker mobile resource event failed: {error}");
            false
        })
}

impl MobileRuntime {
    fn drain_resource_commands(&mut self) {
        for command in self.runtime.surface().take_resource_commands() {
            if self.resources.send(&command) {
                continue;
            }
            let ResourceCommand::Load(request) = command else {
                mobile_error("Whisker mobile Host rejected a resource release");
                continue;
            };
            let event = ResourceEvent::Failed {
                resource: request.resource,
                generation: request.generation,
                code: ResourceFailureCode::Unsupported,
                diagnostic: Some("Host rejected the resource command".to_owned()),
            };
            if let Err(error) = self.runtime.dispatch_resource_event(&event) {
                mobile_error(format_args!(
                    "Whisker mobile resource rejection could not be applied: {error}"
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests;
