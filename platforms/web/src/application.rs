use std::cell::{Cell, RefCell};

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::spawn_local;
use whisker::runtime::RuntimeWakeHandle;
use whisker::runtime::module::RustModuleRuntime;
use whisker::{Element, ElementRegistry, RuntimeInstance, SurfaceRuntime, WhiskerModule};
use whisker_engine::LayoutOptions;
use whisker_protocol::{
    InputEvent, InputEventKind, InputPoint, ResourceCommand, ResourceEvent, ResourceId, SurfaceId,
};
use whisker_style::StyleEnvironment;

use crate::input::{
    WebPointerEvent, WebPointerPhase, dispatch_pointer, pointer_kind, stable_pointer_id,
};
use crate::measure::text::DomMeasurementProvider;
use crate::scene::frame_sink::DomFrameSink;
use crate::scene::resource_service::WebResourceService;
use crate::scene::resource_store::WebResourceStore;
use crate::{
    BuiltInElementModule, WebAppConfig, WebError, WebModuleDefinition, js_error, set_style,
};

thread_local! {
    static APPLICATION: RefCell<Option<WebApplication>> = const { RefCell::new(None) };
    static FRAME_SCHEDULED: Cell<bool> = const { Cell::new(false) };
    static RETRYING_FAILED_FRAME: Cell<bool> = const { Cell::new(false) };
    static URGENT_FRAME_SCHEDULED: Cell<bool> = const { Cell::new(false) };
    #[cfg(feature = "hot-reload")]
    static MOUNTED_APPLICATION_HASH: Cell<u64> = const { Cell::new(0) };
}

/// Mounts a Whisker application into the current browser document.
///
/// The generated `gen/web` crate calls this once from its WASM start
/// function. Subsequent work is driven by `requestAnimationFrame`.
pub fn run(config: WebAppConfig, application: fn() -> Element) -> Result<(), WebError> {
    run_with_application_hash(config, application, || 0)
}

/// Mounts a browser application with the generated source hash used by Hot
/// Reload to distinguish root edits from component-only edits.
pub fn run_with_application_hash(
    config: WebAppConfig,
    application: fn() -> Element,
    _application_hash: fn() -> u64,
) -> Result<(), WebError> {
    RETRYING_FAILED_FRAME.set(false);
    #[cfg(feature = "hot-reload")]
    MOUNTED_APPLICATION_HASH.set(_application_hash());
    APPLICATION.with(|slot| {
        if slot.borrow().is_some() {
            return Err(WebError("a Web application is already mounted".into()));
        }
        *slot.borrow_mut() = Some(WebApplication::new(config)?);
        Ok(())
    })?;

    let mount = APPLICATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let application_host = slot.as_mut().expect("application was installed");
        application_host
            .modules
            .with_host(|| application_host.runtime.mount(application))
            .map(|_| ())
            .map_err(|error| WebError(format!("mount Whisker application: {error}")))
    });
    if let Err(error) = mount {
        APPLICATION.with(|slot| *slot.borrow_mut() = None);
        return Err(error);
    }

    let pointer_listeners = APPLICATION.with(|slot| {
        let slot = slot.borrow();
        let application = slot
            .as_ref()
            .ok_or_else(|| WebError("Web application is not mounted".into()))?;
        install_pointer_listeners(&application.root)
    });
    if let Err(error) = pointer_listeners {
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

/// Applies a wasm side-module patch received by the generated browser shell.
/// The side module is compiled asynchronously; component reflection begins
/// only after subsecond commits its indirect-function-table update.
#[cfg(feature = "hot-reload")]
pub fn apply_hot_patch(
    header_json: &str,
    patch_bytes: &[u8],
    application: fn() -> Element,
    application_hash: fn() -> u64,
) -> Result<(), WebError> {
    use js_sys::{Array, Uint8Array};

    let mut table = decode_patch_header(header_json)?;
    let bytes = Uint8Array::from(patch_bytes);
    let parts = Array::new();
    parts.push(&bytes);
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)
        .map_err(|error| js_error("create Hot Reload Blob", error))?;
    let object_url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|error| js_error("create Hot Reload object URL", error))?;
    table.lib = object_url.clone().into();

    unsafe {
        subsecond::apply_patch_with_callback(table, move |patched_functions| {
            let result = reflect_hot_patch(&patched_functions, application, application_hash);
            if let Err(error) = result {
                web_sys::console::error_1(&format!("Whisker Hot Reload: {error}").into());
            }
            let _ = web_sys::Url::revoke_object_url(&object_url);
        })
        .map_err(|error| WebError(format!("apply WebAssembly Hot Reload patch: {error}")))?;
    }
    Ok(())
}

