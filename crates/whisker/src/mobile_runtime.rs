//! Platform-neutral runtime behind the Android and iOS C entry points.

use std::ffi::{CString, c_void};

use whisker_driver::mobile_abi::*;
use whisker_driver::module::{
    InvokeModuleCallback, MobileModuleHost, ObserveModuleCallback, with_mobile_module_host,
};
use whisker_engine::whisker_protocol::{
    ApplyResult, AvailableSpace, BackgroundAttachment, BackgroundSize, BlendMode, BorderLineStyle,
    ChildPolicy, ClipShape, ElementMeasurement, ElementRegistration, ElementValueKind, FillRule,
    FrameMode, FramePacket, ImageRendering, ImageRepeat, MeasureFontFamily, MeasureFontStyle,
    MeasureLineHeight, MeasureTextWrap, MeasuredSize, MeasurementMetrics, MeasurementPayload,
    MeasurementRequest, MeasurementRequestId, MeasurementResponse, NodeId, Operation, PaintBox,
    PaintColor, PaintImage, PaintLengthPercentage, PaintPosition, PathCommand, PreparedContentId,
    RadialGradientExtent, RadialGradientShape, ResourceCommand, ResourceDimensions, ResourceEvent,
    ResourceFailureCode, ResourceId, ResourceKind, ResourceSource, SurfaceId,
    UnsupportedMeasurementReason, VisualEffects,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{FrameSink, LayoutOptions, MeasurementProvider};
use whisker_runtime::RuntimeWakeHandle;
use whisker_runtime::view::Element;

use crate::{RuntimeInstance, SurfaceRuntime};

pub type RequestFrameCallback = extern "C" fn(*mut c_void);

struct MobileRuntime {
    runtime: RuntimeInstance,
    modules: std::sync::Arc<MobileModuleHost>,
    measurement: MobileMeasurementHost,
    sink: MobileFrameSink,
    resources: MobileResourceHost,
    environment_epoch: u64,
    viewport_epoch: u32,
    viewport: Viewport,
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

/// Mounts Rust, negotiates all element registrations with the Host, then
/// enables measurement and frame production. All callbacks run on the caller.
#[allow(clippy::too_many_arguments)]
pub fn create(
    width: f32,
    height: f32,
    scale: f32,
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
    application: impl FnOnce() -> Element,
) -> *mut c_void {
    #[cfg(target_os = "android")]
    whisker_driver::ensure_mobile_bridge_linked();
    let Some(viewport) = Viewport::new(width, height, scale) else {
        return std::ptr::null_mut();
    };
    let wake_data = request_data as usize;
    let wake = RuntimeWakeHandle::new(move || request_frame(wake_data as *mut c_void));
    let surface = SurfaceRuntime::new(
        SurfaceId::new(1).expect("mobile surface ID is non-zero"),
        viewport.environment(),
    );
    let mut runtime = RuntimeInstance::new(surface, wake);
    let modules = MobileModuleHost::new(module_data, invoke_module, observe_module);
    if let Err(error) = with_mobile_module_host(&modules, || runtime.mount(application)) {
        eprintln!("Whisker mobile mount failed: {error}");
        return std::ptr::null_mut();
    }
    let registrations = runtime.surface().element_registrations();
    let owned_bootstrap = MobileBootstrapOwned::new(&registrations);
    if !bootstrap(bootstrap_data, &owned_bootstrap.value) {
        let _ = with_mobile_module_host(&modules, || runtime.unmount());
        return std::ptr::null_mut();
    }
    let mut mobile = Box::new(MobileRuntime {
        runtime,
        modules,
        measurement: MobileMeasurementHost {
            callback: measure,
            data: measure_data,
        },
        sink: MobileFrameSink {
            present: present_frame,
            data: present_data,
        },
        resources: MobileResourceHost {
            callback: resource_command,
            data: resource_data,
        },
        environment_epoch: 1,
        viewport_epoch: 1,
        viewport,
    });
    if !mobile.drain_resource_commands() {
        let modules = std::sync::Arc::clone(&mobile.modules);
        let _ = with_mobile_module_host(&modules, || mobile.runtime.unmount());
        return std::ptr::null_mut();
    }
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
    let modules = std::sync::Arc::clone(&mobile.modules);
    let frame_result = with_mobile_module_host(&modules, || {
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
    if !mobile.drain_resource_commands() {
        eprintln!("Whisker mobile Host rejected a resource command");
        return true;
    }
    match frame_result {
        Ok(drive) => !drive.needs_frame,
        Err(error) => {
            eprintln!("Whisker mobile frame failed: {error}");
            true
        }
    }
}

/// # Safety
/// `handle` must be live and must not be used after this call.
pub unsafe fn destroy(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    let mut mobile = unsafe { Box::from_raw(handle.cast::<MobileRuntime>()) };
    let modules = std::sync::Arc::clone(&mobile.modules);
    let _ = with_mobile_module_host(&modules, || mobile.runtime.unmount());
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
        surface: SurfaceId::new(1).unwrap(),
        timestamp_ms,
        kind: whisker_engine::whisker_protocol::InputEventKind::Named(name.to_owned()),
        pointer: None,
        target: Some(node),
        detail: unsafe { decode_value(detail) },
    };
    let modules = std::sync::Arc::clone(&mobile.modules);
    with_mobile_module_host(&modules, || mobile.runtime.dispatch_input(&event))
        .map(|value| value.consumed)
        .unwrap_or_else(|error| {
            eprintln!("Whisker mobile event failed: {error}");
            false
        })
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
    let modules = std::sync::Arc::clone(&mobile.modules);
    with_mobile_module_host(&modules, || modules.dispatch_event(module, event, payload))
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
    fn drain_resource_commands(&mut self) -> bool {
        self.runtime
            .surface()
            .take_resource_commands()
            .iter()
            .all(|command| self.resources.send(command))
    }
}

struct MobileResourceHost {
    callback: ResourceCommandCallback,
    data: *mut c_void,
}

impl MobileResourceHost {
    fn send(&self, command: &ResourceCommand) -> bool {
        let empty_string = WhiskerStringRef {
            ptr: std::ptr::null(),
            len: 0,
        };
        let empty_bytes = WhiskerBytesRef {
            ptr: std::ptr::null(),
            len: 0,
        };
        let value = match command {
            ResourceCommand::Load(request) => {
                let (source, identifier, data) = match &request.source {
                    ResourceSource::Url(value) => {
                        (RESOURCE_SOURCE_URL, string_ref(value), empty_bytes)
                    }
                    ResourceSource::BundledAsset(value) => (
                        RESOURCE_SOURCE_BUNDLED_ASSET,
                        string_ref(value),
                        empty_bytes,
                    ),
                    ResourceSource::Bytes { media_type, data } => (
                        RESOURCE_SOURCE_BYTES,
                        string_ref(media_type),
                        WhiskerBytesRef {
                            ptr: data.as_ptr(),
                            len: data.len(),
                        },
                    ),
                };
                MobileResourceCommand {
                    command: RESOURCE_COMMAND_LOAD,
                    kind: encode_resource_kind(request.kind),
                    source,
                    _reserved: 0,
                    resource: request.resource.get(),
                    generation: request.generation,
                    identifier,
                    data,
                }
            }
            ResourceCommand::Release {
                resource,
                generation,
            } => MobileResourceCommand {
                command: RESOURCE_COMMAND_RELEASE,
                kind: 0,
                source: RESOURCE_SOURCE_NONE,
                _reserved: 0,
                resource: resource.get(),
                generation: *generation,
                identifier: empty_string,
                data: empty_bytes,
            },
        };
        (self.callback)(self.data, &value)
    }
}

fn string_ref(value: &str) -> WhiskerStringRef {
    WhiskerStringRef {
        ptr: value.as_ptr().cast(),
        len: value.len(),
    }
}

fn encode_resource_kind(kind: ResourceKind) -> u32 {
    match kind {
        ResourceKind::RasterImage => RESOURCE_RASTER_IMAGE,
        ResourceKind::VectorImage => RESOURCE_VECTOR_IMAGE,
        ResourceKind::Font => RESOURCE_FONT,
        ResourceKind::Cursor => RESOURCE_CURSOR,
        ResourceKind::PaintServer => RESOURCE_PAINT_SERVER,
    }
}

fn decode_resource_event(event: &MobileResourceEvent) -> Option<ResourceEvent> {
    let resource = ResourceId::new(event.resource)?;
    match event.status {
        RESOURCE_EVENT_READY => {
            let dimensions = if event.dimensions_mask & RESOURCE_DIMENSIONS_PRESENT != 0 {
                Some(ResourceDimensions {
                    width: event.width,
                    height: event.height,
                    scale: event.scale,
                })
            } else {
                None
            };
            let event = ResourceEvent::Ready {
                resource,
                generation: event.generation,
                dimensions,
            };
            event.validate().ok()?;
            Some(event)
        }
        RESOURCE_EVENT_FAILED => {
            let diagnostic = decode_optional_string(event.diagnostic)?;
            let event = ResourceEvent::Failed {
                resource,
                generation: event.generation,
                code: decode_resource_failure(event.failure_code)?,
                diagnostic,
            };
            event.validate().ok()?;
            Some(event)
        }
        _ => None,
    }
}

fn decode_optional_string(value: WhiskerStringRef) -> Option<Option<String>> {
    if value.len == 0 {
        return Some(None);
    }
    if value.ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr.cast::<u8>(), value.len) };
    Some(Some(std::str::from_utf8(bytes).ok()?.to_owned()))
}

fn decode_resource_failure(value: u32) -> Option<ResourceFailureCode> {
    Some(match value {
        RESOURCE_FAILURE_NOT_FOUND => ResourceFailureCode::NotFound,
        RESOURCE_FAILURE_DENIED => ResourceFailureCode::Denied,
        RESOURCE_FAILURE_NETWORK => ResourceFailureCode::Network,
        RESOURCE_FAILURE_DECODE => ResourceFailureCode::Decode,
        RESOURCE_FAILURE_CANCELLED => ResourceFailureCode::Cancelled,
        RESOURCE_FAILURE_UNSUPPORTED => ResourceFailureCode::Unsupported,
        _ => return None,
    })
}

struct MobileBootstrapOwned {
    value: MobileBootstrap,
    _strings: Vec<CString>,
    _members: Vec<Vec<MobileMemberRegistration>>,
    _registrations: Vec<MobileElementRegistration>,
}

impl MobileBootstrapOwned {
    fn new(source: &[ElementRegistration]) -> Self {
        let mut strings = Vec::new();
        let mut members = Vec::<Vec<MobileMemberRegistration>>::new();
        let mut registrations = Vec::with_capacity(source.len());
        for registration in source {
            let name = push_string(&mut strings, &registration.name);
            let properties = registration
                .properties
                .iter()
                .map(|item| {
                    member_registration(
                        item.property.get(),
                        &item.name,
                        item.value,
                        false,
                        &mut strings,
                    )
                })
                .collect::<Vec<_>>();
            let events = registration
                .events
                .iter()
                .map(|item| {
                    member_registration(
                        item.event.get(),
                        &item.name,
                        item.detail.unwrap_or(ElementValueKind::Null),
                        item.detail.is_some(),
                        &mut strings,
                    )
                })
                .collect::<Vec<_>>();
            let commands = registration
                .commands
                .iter()
                .map(|item| {
                    member_registration(
                        item.command.get(),
                        &item.name,
                        item.arguments,
                        false,
                        &mut strings,
                    )
                })
                .collect::<Vec<_>>();
            members.push(properties);
            let (property_ptr, property_count) = (
                members.last().unwrap().as_ptr(),
                members.last().unwrap().len(),
            );
            members.push(events);
            let (event_ptr, event_count) = (
                members.last().unwrap().as_ptr(),
                members.last().unwrap().len(),
            );
            members.push(commands);
            let commands = members.last().unwrap();
            registrations.push(MobileElementRegistration {
                element_type: registration.element_type.get(),
                child_policy: match registration.child_policy {
                    ChildPolicy::None => 0,
                    ChildPolicy::Elements => 1,
                    ChildPolicy::PlainText => 2,
                },
                measurement: match registration.measurement {
                    ElementMeasurement::None => 0,
                    ElementMeasurement::Text => 1,
                    ElementMeasurement::ReplacedContent => 2,
                    ElementMeasurement::Custom => 3,
                },
                _pad: [0; 2],
                name,
                properties: property_ptr,
                property_count,
                events: event_ptr,
                event_count,
                commands: commands.as_ptr(),
                command_count: commands.len(),
            });
        }
        let value = MobileBootstrap {
            abi_major: MOBILE_ABI_MAJOR,
            abi_minor: MOBILE_ABI_MINOR,
            protocol_major: whisker_engine::whisker_protocol::PROTOCOL_MAJOR,
            protocol_minor: whisker_engine::whisker_protocol::PROTOCOL_MINOR,
            registrations: registrations.as_ptr(),
            registration_count: registrations.len(),
        };
        Self {
            value,
            _strings: strings,
            _members: members,
            _registrations: registrations,
        }
    }
}

fn push_string(strings: &mut Vec<CString>, value: &str) -> WhiskerStringRef {
    let value = CString::new(value).unwrap_or_default();
    let result = WhiskerStringRef {
        ptr: value.as_ptr(),
        len: value.as_bytes().len(),
    };
    strings.push(value);
    result
}

fn empty_string() -> WhiskerStringRef {
    WhiskerStringRef {
        ptr: std::ptr::null(),
        len: 0,
    }
}

fn member_registration(
    id: u32,
    name: &str,
    kind: ElementValueKind,
    optional: bool,
    strings: &mut Vec<CString>,
) -> MobileMemberRegistration {
    MobileMemberRegistration {
        id,
        value_kind: match kind {
            ElementValueKind::Null => 0,
            ElementValueKind::Bool => 1,
            ElementValueKind::Int => 2,
            ElementValueKind::Float => 3,
            ElementValueKind::String => 4,
            ElementValueKind::Bytes => 5,
            ElementValueKind::Array => 6,
            ElementValueKind::Map => 7,
        },
        optional_kind: u8::from(optional),
        _pad: [0; 2],
        name: push_string(strings, name),
    }
}

#[derive(Debug)]
struct MobileMeasureError(&'static str);
impl std::fmt::Display for MobileMeasureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for MobileMeasureError {}

struct MobileMeasurementHost {
    callback: MeasureCallback,
    data: *mut c_void,
}

impl MeasurementProvider for MobileMeasurementHost {
    type Error = MobileMeasureError;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        if requests.iter().any(|request| {
            matches!(
                &request.payload,
                MeasurementPayload::Text(text) if text.style.uses_extended_typography()
            )
        }) {
            return Err(MobileMeasureError(
                "mobile Host does not implement extended text typography",
            ));
        }
        let mut batch = MobileMeasureBatch::new(requests);
        if !(self.callback)(
            self.data,
            batch.requests.as_ptr(),
            batch.requests.len(),
            batch.responses.as_mut_ptr(),
        ) {
            return Err(MobileMeasureError("mobile Host rejected measurement batch"));
        }
        for raw in &batch.responses {
            let Some(request) = requests.iter().find(|item| item.key.get() == raw.key) else {
                return Err(MobileMeasureError(
                    "mobile Host returned an unknown measurement key",
                ));
            };
            if raw.environment_epoch != request.environment_epoch {
                return Err(MobileMeasureError(
                    "mobile Host returned a stale measurement epoch",
                ));
            }
            let make_metrics = || MeasurementMetrics {
                size: MeasuredSize::new(raw.width, raw.height),
                first_baseline: (raw.metrics_mask & 1 != 0).then_some(raw.first_baseline),
                last_baseline: (raw.metrics_mask & 2 != 0).then_some(raw.last_baseline),
                overflow: None,
                prepared_content: (raw.metrics_mask & 4 != 0)
                    .then(|| PreparedContentId::new(raw.prepared_content))
                    .flatten(),
            };
            responses.push(match raw.status {
                MEASURE_READY => MeasurementResponse::Ready {
                    key: request.key,
                    environment_epoch: raw.environment_epoch,
                    metrics: make_metrics(),
                },
                MEASURE_PENDING => MeasurementResponse::Pending {
                    key: request.key,
                    environment_epoch: raw.environment_epoch,
                    request_id: MeasurementRequestId::new(raw.request_id)
                        .ok_or(MobileMeasureError("pending measurement omitted request ID"))?,
                    provisional: (raw.metrics_mask & 8 != 0).then(make_metrics),
                },
                MEASURE_UNSUPPORTED => MeasurementResponse::Unsupported {
                    key: request.key,
                    environment_epoch: raw.environment_epoch,
                    reason: match raw.reason {
                        1 => UnsupportedMeasurementReason::Element,
                        2 => UnsupportedMeasurementReason::PayloadVersion,
                        3 => UnsupportedMeasurementReason::Environment,
                        4 => UnsupportedMeasurementReason::Feature,
                        _ => UnsupportedMeasurementReason::Kind,
                    },
                },
                _ => {
                    return Err(MobileMeasureError(
                        "mobile Host returned an invalid measurement status",
                    ));
                }
            });
        }
        Ok(())
    }
}

struct MobileMeasureBatch {
    _strings: Vec<CString>,
    _bytes: Vec<Vec<u8>>,
    requests: Vec<MobileMeasureRequest>,
    responses: Vec<MobileMeasureResponse>,
}

impl MobileMeasureBatch {
    fn new(source: &[MeasurementRequest]) -> Self {
        let mut strings = Vec::new();
        let mut bytes = Vec::new();
        let mut requests = Vec::with_capacity(source.len());
        let mut responses = Vec::with_capacity(source.len());
        for request in source {
            let mut raw = MobileMeasureRequest {
                key: request.key.get(),
                node: request.node.get(),
                element_type: request.element_type.get(),
                kind: 0,
                environment_epoch: request.environment_epoch,
                known_width: request.constraints.known_dimensions[0].unwrap_or_default(),
                known_height: request.constraints.known_dimensions[1].unwrap_or_default(),
                known_mask: u32::from(request.constraints.known_dimensions[0].is_some())
                    | (u32::from(request.constraints.known_dimensions[1].is_some()) << 1),
                available_width: available_value(request.constraints.available_space[0]),
                available_height: available_value(request.constraints.available_space[1]),
                available_width_kind: available_kind(request.constraints.available_space[0]),
                available_height_kind: available_kind(request.constraints.available_space[1]),
                font_style: 0,
                wrap: 0,
                text: empty_string(),
                locale: empty_string(),
                font_family: empty_string(),
                font_size: 0.0,
                font_weight: 400,
                payload_version: 0,
                line_height: 0.0,
                letter_spacing: 0.0,
                max_lines: 0,
                payload: WhiskerBytesRef {
                    ptr: std::ptr::null(),
                    len: 0,
                },
                intrinsic_width: 0.0,
                intrinsic_height: 0.0,
                intrinsic_mask: 0,
            };
            match &request.payload {
                MeasurementPayload::Text(value) => {
                    raw.kind = MEASURE_TEXT;
                    raw.text = push_string(&mut strings, &value.text);
                    raw.locale = value
                        .locale
                        .as_deref()
                        .map(|value| push_string(&mut strings, value))
                        .unwrap_or_else(empty_string);
                    if let Some(MeasureFontFamily::Named(value)) = value.style.font_families.first()
                    {
                        raw.font_family = push_string(&mut strings, value);
                    }
                    raw.font_size = value.style.font_size;
                    raw.font_weight = value.style.font_weight;
                    raw.font_style = match value.style.font_style {
                        MeasureFontStyle::Normal => 0,
                        MeasureFontStyle::Italic => 1,
                        MeasureFontStyle::Oblique => 2,
                    };
                    raw.wrap = u8::from(matches!(value.wrap, MeasureTextWrap::Wrap));
                    raw.line_height = match value.style.line_height {
                        MeasureLineHeight::Normal => 0.0,
                        MeasureLineHeight::LogicalPixels(value) => value,
                    };
                    raw.letter_spacing = value.style.letter_spacing;
                    raw.max_lines = value.max_lines.unwrap_or(0);
                }
                MeasurementPayload::ReplacedContent(value) => {
                    raw.kind = MEASURE_REPLACED_CONTENT;
                    if let Some(size) = value.intrinsic_size {
                        raw.intrinsic_width = size.width;
                        raw.intrinsic_height = size.height;
                        raw.intrinsic_mask = 3;
                    }
                }
                MeasurementPayload::NativeControl(value) => {
                    raw.kind = MEASURE_NATIVE_CONTROL;
                    raw.payload_version = value.version;
                    raw.payload = push_bytes(&mut bytes, &value.state);
                }
                MeasurementPayload::EmbeddedSurface(value) => {
                    raw.kind = MEASURE_EMBEDDED_SURFACE;
                    if let Some(size) = value.preferred_size {
                        raw.intrinsic_width = size.width;
                        raw.intrinsic_height = size.height;
                        raw.intrinsic_mask = 3;
                    }
                }
                MeasurementPayload::Custom(value) => {
                    raw.kind = MEASURE_CUSTOM;
                    raw.payload_version = value.version;
                    raw.payload = push_bytes(&mut bytes, &value.data);
                }
            }
            responses.push(MobileMeasureResponse {
                key: raw.key,
                environment_epoch: raw.environment_epoch,
                ..MobileMeasureResponse::default()
            });
            requests.push(raw);
        }
        Self {
            _strings: strings,
            _bytes: bytes,
            requests,
            responses,
        }
    }
}

fn push_bytes(storage: &mut Vec<Vec<u8>>, value: &[u8]) -> WhiskerBytesRef {
    let value = value.to_vec();
    let result = WhiskerBytesRef {
        ptr: value.as_ptr(),
        len: value.len(),
    };
    storage.push(value);
    result
}
fn available_kind(value: AvailableSpace) -> u8 {
    match value {
        AvailableSpace::Definite(_) => 0,
        AvailableSpace::MinContent => 1,
        AvailableSpace::MaxContent => 2,
    }
}
fn available_value(value: AvailableSpace) -> f32 {
    match value {
        AvailableSpace::Definite(value) => value,
        _ => 0.0,
    }
}

#[derive(Debug)]
struct MobileFrameError;
impl std::fmt::Display for MobileFrameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("mobile Host rejected a frame")
    }
}
impl std::error::Error for MobileFrameError {}

