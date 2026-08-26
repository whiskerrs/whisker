use std::cell::{Cell, RefCell};

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use whisker::runtime::RuntimeWakeHandle;
use whisker::{Element, ElementRegistry, RuntimeInstance, SurfaceRuntime, WhiskerModule};
use whisker_engine::LayoutOptions;
use whisker_protocol::{InputEvent, InputEventKind, ResourceId, SurfaceId};
use whisker_style::StyleEnvironment;

use crate::measure::text::DomMeasurementProvider;
use crate::scene::frame_sink::DomFrameSink;
use crate::{
    BuiltInElementModule, WebAppConfig, WebError, WebModuleDefinition, js_error, set_style,
};

thread_local! {
    static APPLICATION: RefCell<Option<WebApplication>> = const { RefCell::new(None) };
    static FRAME_SCHEDULED: Cell<bool> = const { Cell::new(false) };
}

/// Mounts a Whisker application into the current browser document.
///
/// The generated `gen/web` crate calls this once from its WASM start
/// function. Subsequent work is driven by `requestAnimationFrame`.
pub fn run(config: WebAppConfig, application: fn() -> Element) -> Result<(), WebError> {
    APPLICATION.with(|slot| {
        if slot.borrow().is_some() {
            return Err(WebError("a Web application is already mounted".into()));
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
            .map_err(|error| WebError(format!("mount Whisker application: {error}")))
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

struct WebApplication {
    runtime: RuntimeInstance,
    measurements: DomMeasurementProvider,
    frames: DomFrameSink,
    viewport: (f32, f32, f32),
    viewport_epoch: u32,
    environment_epoch: u64,
}

impl WebApplication {
    fn new(mut config: WebAppConfig) -> Result<Self, WebError> {
        let mut element_factories = BuiltInElementModule::definition().into_factories();
        let elements = ElementRegistry::standard_builder()
            .register_modules(config.element_modules.drain(..))
            .build()
            .map_err(|error| WebError(format!("build element registry: {error}")))?;
        element_factories.extend(
            config
                .module_definitions
                .drain(..)
                .flat_map(WebModuleDefinition::into_factories),
        );
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
        let surface = SurfaceRuntime::with_element_registry(
            surface_id,
            StyleEnvironment::new(viewport.0, viewport.1, viewport.2, 16.0),
            elements,
        );
        let wake = RuntimeWakeHandle::new(request_frame);
        Ok(Self {
            runtime: RuntimeInstance::new(surface, wake),
            measurements: DomMeasurementProvider::new(document.clone()),
            frames: DomFrameSink::new(
                document,
                root,
                surface_id,
                &registrations,
                &element_factories,
            )?,
            viewport,
            viewport_epoch: 1,
            environment_epoch: 1,
        })
    }

    fn drive_frame(&mut self, timestamp_ms: f64) -> Result<(), WebError> {
        for event in self.frames.take_events() {
            self.runtime
                .dispatch_input(&InputEvent {
                    surface: self.runtime.surface().surface(),
                    timestamp_ms,
                    kind: InputEventKind::Named(event.name),
                    pointer: None,
                    target: Some(event.target),
                    detail: event.detail,
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
            .runtime
            .drive_frame(
                timestamp_ms,
                StyleEnvironment::new(self.viewport.0, self.viewport.1, self.viewport.2, 16.0),
                self.environment_epoch,
                self.viewport_epoch,
                &mut self.measurements,
                &mut self.frames,
                LayoutOptions::default(),
            )
            .map_err(|error| WebError(format!("drive Web frame: {error}")))?;
        if drive.needs_frame {
            request_frame();
        }
        Ok(())
    }
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