#[cfg(feature = "hot-reload")]
fn reflect_hot_patch(
    patched_functions: &[*const ()],
    application: fn() -> Element,
    application_hash: fn() -> u64,
) -> Result<(), WebError> {
    APPLICATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let host = slot
            .as_mut()
            .ok_or_else(|| WebError("a Web application is not mounted".into()))?;
        host.modules.with_host(|| {
            let current_hash = application_hash();
            let mut remount_root = MOUNTED_APPLICATION_HASH.get() != current_hash;
            if !remount_root {
                let stats = host
                    .runtime
                    .remount_components(patched_functions)
                    .map_err(|error| WebError(error.to_string()))?;
                remount_root = stats.remounted == 0 || stats.layout_changed > 0;
            }
            if remount_root {
                host.runtime
                    .remount_root(application)
                    .map_err(|error| WebError(error.to_string()))?;
            }
            MOUNTED_APPLICATION_HASH.set(current_hash);
            Ok(())
        })
    })?;
    request_frame();
    Ok(())
}

#[cfg(feature = "hot-reload")]
fn decode_patch_header(header_json: &str) -> Result<subsecond::JumpTable, WebError> {
    #[derive(serde::Deserialize)]
    struct Header {
        kind: String,
        table: WireTable,
    }
    #[derive(serde::Deserialize)]
    struct WireTable {
        lib: std::path::PathBuf,
        map: Vec<(u64, u64)>,
        aslr_reference: u64,
        new_base_address: u64,
        ifunc_count: u64,
    }
    let header: Header = serde_json::from_str(header_json)
        .map_err(|error| WebError(format!("decode Hot Reload header: {error}")))?;
    if header.kind != "patch" {
        return Err(WebError(format!(
            "unexpected development message `{}`",
            header.kind
        )));
    }
    let mut map = subsecond_types::AddressMap::default();
    map.extend(header.table.map);
    Ok(subsecond::JumpTable {
        lib: header.table.lib,
        map,
        aslr_reference: header.table.aslr_reference,
        new_base_address: header.table.new_base_address,
        ifunc_count: header.table.ifunc_count,
    })
}

/// Registers or replaces a browser URL for a ready Host resource.
///
/// URL, asset, and byte acquisition remain outside frame transactions. Once
/// acquisition has completed, the provider calls this entry point before a
/// frame references the corresponding [`ResourceId`].
pub fn register_resource_url(resource: ResourceId, url: impl Into<String>) -> Result<(), WebError> {
    APPLICATION.with(|slot| {
        let slot = slot.borrow();
        let application = slot
            .as_ref()
            .ok_or_else(|| WebError("a Web application is not mounted".into()))?;
        application.frames.register_resource_url(resource, url)
    })?;
    request_frame();
    Ok(())
}

/// Sends one out-of-frame protocol resource command to the mounted browser
/// Host and returns the non-stale completion produced by a load.
pub async fn handle_resource_command(
    command: ResourceCommand,
) -> Result<Option<ResourceEvent>, WebError> {
    let resources = APPLICATION.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|application| application.resources.clone())
            .ok_or_else(|| WebError("a Web application is not mounted".into()))
    })?;
    let event = resources.handle(command).await?;
    resources.take_events();
    if event.is_some() {
        request_frame();
    }
    Ok(event)
}

struct WebApplication {
    root: web_sys::Element,
    runtime: RuntimeInstance,
    measurements: DomMeasurementProvider,
    frames: DomFrameSink,
    resources: WebResourceService,
    modules: RustModuleRuntime,
    viewport: (f32, f32, f32),
    viewport_epoch: u32,
    environment_epoch: u64,
}