struct MobileFrameSink {
    present: PresentFrameCallback,
    data: *mut c_void,
}

impl FrameSink for MobileFrameSink {
    type Error = MobileFrameError;
    fn capabilities(&self) -> whisker_engine::whisker_protocol::RenderCapabilities {
        whisker_engine::whisker_protocol::RenderCapabilities::new(
            whisker_engine::whisker_protocol::ProtocolVersion::CURRENT,
            [
                whisker_engine::whisker_protocol::CapabilityEntry {
                    capability:
                        whisker_engine::whisker_protocol::RenderCapability::EllipticalBorderRadius,
                    support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
                },
                whisker_engine::whisker_protocol::CapabilityEntry {
                    capability: whisker_engine::whisker_protocol::RenderCapability::VisualEffects,
                    support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
                },
                whisker_engine::whisker_protocol::CapabilityEntry {
                    capability: whisker_engine::whisker_protocol::RenderCapability::TextEffects,
                    support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
                },
                whisker_engine::whisker_protocol::CapabilityEntry {
                    capability: whisker_engine::whisker_protocol::RenderCapability::LinearGradients,
                    support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
                },
                whisker_engine::whisker_protocol::CapabilityEntry {
                    capability: whisker_engine::whisker_protocol::RenderCapability::RadialGradients,
                    support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
                },
                whisker_engine::whisker_protocol::CapabilityEntry {
                    capability: whisker_engine::whisker_protocol::RenderCapability::ConicGradients,
                    support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
                },
                whisker_engine::whisker_protocol::CapabilityEntry {
                    capability:
                        whisker_engine::whisker_protocol::RenderCapability::BackgroundGeometry,
                    support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
                },
                whisker_engine::whisker_protocol::CapabilityEntry {
                    capability:
                        whisker_engine::whisker_protocol::RenderCapability::BackgroundLayerStacking,
                    support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
                },
                whisker_engine::whisker_protocol::CapabilityEntry {
                    capability:
                        whisker_engine::whisker_protocol::RenderCapability::BackgroundImageResources,
                    support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
                },
            ],
        )
        .expect("mobile capability profile is unique")
    }
    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        let capabilities = self.capabilities();
        if !capabilities.supports_protocol(packet.header.version)
            || capabilities.first_unsupported(packet).is_some()
        {
            return Err(MobileFrameError);
        }
        let owned = MobileFrameOwned::new(packet)?;
        let mut response = MobileApplyResponse::default();
        if !(self.present)(self.data, &owned.value, &mut response) {
            return Err(MobileFrameError);
        }
        match response.status {
            APPLY_ACCEPTED if response.revision == packet.header.target_revision => {
                Ok(ApplyResult::Accepted {
                    revision: response.revision,
                })
            }
            APPLY_NEED_SNAPSHOT => Ok(ApplyResult::NeedSnapshot {
                receiver_revision: response.revision,
            }),
            _ => Err(MobileFrameError),
        }
    }
}