impl WebApplication {
    fn new(mut config: WebAppConfig) -> Result<Self, WebError> {
        let built_ins = BuiltInElementModule::definition();
        let mut module_definitions = vec![built_ins];
        module_definitions.extend(config.modules.iter().map(|module| module.host.clone()));
        let module_services = module_definitions
            .iter()
            .map(|definition| definition.service_definition().clone());
        let modules = RustModuleRuntime::new(module_services, request_frame)
            .map_err(|error| WebError(format!("bind Web modules: {error}")))?;
        let element_factories = module_definitions
            .into_iter()
            .flat_map(WebModuleDefinition::into_factories)
            .collect::<Vec<_>>();
        let elements = ElementRegistry::standard_builder()
            .register_modules(config.modules.drain(..).map(|module| module.elements))
            .build()
            .map_err(|error| WebError(format!("build element registry: {error}")))?;
        let window = browser_window()?;
        let document = window
            .document()
            .ok_or_else(|| WebError("browser document is unavailable".into()))?;
        document.set_title(&config.title);
        let root = document
            .get_element_by_id(&config.root_id)
            .ok_or_else(|| WebError(format!("missing Web Host root #{}", config.root_id)))?;
        set_style(&root, "position", "relative")?;
        set_style(&root, "width", "100vw")?;
        set_style(&root, "height", "100vh")?;
        set_style(&root, "overflow", "hidden")?;

        let viewport = viewport(&window)?;
        let surface_id = SurfaceId::new(1).expect("the browser surface id is non-zero");
        let registrations = elements.registrations().to_vec();
        let capabilities = crate::capabilities::detect_host_capabilities()
            .negotiate(whisker_protocol::ProtocolVersion::CURRENT)
            .map_err(|error| WebError(format!("negotiate Web Host capabilities: {error}")))?;
        let surface = SurfaceRuntime::with_element_registry_and_protocol(
            surface_id,
            StyleEnvironment::new(viewport.0, viewport.1, viewport.2, 16.0),
            elements,
            capabilities.protocol(),
        );
        let wake = RuntimeWakeHandle::new(request_frame);
        let resource_store = WebResourceStore::new();
        let resources = WebResourceService::new(resource_store.clone());
        Ok(Self {
            root: root.clone(),
            runtime: RuntimeInstance::new(surface, wake),
            measurements: DomMeasurementProvider::with_elements(
                document.clone(),
                &registrations,
                &element_factories,
            )?,
            frames: DomFrameSink::new_with_resources(
                document,
                root,
                surface_id,
                &registrations,
                &element_factories,
                resource_store,
                capabilities,
            )?,
            resources,
            modules,
            viewport,
            viewport_epoch: 1,
            environment_epoch: 1,
        })
    }

    fn drive_frame(&mut self, timestamp_ms: f64) -> Result<(), WebError> {
        self.start_resource_commands();
        self.modules
            .dispatch_pending_events(&self.runtime)
            .map_err(|error| WebError(format!("dispatch Web module event: {error}")))?;
        for event in self.frames.take_events() {
            self.modules
                .with_host(|| {
                    self.runtime.dispatch_input(&InputEvent {
                        surface: self.runtime.surface().surface(),
                        timestamp_ms,
                        kind: InputEventKind::Named(event.name),
                        pointer: None,
                        target: Some(event.target),
                        detail: event.detail,
                    })
                })
                .map_err(|error| WebError(format!("dispatch Web provider event: {error}")))?;
        }
        let current = viewport(&browser_window()?)?;
        if current != self.viewport {
            self.viewport = current;
            self.viewport_epoch = self.viewport_epoch.wrapping_add(1).max(1);
            self.environment_epoch = self.environment_epoch.wrapping_add(1).max(1);
        }
        let drive = self
            .modules
            .with_host(|| {
                self.runtime.drive_frame(
                    timestamp_ms,
                    StyleEnvironment::new(self.viewport.0, self.viewport.1, self.viewport.2, 16.0),
                    self.environment_epoch,
                    self.viewport_epoch,
                    &mut self.measurements,
                    &mut self.frames,
                    LayoutOptions::default(),
                )
            })
            .map_err(|error| WebError(format!("drive Web frame: {error}")))?;
        self.modules
            .dispatch_pending_events(&self.runtime)
            .map_err(|error| WebError(format!("dispatch Web module event: {error}")))?;
        self.start_resource_commands();
        if drive.needs_frame {
            request_frame();
        }
        Ok(())
    }

    fn start_resource_commands(&self) {
        for command in self.runtime.surface().take_resource_commands() {
            let resources = self.resources.clone();
            spawn_local(async move {
                let result = async {
                    resources.handle(command).await?;
                    let events = resources.take_events();
                    APPLICATION.with(|slot| {
                        let slot = slot.borrow();
                        let application = slot
                            .as_ref()
                            .ok_or_else(|| WebError("a Web application is not mounted".into()))?;
                        for event in events {
                            application
                                .modules
                                .with_host(|| application.runtime.dispatch_resource_event(&event))
                                .map_err(|error| {
                                    WebError(format!("dispatch Web resource event: {error}"))
                                })?;
                        }
                        Ok::<_, WebError>(())
                    })
                }
                .await;
                if let Err(error) = result {
                    web_sys::console::error_1(&error.to_string().into());
                }
            });
        }
    }
}