struct MobileFrameOwned {
    value: MobileFrame,
    _arena: RawValueArena,
    _layouts: Vec<Box<MobileLayoutGeometry>>,
    _paints: Vec<Box<MobileBoxPaint>>,
    _box_shadows: Vec<Box<[MobileBoxShadow]>>,
    _clip_insets: Vec<Box<MobileClipInset>>,
    _clip_circles: Vec<Box<MobileClipCircle>>,
    _clip_ellipses: Vec<Box<MobileClipEllipse>>,
    _path_commands: Vec<Box<[MobilePathCommand]>>,
    _clip_path_commands: Vec<Box<MobileClipPathCommands>>,
    _clip_paths: Vec<Box<MobileClipPath>>,
    _gradient_stops: Vec<Box<[MobileGradientStop]>>,
    _radial_gradients: Vec<Box<MobileRadialGradient>>,
    _conic_gradients: Vec<Box<MobileConicGradient>>,
    _background_layers: Vec<Box<[MobileBackgroundLayer]>>,
    _background_resource_ids: Vec<Box<u64>>,
    _texts: Vec<Box<MobileText>>,
    _transforms: Vec<Box<[f32; 16]>>,
    _values: Vec<Box<WhiskerValueRaw>>,
    _strings: Vec<CString>,
    _operations: Vec<MobileOperation>,
}

fn empty_mobile_operation() -> MobileOperation {
    MobileOperation {
        tag: 0,
        flags: 0,
        node: 0,
        parent: 0,
        child: 0,
        index: 0,
        member: 0,
        integer: 0,
        scalar: 0.0,
        wide: 0,
        payload: std::ptr::null(),
        payload_count: 0,
    }
}

impl MobileFrameOwned {
    fn new(packet: &FramePacket) -> Result<Self, MobileFrameError> {
        let mut arena = RawValueArena::default();
        let mut layouts = Vec::<Box<MobileLayoutGeometry>>::new();
        let mut paints = Vec::<Box<MobileBoxPaint>>::new();
        let mut box_shadows = Vec::<Box<[MobileBoxShadow]>>::new();
        let mut clip_insets = Vec::<Box<MobileClipInset>>::new();
        let mut clip_circles = Vec::<Box<MobileClipCircle>>::new();
        let mut clip_ellipses = Vec::<Box<MobileClipEllipse>>::new();
        let mut path_commands = Vec::<Box<[MobilePathCommand]>>::new();
        let mut clip_path_commands = Vec::<Box<MobileClipPathCommands>>::new();
        let mut clip_paths = Vec::<Box<MobileClipPath>>::new();
        let mut gradient_stops = Vec::<Box<[MobileGradientStop]>>::new();
        let mut radial_gradients = Vec::<Box<MobileRadialGradient>>::new();
        let mut conic_gradients = Vec::<Box<MobileConicGradient>>::new();
        let mut background_layers = Vec::<Box<[MobileBackgroundLayer]>>::new();
        let mut background_resource_ids = Vec::<Box<u64>>::new();
        let mut texts = Vec::<Box<MobileText>>::new();
        let mut transforms = Vec::<Box<[f32; 16]>>::new();
        let mut values = Vec::<Box<WhiskerValueRaw>>::new();
        let mut strings = Vec::new();
        let mut operations = Vec::with_capacity(packet.operations.len() * 2);
        for operation in &packet.operations {
            let mut raw = empty_mobile_operation();
            match operation {
                Operation::CreateNode { node, element_type } => {
                    raw.tag = OP_CREATE;
                    raw.node = node.get();
                    raw.member = element_type.get();
                }
                Operation::DeleteNode { node } => {
                    raw.tag = OP_DELETE;
                    raw.node = node.get();
                }
                Operation::InsertChild {
                    parent,
                    child,
                    index,
                } => {
                    raw.tag = OP_INSERT;
                    raw.parent = parent.get();
                    raw.child = child.get();
                    raw.index = *index;
                }
                Operation::RemoveChild { parent, child } => {
                    raw.tag = OP_REMOVE;
                    raw.parent = parent.get();
                    raw.child = child.get();
                }
                Operation::MoveChild {
                    parent,
                    child,
                    index,
                } => {
                    raw.tag = OP_MOVE;
                    raw.parent = parent.get();
                    raw.child = child.get();
                    raw.index = *index;
                }
                Operation::SetLayout { node, geometry } => {
                    raw.tag = OP_LAYOUT;
                    raw.node = node.get();
                    layouts.push(Box::new(MobileLayoutGeometry {
                        border: mobile_rect(geometry.border_box),
                        content: mobile_rect(geometry.content_box),
                    }));
                    raw.payload = layouts.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::SetBoxPaint { node, paint } => {
                    raw.tag = OP_PAINT;
                    raw.node = node.get();
                    paints.push(Box::new(mobile_paint(paint, &mut strings)));
                    raw.payload = paints.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::SetBackgroundLayers { node, layers } => {
                    raw.tag = OP_BACKGROUND_LAYERS;
                    raw.node = node.get();
                    if !layers.is_empty() {
                        let mut mobile_layers = Vec::with_capacity(layers.len());
                        for layer in layers {
                            if layer.attachment != BackgroundAttachment::Scroll
                                || layer.blend_mode != BlendMode::Normal
                            {
                                return Err(MobileFrameError);
                            }
                            let empty = MobileLengthPercentage::default();
                            let (size_kind, size_width, size_height) = match layer.size {
                                BackgroundSize::Auto => (BACKGROUND_SIZE_AUTO, empty, empty),
                                BackgroundSize::Cover => (BACKGROUND_SIZE_COVER, empty, empty),
                                BackgroundSize::Contain => (BACKGROUND_SIZE_CONTAIN, empty, empty),
                                BackgroundSize::Explicit {
                                    width: Some(width),
                                    height: Some(height),
                                } => (
                                    BACKGROUND_SIZE_EXPLICIT,
                                    mobile_length(width),
                                    mobile_length(height),
                                ),
                                BackgroundSize::Explicit {
                                    width: Some(width),
                                    height: None,
                                } => (BACKGROUND_SIZE_WIDTH, mobile_length(width), empty),
                                BackgroundSize::Explicit {
                                    width: None,
                                    height: Some(height),
                                } => (BACKGROUND_SIZE_HEIGHT, empty, mobile_length(height)),
                                BackgroundSize::Explicit {
                                    width: None,
                                    height: None,
                                } => (BACKGROUND_SIZE_AUTO, empty, empty),
                            };
                            let repeat_x = mobile_background_repeat(layer.repeat_x);
                            let repeat_y = mobile_background_repeat(layer.repeat_y);
                            let origin = match layer.origin {
                                PaintBox::Border => BACKGROUND_BOX_BORDER,
                                PaintBox::Padding => BACKGROUND_BOX_PADDING,
                                PaintBox::Content => BACKGROUND_BOX_CONTENT,
                                _ => return Err(MobileFrameError),
                            };
                            let clip = match layer.clip {
                                PaintBox::Border => BACKGROUND_BOX_BORDER,
                                PaintBox::Padding => BACKGROUND_BOX_PADDING,
                                PaintBox::Content => BACKGROUND_BOX_CONTENT,
                                PaintBox::BorderArea => BACKGROUND_BOX_BORDER_AREA,
                                _ => return Err(MobileFrameError),
                            };
                            let image = match &layer.image {
                                PaintImage::Resource(resource) => {
                                    background_resource_ids.push(Box::new(resource.get()));
                                    MobileBackgroundImage {
                                        kind: BACKGROUND_RESOURCE,
                                        scalar: 0.0,
                                        payload: background_resource_ids.last().unwrap().as_ref()
                                            as *const _
                                            as *const c_void,
                                        payload_count: 1,
                                    }
                                }
                                PaintImage::LinearGradient {
                                    angle_degrees,
                                    repeating: false,
                                    stops,
                                } => {
                                    let stops = mobile_gradient_stops(stops, &mut strings)?;
                                    gradient_stops.push(stops);
                                    let stops = gradient_stops.last().unwrap();
                                    MobileBackgroundImage {
                                        kind: BACKGROUND_LINEAR,
                                        scalar: *angle_degrees,
                                        payload: stops.as_ptr().cast(),
                                        payload_count: stops.len(),
                                    }
                                }
                                PaintImage::RadialGradient {
                                    shape: RadialGradientShape::Ellipse,
                                    extent: RadialGradientExtent::Explicit,
                                    center,
                                    radii: Some((radius_x, radius_y)),
                                    repeating: false,
                                    stops,
                                } => {
                                    let stops = mobile_gradient_stops(stops, &mut strings)?;
                                    gradient_stops.push(stops);
                                    let stops = gradient_stops.last().unwrap();
                                    radial_gradients.push(Box::new(MobileRadialGradient {
                                        center_x: mobile_coordinate(center.x),
                                        center_y: mobile_coordinate(center.y),
                                        radius_x: mobile_length(*radius_x),
                                        radius_y: mobile_length(*radius_y),
                                        stops: stops.as_ptr(),
                                        stop_count: stops.len(),
                                    }));
                                    MobileBackgroundImage {
                                        kind: BACKGROUND_RADIAL,
                                        scalar: 0.0,
                                        payload: radial_gradients.last().unwrap().as_ref()
                                            as *const _
                                            as *const c_void,
                                        payload_count: 1,
                                    }
                                }
                                PaintImage::ConicGradient {
                                    from_degrees,
                                    center,
                                    repeating: false,
                                    stops,
                                } if stops.iter().all(|stop| {
                                    stop.position.is_some_and(|position| position.length == 0.0)
                                }) =>
                                {
                                    let stops = mobile_gradient_stops(stops, &mut strings)?;
                                    gradient_stops.push(stops);
                                    let stops = gradient_stops.last().unwrap();
                                    conic_gradients.push(Box::new(MobileConicGradient {
                                        center_x: mobile_coordinate(center.x),
                                        center_y: mobile_coordinate(center.y),
                                        stops: stops.as_ptr(),
                                        stop_count: stops.len(),
                                    }));
                                    MobileBackgroundImage {
                                        kind: BACKGROUND_CONIC,
                                        scalar: *from_degrees,
                                        payload: conic_gradients.last().unwrap().as_ref()
                                            as *const _
                                            as *const c_void,
                                        payload_count: 1,
                                    }
                                }
                                _ => return Err(MobileFrameError),
                            };
                            mobile_layers.push(MobileBackgroundLayer {
                                image,
                                position_x: mobile_coordinate(layer.position.x),
                                position_y: mobile_coordinate(layer.position.y),
                                size_width,
                                size_height,
                                size_kind,
                                repeat_x,
                                repeat_y,
                                origin,
                                clip,
                                attachment: BACKGROUND_ATTACHMENT_SCROLL,
                                blend_mode: BACKGROUND_BLEND_NORMAL,
                            });
                        }
                        background_layers.push(mobile_layers.into_boxed_slice());
                        let layers = background_layers.last().unwrap();
                        raw.payload = layers.as_ptr().cast();
                        raw.payload_count = layers.len();
                    }
                }
                Operation::SetVisualEffects { node, effects } => {
                    let mut remainder = effects.clone();
                    remainder.box_shadows.clear();
                    remainder.clip_path = None;
                    remainder.backdrop_blur = None;
                    remainder.image_rendering = ImageRendering::Auto;
                    if remainder != VisualEffects::default() {
                        return Err(MobileFrameError);
                    }
                    if !matches!(
                        effects.image_rendering,
                        ImageRendering::Auto
                            | ImageRendering::Pixelated
                            | ImageRendering::CrispEdges
                    ) {
                        return Err(MobileFrameError);
                    }
                    raw.tag = OP_BOX_SHADOWS;
                    raw.node = node.get();
                    box_shadows.push(
                        effects
                            .box_shadows
                            .iter()
                            .map(|shadow| MobileBoxShadow {
                                offset_x: shadow.offset_x,
                                offset_y: shadow.offset_y,
                                blur_radius: shadow.blur_radius,
                                spread_radius: shadow.spread_radius,
                                color: mobile_color(&shadow.color, &mut strings),
                                inset: u8::from(shadow.inset),
                                _pad: [0; 7],
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    let shadows = box_shadows.last().unwrap();
                    raw.payload = if shadows.is_empty() {
                        std::ptr::null()
                    } else {
                        shadows.as_ptr().cast()
                    };
                    raw.payload_count = shadows.len();
                    operations.push(raw);

                    raw = empty_mobile_operation();
                    raw.tag = OP_CLIP_PATH;
                    raw.node = node.get();
                    if let Some((reference_box, shape)) = effects.clip_path.as_ref() {
                        let reference_box = match reference_box {
                            PaintBox::Border => BACKGROUND_BOX_BORDER,
                            PaintBox::Padding => BACKGROUND_BOX_PADDING,
                            PaintBox::Content => BACKGROUND_BOX_CONTENT,
                            _ => return Err(MobileFrameError),
                        };
                        let (shape_kind, payload) = match shape {
                            ClipShape::Inset { edges, radii } => {
                                clip_insets.push(Box::new(MobileClipInset {
                                    edges: [
                                        mobile_coordinate(edges.top),
                                        mobile_coordinate(edges.right),
                                        mobile_coordinate(edges.bottom),
                                        mobile_coordinate(edges.left),
                                    ],
                                    radii_horizontal: [
                                        mobile_length(radii.top_left.horizontal),
                                        mobile_length(radii.top_right.horizontal),
                                        mobile_length(radii.bottom_right.horizontal),
                                        mobile_length(radii.bottom_left.horizontal),
                                    ],
                                    radii_vertical: [
                                        mobile_length(radii.top_left.vertical),
                                        mobile_length(radii.top_right.vertical),
                                        mobile_length(radii.bottom_right.vertical),
                                        mobile_length(radii.bottom_left.vertical),
                                    ],
                                }));
                                (
                                    CLIP_SHAPE_INSET,
                                    clip_insets.last().unwrap().as_ref() as *const _
                                        as *const c_void,
                                )
                            }
                            ClipShape::Circle { radius, center } => {
                                clip_circles.push(Box::new(MobileClipCircle {
                                    radius: mobile_length(*radius),
                                    center_x: mobile_coordinate(center.x),
                                    center_y: mobile_coordinate(center.y),
                                }));
                                (
                                    CLIP_SHAPE_CIRCLE,
                                    clip_circles.last().unwrap().as_ref() as *const _
                                        as *const c_void,
                                )
                            }
                            ClipShape::Ellipse {
                                radius_x,
                                radius_y,
                                center,
                            } => {
                                clip_ellipses.push(Box::new(MobileClipEllipse {
                                    radius_x: mobile_length(*radius_x),
                                    radius_y: mobile_length(*radius_y),
                                    center_x: mobile_coordinate(center.x),
                                    center_y: mobile_coordinate(center.y),
                                }));
                                (
                                    CLIP_SHAPE_ELLIPSE,
                                    clip_ellipses.last().unwrap().as_ref() as *const _
                                        as *const c_void,
                                )
                            }
                            ClipShape::Path {
                                fill_rule,
                                commands,
                            } => {
                                path_commands.push(
                                    commands
                                        .iter()
                                        .map(mobile_path_command)
                                        .collect::<Vec<_>>()
                                        .into_boxed_slice(),
                                );
                                let commands = path_commands.last().unwrap();
                                clip_path_commands.push(Box::new(MobileClipPathCommands {
                                    fill_rule: match fill_rule {
                                        FillRule::NonZero => FILL_RULE_NON_ZERO,
                                        FillRule::EvenOdd => FILL_RULE_EVEN_ODD,
                                    },
                                    _reserved: 0,
                                    commands: commands.as_ptr(),
                                    command_count: commands.len(),
                                }));
                                (
                                    CLIP_SHAPE_PATH,
                                    clip_path_commands.last().unwrap().as_ref() as *const _
                                        as *const c_void,
                                )
                            }
                            _ => return Err(MobileFrameError),
                        };
                        clip_paths.push(Box::new(MobileClipPath {
                            reference_box,
                            shape_kind,
                            payload,
                            payload_count: 1,
                        }));
                        raw.payload =
                            clip_paths.last().unwrap().as_ref() as *const _ as *const c_void;
                        raw.payload_count = 1;
                    }
                    operations.push(raw);

                    raw = empty_mobile_operation();
                    raw.tag = OP_BACKDROP_BLUR;
                    raw.node = node.get();
                    raw.scalar = effects.backdrop_blur.unwrap_or(0.0);
                    operations.push(raw);

                    raw = empty_mobile_operation();
                    raw.tag = OP_IMAGE_RENDERING;
                    raw.node = node.get();
                    raw.integer = match effects.image_rendering {
                        ImageRendering::Auto => IMAGE_RENDERING_AUTO,
                        ImageRendering::Pixelated => IMAGE_RENDERING_PIXELATED,
                        ImageRendering::CrispEdges => IMAGE_RENDERING_CRISP_EDGES,
                        _ => unreachable!("unsupported image-rendering rejected above"),
                    };
                }
                Operation::SetClip { node, clip } => {
                    raw.tag = OP_CLIP;
                    raw.node = node.get();
                    raw.flags = u32::from(matches!(
                        clip.horizontal,
                        whisker_engine::whisker_protocol::OverflowClip::Hidden
                    )) | (u32::from(matches!(
                        clip.vertical,
                        whisker_engine::whisker_protocol::OverflowClip::Hidden
                    )) << 1);
                }
                Operation::SetTransform { node, transform } => {
                    raw.tag = OP_TRANSFORM;
                    raw.node = node.get();
                    transforms.push(Box::new(transform.0));
                    raw.payload = transforms.last().unwrap().as_ref().as_ptr().cast();
                    raw.payload_count = 16;
                }
                Operation::SetOpacity { node, opacity } => {
                    raw.tag = OP_OPACITY;
                    raw.node = node.get();
                    raw.scalar = *opacity;
                }
                Operation::SetVisibility { node, visibility } => {
                    raw.tag = OP_VISIBILITY;
                    raw.node = node.get();
                    raw.integer = i32::from(matches!(
                        visibility,
                        whisker_engine::whisker_protocol::Visibility::Visible
                    ));
                }
                Operation::SetZOrder { node, z_order } => {
                    raw.tag = OP_Z_ORDER;
                    raw.node = node.get();
                    raw.integer = *z_order;
                }
                Operation::SetText { node, content } => {
                    if content.paint.decoration.lines.overline
                        || (content.paint.decoration.lines.underline
                            && content.paint.decoration.lines.line_through)
                        || !matches!(
                            content.paint.decoration.thickness,
                            whisker_engine::whisker_protocol::TextDecorationThickness::Auto
                        )
                        || content.paint.shadows.len() > 1
                        || content.payload.style.uses_extended_typography()
                    {
                        return Err(MobileFrameError);
                    }
                    raw.tag = OP_TEXT;
                    raw.node = node.get();
                    let shadow = content.paint.shadows.first();
                    let shadow_color = match shadow {
                        Some(value) => mobile_color(&value.color, &mut strings),
                        None => mobile_color(&PaintColor::default(), &mut strings),
                    };
                    let decoration_color =
                        mobile_color(&content.paint.decoration.color, &mut strings);
                    texts.push(Box::new(MobileText {
                        text: push_string(&mut strings, &content.payload.text),
                        font_size: content.payload.style.font_size,
                        font_weight: content.payload.style.font_weight,
                        font_style: match content.payload.style.font_style {
                            MeasureFontStyle::Normal => 0,
                            MeasureFontStyle::Italic => 1,
                            MeasureFontStyle::Oblique => 2,
                        },
                        wrap: u8::from(matches!(content.payload.wrap, MeasureTextWrap::Wrap)),
                        max_lines: content.payload.max_lines.unwrap_or(0),
                        line_height: match content.payload.style.line_height {
                            MeasureLineHeight::Normal => 0.0,
                            MeasureLineHeight::LogicalPixels(value) => value,
                        },
                        letter_spacing: content.payload.style.letter_spacing,
                        color: mobile_color(&content.paint.foreground, &mut strings),
                        shadow_offset_x: shadow.map_or(0.0, |value| value.offset_x),
                        shadow_offset_y: shadow.map_or(0.0, |value| value.offset_y),
                        shadow_blur_radius: shadow.map_or(0.0, |value| value.blur_radius),
                        shadow_flags: u32::from(shadow.is_some()),
                        shadow_color,
                        decoration_flags: u32::from(content.paint.decoration.lines.underline)
                            | (u32::from(content.paint.decoration.lines.line_through) << 1),
                        decoration_style: match content.paint.decoration.style {
                            whisker_engine::whisker_protocol::TextDecorationStyle::Solid => 0,
                            whisker_engine::whisker_protocol::TextDecorationStyle::Double => 1,
                            whisker_engine::whisker_protocol::TextDecorationStyle::Dotted => 2,
                            whisker_engine::whisker_protocol::TextDecorationStyle::Dashed => 3,
                            whisker_engine::whisker_protocol::TextDecorationStyle::Wavy => 4,
                        },
                        decoration_color,
                        prepared_content: content.prepared_content.map_or(0, |value| value.get()),
                    }));
                    raw.payload = texts.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::SetProperty {
                    node,
                    property,
                    value,
                } => {
                    raw.tag = OP_PROPERTY;
                    raw.node = node.get();
                    raw.member = property.get();
                    values.push(Box::new(arena.encode(value)));
                    raw.payload = values.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::ClearProperty { node, property } => {
                    raw.tag = OP_CLEAR_PROPERTY;
                    raw.node = node.get();
                    raw.member = property.get();
                }
                Operation::SetEventMask { node, event_mask } => {
                    raw.tag = OP_EVENT_MASK;
                    raw.node = node.get();
                    raw.wide = *event_mask;
                }
                Operation::SetHitTest { node, behavior } => {
                    raw.tag = OP_HIT_TEST;
                    raw.node = node.get();
                    raw.integer = match behavior {
                        whisker_engine::whisker_protocol::HitTestBehavior::Auto => 0,
                        whisker_engine::whisker_protocol::HitTestBehavior::None => 1,
                        whisker_engine::whisker_protocol::HitTestBehavior::BoxOnly => 2,
                        whisker_engine::whisker_protocol::HitTestBehavior::DescendantsOnly => 3,
                    };
                }
                Operation::SetPointerCapture { node, pointer } => {
                    raw.tag = OP_CAPTURE;
                    raw.node = node.get();
                    raw.wide = pointer.get();
                }
                Operation::ReleasePointerCapture { node, pointer } => {
                    raw.tag = OP_RELEASE_CAPTURE;
                    raw.node = node.get();
                    raw.wide = pointer.get();
                }
                Operation::InvokeCommand {
                    node,
                    command,
                    arguments,
                    result,
                } => {
                    raw.tag = OP_COMMAND;
                    raw.node = node.get();
                    raw.member = command.get();
                    raw.wide = result.map_or(0, |value| value.get());
                    values.push(Box::new(arena.encode(arguments)));
                    raw.payload = values.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::SetImage { .. } | Operation::SetCursor { .. } => {
                    return Err(MobileFrameError);
                }
            }
            operations.push(raw);
        }
        let value = MobileFrame {
            abi_major: MOBILE_ABI_MAJOR,
            abi_minor: MOBILE_ABI_MINOR,
            protocol_major: packet.header.version.major,
            protocol_minor: packet.header.version.minor,
            mode: match packet.header.mode {
                FrameMode::Snapshot => FRAME_SNAPSHOT,
                FrameMode::Delta => FRAME_DELTA,
            },
            _pad: [0; 7],
            surface: packet.header.surface.get(),
            scene_epoch: packet.header.scene_epoch,
            viewport_epoch: packet.header.viewport_epoch,
            frame_id: packet.header.frame_id,
            base_revision: packet.header.base_revision,
            target_revision: packet.header.target_revision,
            operations: operations.as_ptr(),
            operation_count: operations.len(),
        };
        Ok(Self {
            value,
            _arena: arena,
            _layouts: layouts,
            _paints: paints,
            _box_shadows: box_shadows,
            _clip_insets: clip_insets,
            _clip_circles: clip_circles,
            _clip_ellipses: clip_ellipses,
            _path_commands: path_commands,
            _clip_path_commands: clip_path_commands,
            _clip_paths: clip_paths,
            _gradient_stops: gradient_stops,
            _radial_gradients: radial_gradients,
            _conic_gradients: conic_gradients,
            _background_layers: background_layers,
            _background_resource_ids: background_resource_ids,
            _texts: texts,
            _transforms: transforms,
            _values: values,
            _strings: strings,
            _operations: operations,
        })
    }
}

fn mobile_rect(value: whisker_engine::whisker_protocol::LayoutRect) -> MobileRect {
    MobileRect {
        x: value.x,
        y: value.y,
        width: value.width,
        height: value.height,
    }
}
fn mobile_coordinate(
    value: whisker_engine::whisker_protocol::PaintCoordinate,
) -> MobileLengthPercentage {
    MobileLengthPercentage {
        length: value.length,
        fraction: value.fraction,
    }
}
fn mobile_length(value: PaintLengthPercentage) -> MobileLengthPercentage {
    MobileLengthPercentage {
        length: value.length,
        fraction: value.fraction,
    }
}

fn mobile_path_point(
    points: &mut [MobileLengthPercentage; 6],
    offset: usize,
    value: PaintPosition,
) {
    points[offset] = mobile_coordinate(value.x);
    points[offset + 1] = mobile_coordinate(value.y);
}

fn mobile_path_command(value: &PathCommand) -> MobilePathCommand {
    let mut result = MobilePathCommand::default();
    match value {
        PathCommand::MoveTo(point) => {
            result.kind = PATH_MOVE_TO;
            mobile_path_point(&mut result.points, 0, *point);
        }
        PathCommand::LineTo(point) => {
            result.kind = PATH_LINE_TO;
            mobile_path_point(&mut result.points, 0, *point);
        }
        PathCommand::QuadraticTo { control, end } => {
            result.kind = PATH_QUADRATIC_TO;
            mobile_path_point(&mut result.points, 0, *control);
            mobile_path_point(&mut result.points, 2, *end);
        }
        PathCommand::CubicTo {
            control_1,
            control_2,
            end,
        } => {
            result.kind = PATH_CUBIC_TO;
            mobile_path_point(&mut result.points, 0, *control_1);
            mobile_path_point(&mut result.points, 2, *control_2);
            mobile_path_point(&mut result.points, 4, *end);
        }
        PathCommand::Close => result.kind = PATH_CLOSE,
    }
    result
}

fn mobile_background_repeat(value: ImageRepeat) -> u32 {
    match value {
        ImageRepeat::Repeat => BACKGROUND_REPEAT_REPEAT,
        ImageRepeat::NoRepeat => BACKGROUND_REPEAT_NO_REPEAT,
        ImageRepeat::Space => BACKGROUND_REPEAT_SPACE,
        ImageRepeat::Round => BACKGROUND_REPEAT_ROUND,
    }
}
fn mobile_gradient_stops(
    stops: &[whisker_engine::whisker_protocol::GradientStop],
    strings: &mut Vec<CString>,
) -> Result<Box<[MobileGradientStop]>, MobileFrameError> {
    if !(2..=4_096).contains(&stops.len()) {
        return Err(MobileFrameError);
    }
    stops
        .iter()
        .map(|stop| {
            let position = stop.position.ok_or(MobileFrameError)?;
            Ok(MobileGradientStop {
                color: mobile_color(&stop.color, strings),
                position: mobile_coordinate(position),
            })
        })
        .collect::<Result<Vec<_>, MobileFrameError>>()
        .map(Vec::into_boxed_slice)
}
fn mobile_paint(
    value: &whisker_engine::whisker_protocol::BoxPaint,
    strings: &mut Vec<CString>,
) -> MobileBoxPaint {
    MobileBoxPaint {
        background: mobile_color(&value.background_color, strings),
        widths: [
            mobile_length(value.border_widths.top),
            mobile_length(value.border_widths.right),
            mobile_length(value.border_widths.bottom),
            mobile_length(value.border_widths.left),
        ],
        colors: [
            mobile_color(&value.border_colors.top, strings),
            mobile_color(&value.border_colors.right, strings),
            mobile_color(&value.border_colors.bottom, strings),
            mobile_color(&value.border_colors.left, strings),
        ],
        styles: [
            border_style(value.border_styles.top),
            border_style(value.border_styles.right),
            border_style(value.border_styles.bottom),
            border_style(value.border_styles.left),
        ],
        radii_horizontal: [
            mobile_length(value.border_radii.top_left.horizontal),
            mobile_length(value.border_radii.top_right.horizontal),
            mobile_length(value.border_radii.bottom_right.horizontal),
            mobile_length(value.border_radii.bottom_left.horizontal),
        ],
        radii_vertical: [
            mobile_length(value.border_radii.top_left.vertical),
            mobile_length(value.border_radii.top_right.vertical),
            mobile_length(value.border_radii.bottom_right.vertical),
            mobile_length(value.border_radii.bottom_left.vertical),
        ],
    }
}
fn mobile_color(value: &PaintColor, strings: &mut Vec<CString>) -> MobileColor {
    match value {
        PaintColor::Named(name) => MobileColor {
            kind: 0,
            red: 0,
            green: 0,
            blue: 0,
            _pad: 0,
            alpha: 1.0,
            name: push_string(strings, name),
        },
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => MobileColor {
            kind: 1,
            red: *red,
            green: *green,
            blue: *blue,
            _pad: 0,
            alpha: *alpha,
            name: empty_string(),
        },
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => {
            let (red, green, blue) =
                hsl_to_rgb(*hue_degrees, *saturation / 100.0, *lightness / 100.0);
            MobileColor {
                kind: 1,
                red,
                green,
                blue,
                _pad: 0,
                alpha: *alpha,
                name: empty_string(),
            }
        }
    }
}
fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = hue.rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}
fn border_style(value: BorderLineStyle) -> u32 {
    match value {
        BorderLineStyle::None => 0,
        BorderLineStyle::Hidden => 1,
        BorderLineStyle::Solid => 2,
        BorderLineStyle::Dashed => 3,
        BorderLineStyle::Dotted => 4,
        BorderLineStyle::Double => 5,
        BorderLineStyle::Groove => 6,
        BorderLineStyle::Ridge => 7,
        BorderLineStyle::Inset => 8,
        BorderLineStyle::Outset => 9,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use whisker_engine::whisker_protocol::{
        BackgroundLayer, FrameHeader, GradientStop, PaintCoordinate, PaintPosition,
        ProtocolVersion, TextContent, TextMeasurePayload, TextMeasureStyle, TextPaint, TextShadow,
    };

    fn linear_background(name: &str) -> BackgroundLayer {
        BackgroundLayer {
            image: PaintImage::LinearGradient {
                angle_degrees: 180.0,
                repeating: false,
                stops: [0.0, 1.0]
                    .into_iter()
                    .map(|fraction| GradientStop {
                        color: PaintColor::Named(name.into()),
                        position: Some(PaintCoordinate {
                            length: 0.0,
                            fraction,
                        }),
                    })
                    .collect(),
            },
            position: PaintPosition::default(),
            size: BackgroundSize::Auto,
            repeat_x: ImageRepeat::Repeat,
            repeat_y: ImageRepeat::Repeat,
            origin: PaintBox::Padding,
            clip: PaintBox::Border,
            attachment: BackgroundAttachment::Scroll,
            blend_mode: BlendMode::Normal,
        }
    }

    fn text_with_shadow(shadows: Vec<TextShadow>) -> TextContent {
        TextContent {
            payload: TextMeasurePayload {
                text: "shadow".into(),
                style: TextMeasureStyle::default(),
                locale: None,
                direction: Default::default(),
                wrap: MeasureTextWrap::Wrap,
                max_lines: None,
                overflow: Default::default(),
            },
            paint: TextPaint {
                foreground: PaintColor::Named("black".into()),
                shadows,
                ..TextPaint::default()
            },
            prepared_content: None,
        }
    }

    #[test]
    fn viewport_rejects_invalid_host_metrics() {
        assert!(Viewport::new(320.0, 640.0, 2.0).is_some());
        assert!(Viewport::new(-1.0, 640.0, 2.0).is_none());
        assert!(Viewport::new(320.0, 640.0, 0.0).is_none());
    }
    #[test]
    fn hsla_is_lowered_without_host_string_parsing() {
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), (255, 0, 0));
        assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), (0, 255, 0));
    }

    #[derive(Debug, PartialEq, Eq)]
    struct CapturedResourceCommand {
        command: u32,
        kind: u32,
        source: u32,
        resource: u64,
        generation: u64,
        identifier: Vec<u8>,
        data: Vec<u8>,
    }

    extern "C" fn capture_resource_command(
        data: *mut c_void,
        command: *const MobileResourceCommand,
    ) -> bool {
        let commands = unsafe { &*(data.cast::<RefCell<Vec<CapturedResourceCommand>>>()) };
        let command = unsafe { &*command };
        let identifier = if command.identifier.len == 0 {
            Vec::new()
        } else {
            unsafe {
                std::slice::from_raw_parts(
                    command.identifier.ptr.cast::<u8>(),
                    command.identifier.len,
                )
                .to_vec()
            }
        };
        let bytes = if command.data.len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(command.data.ptr, command.data.len).to_vec() }
        };
        commands.borrow_mut().push(CapturedResourceCommand {
            command: command.command,
            kind: command.kind,
            source: command.source,
            resource: command.resource,
            generation: command.generation,
            identifier,
            data: bytes,
        });
        true
    }

    #[test]
    fn mobile_resource_commands_are_typed_and_borrowed_only_for_the_callback() {
        let captured = RefCell::new(Vec::new());
        let host = MobileResourceHost {
            callback: capture_resource_command,
            data: (&captured as *const RefCell<Vec<CapturedResourceCommand>>)
                .cast_mut()
                .cast(),
        };
        let resource = ResourceId::new(42).unwrap();
        host.send(&ResourceCommand::Load(
            whisker_engine::whisker_protocol::ResourceRequest {
                resource,
                generation: 3,
                kind: ResourceKind::RasterImage,
                source: ResourceSource::Bytes {
                    media_type: "image/png".into(),
                    data: vec![1, 2, 3],
                },
            },
        ));
        host.send(&ResourceCommand::Release {
            resource,
            generation: 3,
        });

        assert_eq!(
            captured.into_inner(),
            vec![
                CapturedResourceCommand {
                    command: RESOURCE_COMMAND_LOAD,
                    kind: RESOURCE_RASTER_IMAGE,
                    source: RESOURCE_SOURCE_BYTES,
                    resource: 42,
                    generation: 3,
                    identifier: b"image/png".to_vec(),
                    data: vec![1, 2, 3],
                },
                CapturedResourceCommand {
                    command: RESOURCE_COMMAND_RELEASE,
                    kind: 0,
                    source: RESOURCE_SOURCE_NONE,
                    resource: 42,
                    generation: 3,
                    identifier: Vec::new(),
                    data: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn mobile_resource_events_decode_without_json() {
        let diagnostic = b"offline";
        let failed = MobileResourceEvent {
            status: RESOURCE_EVENT_FAILED,
            failure_code: RESOURCE_FAILURE_NETWORK,
            resource: 9,
            generation: 2,
            width: 0.0,
            height: 0.0,
            scale: 0.0,
            dimensions_mask: 0,
            diagnostic: WhiskerStringRef {
                ptr: diagnostic.as_ptr().cast(),
                len: diagnostic.len(),
            },
        };
        assert_eq!(
            decode_resource_event(&failed),
            Some(ResourceEvent::Failed {
                resource: ResourceId::new(9).unwrap(),
                generation: 2,
                code: ResourceFailureCode::Network,
                diagnostic: Some("offline".into()),
            })
        );

        let ready = MobileResourceEvent {
            status: RESOURCE_EVENT_READY,
            failure_code: RESOURCE_FAILURE_NONE,
            resource: 9,
            generation: 2,
            width: 20.0,
            height: 10.0,
            scale: 2.0,
            dimensions_mask: RESOURCE_DIMENSIONS_PRESENT,
            diagnostic: WhiskerStringRef {
                ptr: std::ptr::null(),
                len: 0,
            },
        };
        assert_eq!(
            decode_resource_event(&ready),
            Some(ResourceEvent::Ready {
                resource: ResourceId::new(9).unwrap(),
                generation: 2,
                dimensions: Some(ResourceDimensions {
                    width: 20.0,
                    height: 10.0,
                    scale: 2.0,
                }),
            })
        );
    }

    #[test]
    fn mobile_box_paint_preserves_elliptical_radii() {
        let mut paint = whisker_engine::whisker_protocol::BoxPaint::default();
        paint.border_radii.top_left = whisker_engine::whisker_protocol::PaintCornerRadius {
            horizontal: PaintLengthPercentage {
                length: 40.0,
                fraction: 0.0,
            },
            vertical: PaintLengthPercentage {
                length: 10.0,
                fraction: 0.0,
            },
        };
        let raw = mobile_paint(&paint, &mut Vec::new());
        assert_eq!(raw.radii_horizontal[0].length, 40.0);
        assert_eq!(raw.radii_vertical[0].length, 10.0);
    }

    #[test]
    fn mobile_frame_encodes_one_text_shadow_without_string_protocols() {
        let shadow = TextShadow {
            offset_x: 3.0,
            offset_y: -2.0,
            blur_radius: 5.0,
            color: PaintColor::Srgba {
                red: 10,
                green: 20,
                blue: 30,
                alpha: 0.4,
            },
        };
        let packet = FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: SurfaceId::new(1).unwrap(),
                scene_epoch: 1,
                frame_id: 1,
                base_revision: 0,
                target_revision: 1,
                viewport_epoch: 1,
                mode: FrameMode::Snapshot,
            },
            operations: vec![Operation::SetText {
                node: NodeId::new(1).unwrap(),
                content: text_with_shadow(vec![shadow]),
            }],
        };

        let frame = MobileFrameOwned::new(&packet).unwrap();
        let operation = &frame._operations[0];
        assert_eq!(operation.tag, OP_TEXT);
        let text = unsafe { &*operation.payload.cast::<MobileText>() };
        assert_eq!(text.shadow_flags, 1);
        assert_eq!(text.shadow_offset_x, 3.0);
        assert_eq!(text.shadow_offset_y, -2.0);
        assert_eq!(text.shadow_blur_radius, 5.0);
        assert_eq!(text.shadow_color.kind, 1);
        assert_eq!(text.shadow_color.red, 10);
        assert_eq!(text.shadow_color.green, 20);
        assert_eq!(text.shadow_color.blue, 30);
        assert_eq!(text.shadow_color.alpha, 0.4);
    }

    #[test]
    fn mobile_frame_rejects_multiple_text_shadows() {
        let shadow = TextShadow {
            offset_x: 0.0,
            offset_y: 1.0,
            blur_radius: 0.0,
            color: PaintColor::default(),
        };
        let packet = FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: SurfaceId::new(1).unwrap(),
                scene_epoch: 1,
                frame_id: 1,
                base_revision: 0,
                target_revision: 1,
                viewport_epoch: 1,
                mode: FrameMode::Snapshot,
            },
            operations: vec![Operation::SetText {
                node: NodeId::new(1).unwrap(),
                content: text_with_shadow(vec![shadow.clone(), shadow]),
            }],
        };

        assert!(MobileFrameOwned::new(&packet).is_err());
    }

    #[test]
    fn mobile_frame_exposes_background_layers_as_one_contiguous_slice() {
        let node = NodeId::new(1).unwrap();
        let resource = whisker_engine::whisker_protocol::ResourceId::new(u64::MAX - 1).unwrap();
        let mut resource_background = linear_background("transparent");
        resource_background.image = PaintImage::Resource(resource);
        let packet = FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: SurfaceId::new(1).unwrap(),
                scene_epoch: 1,
                frame_id: 1,
                base_revision: 0,
                target_revision: 1,
                viewport_epoch: 1,
                mode: FrameMode::Snapshot,
            },
            operations: vec![Operation::SetBackgroundLayers {
                node,
                layers: vec![
                    linear_background("red"),
                    linear_background("blue"),
                    resource_background,
                ],
            }],
        };
        let frame = MobileFrameOwned::new(&packet).unwrap();
        let operation = &frame._operations[0];
        assert_eq!(operation.tag, OP_BACKGROUND_LAYERS);
        assert_eq!(operation.payload_count, 3);
        let layers = unsafe {
            std::slice::from_raw_parts(
                operation.payload.cast::<MobileBackgroundLayer>(),
                operation.payload_count,
            )
        };
        assert_eq!(layers[0].image.kind, BACKGROUND_LINEAR);
        assert_eq!(layers[1].image.kind, BACKGROUND_LINEAR);
        assert_eq!(layers[2].image.kind, BACKGROUND_RESOURCE);
        assert_ne!(layers[0].image.payload, layers[1].image.payload);
        assert_eq!(layers[2].image.payload_count, 1);
        assert_eq!(
            unsafe { *layers[2].image.payload.cast::<u64>() },
            resource.get()
        );
    }