fn install_pointer_listeners(root: &web_sys::Element) -> Result<(), WebError> {
    for (event_name, phase) in [
        ("pointerdown", WebPointerPhase::Down),
        ("pointermove", WebPointerPhase::Move),
        ("pointerup", WebPointerPhase::Up),
        ("pointercancel", WebPointerPhase::Cancel),
    ] {
        let event_root = root.clone();
        let listener = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(
            move |event: web_sys::PointerEvent| {
                let bounds = event_root.get_bounding_client_rect();
                let input = WebPointerEvent {
                    phase,
                    timestamp_ms: event.time_stamp(),
                    pointer_id: stable_pointer_id(event.pointer_id()),
                    pointer_kind: pointer_kind(&event.pointer_type()),
                    client_position: InputPoint {
                        x: event.client_x() as f32,
                        y: event.client_y() as f32,
                    },
                    buttons: u32::from(event.buttons()),
                    changed_button: event.button(),
                };
                let result = APPLICATION.with(|slot| {
                    let slot = slot.borrow();
                    let application = slot
                        .as_ref()
                        .ok_or_else(|| WebError("Web application is not mounted".into()))?;
                    application
                        .modules
                        .with_host(|| {
                            let presentation = application.frames.take_presentation_updates();
                            dispatch_pointer(
                                &application.runtime,
                                InputPoint {
                                    x: bounds.left() as f32,
                                    y: bounds.top() as f32,
                                },
                                input,
                                &presentation,
                            )
                        })
                        .map(|_| ())
                });
                if let Err(error) = result {
                    web_sys::console::error_1(&error.to_string().into());
                }
            },
        );
        root.add_event_listener_with_callback(event_name, listener.as_ref().unchecked_ref())
            .map_err(|error| js_error("register pointer listener", error))?;
        listener.forget();
    }
    Ok(())
}

pub(crate) fn request_frame() {
    FRAME_SCHEDULED.with(|scheduled| {
        if scheduled.replace(true) {
            return;
        }
        let callback = Closure::once(move |timestamp_ms: f64| {
            FRAME_SCHEDULED.with(|scheduled| scheduled.set(false));
            let result = APPLICATION.with(|slot| {
                let mut slot = slot.borrow_mut();
                slot.as_mut()
                    .ok_or_else(|| WebError("Web application is not mounted".into()))?
                    .drive_frame(timestamp_ms)
            });
            finish_frame(result);
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

/// Drives latency-sensitive Host input after the current browser callback but
/// before the browser can paint a newer native scroll offset with stale rows.
pub(crate) fn request_urgent_frame() {
    let application_is_mounted = APPLICATION.with(|slot| {
        slot.try_borrow()
            .is_ok_and(|application| application.is_some())
    });
    if !application_is_mounted {
        return;
    }
    URGENT_FRAME_SCHEDULED.with(|scheduled| {
        if scheduled.replace(true) {
            return;
        }
        spawn_local(async {
            URGENT_FRAME_SCHEDULED.with(|scheduled| scheduled.set(false));
            let timestamp_ms = web_sys::window()
                .and_then(|window| window.performance())
                .map_or_else(js_sys::Date::now, |performance| performance.now());
            let result = APPLICATION.with(|slot| {
                let mut slot = slot.borrow_mut();
                match slot.as_mut() {
                    Some(application) => application.drive_frame(timestamp_ms),
                    None => Ok(()),
                }
            });
            finish_frame(result);
        });
    });
}

fn finish_frame(result: Result<(), WebError>) {
    match result {
        Ok(()) => RETRYING_FAILED_FRAME.set(false),
        Err(error) => {
            web_sys::console::error_1(&error.to_string().into());
            RETRYING_FAILED_FRAME.with(|retrying| {
                if !retrying.replace(true) {
                    request_frame();
                }
            });
        }
    }
}

fn browser_window() -> Result<web_sys::Window, WebError> {
    web_sys::window().ok_or_else(|| WebError("browser window is unavailable".into()))
}

fn viewport(window: &web_sys::Window) -> Result<(f32, f32, f32), WebError> {
    let width = window
        .inner_width()
        .map_err(|error| js_error("read viewport width", error))?
        .as_f64()
        .ok_or_else(|| WebError("viewport width was not numeric".into()))? as f32;
    let height = window
        .inner_height()
        .map_err(|error| js_error("read viewport height", error))?
        .as_f64()
        .ok_or_else(|| WebError("viewport height was not numeric".into()))? as f32;
    Ok((width, height, window.device_pixel_ratio() as f32))
}