    #[test]
    fn mobile_frame_exposes_box_shadows_as_one_contiguous_slice() {
        let node = NodeId::new(1).unwrap();
        let packet = FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: SurfaceId::new(1).unwrap(),
                scene_epoch: 1,
                frame_id: 1,
                base_revision: 0,
                target_revision: 1,
                viewport_epoch: 1,
                mode: FrameMode::Snapshot,
            },
            operations: vec![
                Operation::SetVisualEffects {
                    node,
                    effects: VisualEffects {
                        box_shadows: vec![
                            whisker_engine::whisker_protocol::BoxShadow {
                                offset_x: 4.0,
                                offset_y: 5.0,
                                blur_radius: 0.0,
                                spread_radius: 2.0,
                                color: PaintColor::Named("black".into()),
                                inset: false,
                            },
                            whisker_engine::whisker_protocol::BoxShadow {
                                offset_x: -1.0,
                                offset_y: 3.0,
                                blur_radius: 6.0,
                                spread_radius: -2.0,
                                color: PaintColor::Srgba {
                                    red: 10,
                                    green: 20,
                                    blue: 30,
                                    alpha: 0.5,
                                },
                                inset: true,
                            },
                        ],
                        backdrop_blur: Some(7.0),
                        image_rendering: ImageRendering::Pixelated,
                        ..Default::default()
                    },
                },
                Operation::SetVisualEffects {
                    node,
                    effects: VisualEffects::default(),
                },
            ],
        };

        let frame = MobileFrameOwned::new(&packet).unwrap();
        let operation = &frame._operations[0];
        assert_eq!(operation.tag, OP_BOX_SHADOWS);
        assert_eq!(operation.payload_count, 2);
        let shadows = unsafe {
            std::slice::from_raw_parts(
                operation.payload.cast::<MobileBoxShadow>(),
                operation.payload_count,
            )
        };
        assert_eq!(shadows[0].offset_x, 4.0);
        assert_eq!(shadows[0].spread_radius, 2.0);
        assert_eq!(shadows[0].color.kind, 0);
        assert_eq!(shadows[1].blur_radius, 6.0);
        assert_eq!(shadows[1].inset, 1);
        assert_eq!(shadows[1].color.kind, 1);
        assert_eq!(shadows[1].color.alpha, 0.5);
        assert_eq!(frame._operations[1].tag, OP_CLIP_PATH);
        assert_eq!(frame._operations[1].payload_count, 0);
        assert!(frame._operations[1].payload.is_null());
        assert_eq!(frame._operations[2].tag, OP_BACKDROP_BLUR);
        assert_eq!(frame._operations[2].scalar, 7.0);
        assert_eq!(frame._operations[3].tag, OP_IMAGE_RENDERING);
        assert_eq!(frame._operations[3].integer, IMAGE_RENDERING_PIXELATED);
        assert_eq!(frame._operations[4].tag, OP_BOX_SHADOWS);
        assert_eq!(frame._operations[4].payload_count, 0);
        assert!(frame._operations[4].payload.is_null());
        assert_eq!(frame._operations[5].tag, OP_CLIP_PATH);
        assert_eq!(frame._operations[5].payload_count, 0);
        assert!(frame._operations[5].payload.is_null());
        assert_eq!(frame._operations[6].tag, OP_BACKDROP_BLUR);
        assert_eq!(frame._operations[6].scalar, 0.0);
        assert_eq!(frame._operations[7].tag, OP_IMAGE_RENDERING);
        assert_eq!(frame._operations[7].integer, IMAGE_RENDERING_AUTO);
    }

    #[test]
    fn mobile_frame_exposes_rounded_inset_clip_path_as_typed_payload() {
        use whisker_engine::whisker_protocol::{PaintCornerRadius, PaintCorners, PaintEdges};

        let node = NodeId::new(1).unwrap();
        let coordinate = PaintCoordinate {
            length: 2.0,
            fraction: 0.1,
        };
        let radius = PaintCornerRadius::circular(PaintLengthPercentage {
            length: 4.0,
            fraction: 0.2,
        });
        let packet = FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: SurfaceId::new(1).unwrap(),
                scene_epoch: 1,
                frame_id: 1,
                base_revision: 0,
                target_revision: 1,
                viewport_epoch: 1,
                mode: FrameMode::Snapshot,
            },
            operations: vec![Operation::SetVisualEffects {
                node,
                effects: VisualEffects {
                    clip_path: Some((
                        PaintBox::Padding,
                        ClipShape::Inset {
                            edges: PaintEdges {
                                top: coordinate,
                                right: coordinate,
                                bottom: coordinate,
                                left: coordinate,
                            },
                            radii: PaintCorners {
                                top_left: radius,
                                top_right: radius,
                                bottom_right: radius,
                                bottom_left: radius,
                            },
                        },
                    )),
                    ..Default::default()
                },
            }],
        };

        let frame = MobileFrameOwned::new(&packet).unwrap();
        assert_eq!(frame._operations.len(), 4);
        assert_eq!(frame._operations[0].tag, OP_BOX_SHADOWS);
        let operation = &frame._operations[1];
        assert_eq!(operation.tag, OP_CLIP_PATH);
        assert_eq!(operation.payload_count, 1);
        let clip = unsafe { &*operation.payload.cast::<MobileClipPath>() };
        assert_eq!(clip.reference_box, BACKGROUND_BOX_PADDING);
        assert_eq!(clip.shape_kind, CLIP_SHAPE_INSET);
        assert_eq!(clip.payload_count, 1);
        let inset = unsafe { &*clip.payload.cast::<MobileClipInset>() };
        assert_eq!(inset.edges[0].length, 2.0);
        assert_eq!(inset.edges[0].fraction, 0.1);
        assert_eq!(inset.radii_horizontal[0].length, 4.0);
        assert_eq!(inset.radii_vertical[0].fraction, 0.2);
        assert_eq!(frame._operations[2].tag, OP_BACKDROP_BLUR);
        assert_eq!(frame._operations[3].tag, OP_IMAGE_RENDERING);
    }

    #[test]
    fn mobile_frame_exposes_path_commands_and_fill_rule_as_typed_payload() {
        let position = |x, y| PaintPosition {
            x: PaintCoordinate {
                length: x,
                fraction: 0.0,
            },
            y: PaintCoordinate {
                length: y,
                fraction: 0.0,
            },
        };
        let packet = FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: SurfaceId::new(1).unwrap(),
                scene_epoch: 1,
                frame_id: 1,
                base_revision: 0,
                target_revision: 1,
                viewport_epoch: 1,
                mode: FrameMode::Snapshot,
            },
            operations: vec![Operation::SetVisualEffects {
                node: NodeId::new(1).unwrap(),
                effects: VisualEffects {
                    clip_path: Some((
                        PaintBox::Border,
                        ClipShape::Path {
                            fill_rule: FillRule::EvenOdd,
                            commands: vec![
                                PathCommand::MoveTo(position(1.0, 2.0)),
                                PathCommand::QuadraticTo {
                                    control: position(3.0, 4.0),
                                    end: position(5.0, 6.0),
                                },
                                PathCommand::CubicTo {
                                    control_1: position(7.0, 8.0),
                                    control_2: position(9.0, 10.0),
                                    end: position(11.0, 12.0),
                                },
                                PathCommand::Close,
                            ],
                        },
                    )),
                    ..Default::default()
                },
            }],
        };

        let frame = MobileFrameOwned::new(&packet).unwrap();
        let operation = &frame._operations[1];
        let clip = unsafe { &*operation.payload.cast::<MobileClipPath>() };
        assert_eq!(clip.shape_kind, CLIP_SHAPE_PATH);
        let path = unsafe { &*clip.payload.cast::<MobileClipPathCommands>() };
        assert_eq!(path.fill_rule, FILL_RULE_EVEN_ODD);
        let commands = unsafe { std::slice::from_raw_parts(path.commands, path.command_count) };
        assert_eq!(commands.len(), 4);
        assert_eq!(commands[0].kind, PATH_MOVE_TO);
        assert_eq!(commands[0].points[0].length, 1.0);
        assert_eq!(commands[1].kind, PATH_QUADRATIC_TO);
        assert_eq!(commands[1].points[2].length, 5.0);
        assert_eq!(commands[2].kind, PATH_CUBIC_TO);
        assert_eq!(commands[2].points[5].length, 12.0);
        assert_eq!(commands[3].kind, PATH_CLOSE);
    }

    #[test]
    fn mobile_frame_preserves_every_intrinsic_background_size_kind() {
        let mut auto = linear_background("red");
        auto.position.x.length = 3.0;
        auto.repeat_x = ImageRepeat::NoRepeat;
        let mut cover = linear_background("green");
        cover.size = BackgroundSize::Cover;
        let mut contain = linear_background("blue");
        contain.size = BackgroundSize::Contain;
        let mut width = linear_background("yellow");
        width.size = BackgroundSize::Explicit {
            width: Some(PaintLengthPercentage {
                length: 60.0,
                fraction: 0.0,
            }),
            height: None,
        };
        let mut height = linear_background("black");
        height.size = BackgroundSize::Explicit {
            width: None,
            height: Some(PaintLengthPercentage {
                length: 30.0,
                fraction: 0.0,
            }),
        };
        let packet = FramePacket {
            header: FrameHeader {
                version: ProtocolVersion::CURRENT,
                surface: SurfaceId::new(1).unwrap(),
                scene_epoch: 1,
                frame_id: 1,
                base_revision: 0,
                target_revision: 1,
                viewport_epoch: 1,
                mode: FrameMode::Snapshot,
            },
            operations: vec![Operation::SetBackgroundLayers {
                node: NodeId::new(1).unwrap(),
                layers: vec![auto, cover, contain, width, height],
            }],
        };

        let frame = MobileFrameOwned::new(&packet).unwrap();
        let operation = &frame._operations[0];
        let layers = unsafe {
            std::slice::from_raw_parts(
                operation.payload.cast::<MobileBackgroundLayer>(),
                operation.payload_count,
            )
        };
        assert_eq!(layers[0].size_kind, BACKGROUND_SIZE_AUTO);
        assert_eq!(layers[0].position_x.length, 3.0);
        assert_eq!(layers[0].repeat_x, BACKGROUND_REPEAT_NO_REPEAT);
        assert_eq!(layers[1].size_kind, BACKGROUND_SIZE_COVER);
        assert_eq!(layers[2].size_kind, BACKGROUND_SIZE_CONTAIN);
        assert_eq!(layers[3].size_kind, BACKGROUND_SIZE_WIDTH);
        assert_eq!(layers[3].size_width.length, 60.0);
        assert_eq!(layers[4].size_kind, BACKGROUND_SIZE_HEIGHT);
        assert_eq!(layers[4].size_height.length, 30.0);
    }
}
