use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use whisker::runtime::RuntimeWakeHandle;
use whisker::runtime::module::RustModuleRuntime;
use whisker::{Element, ElementRegistry, RuntimeInstance, SurfaceRuntime};
use whisker_engine::LayoutOptions;
use whisker_protocol::{
    CapabilityEntry, CapabilitySupport, InputEvent, InputEventKind, InputPoint, PointerId,
    PointerInput, PointerKind, ProtocolVersion, RenderCapabilities, RenderCapability, SurfaceId,
    WhiskerValue,
};
use whisker_style::StyleEnvironment;

use crate::node::NodeWidget;
use crate::scene::{GtkFrameSink, SharedScrollFeedback, max_scroll_offset};
use crate::text::PangoMeasurementProvider;
use crate::{LinuxAppConfig, LinuxError, LinuxModuleDefinition};

thread_local! {
    static APPLICATION: RefCell<Option<LinuxApplication>> = const { RefCell::new(None) };
    static FRAME_SCHEDULED: Cell<bool> = const { Cell::new(false) };
    // Separate from APPLICATION so wakes raised during a frame can still schedule one.
    static SURFACE: RefCell<Option<NodeWidget>> = const { RefCell::new(None) };
}

#[cfg(feature = "hot-reload")]
type HotReload = whisker_dev_runtime::NativeHotReload;

#[cfg(not(feature = "hot-reload"))]
struct HotReload;

#[cfg(not(feature = "hot-reload"))]
impl HotReload {
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

    fn record_frame(&self, _timing: Option<whisker::runtime::FrameTiming>) {}
}

const ROOT_FONT_SIZE: f32 = 14.0;
const MOUSE_POINTER: u64 = 1;

pub(crate) fn host_capabilities() -> RenderCapabilities {
    let native = [
        RenderCapability::EllipticalBorderRadius,
        RenderCapability::VisualEffects,
        RenderCapability::TextEffects,
        RenderCapability::TextTypography,
        RenderCapability::RichText,
        RenderCapability::Cursor,
        RenderCapability::ResourceLifecycle,
        RenderCapability::LinearGradients,
        RenderCapability::RadialGradients,
        RenderCapability::ConicGradients,
        RenderCapability::BackgroundGeometry,
        RenderCapability::BackgroundLayerStacking,
        RenderCapability::BackgroundImageResources,
    ]
    .map(|capability| CapabilityEntry {
        capability,
        support: CapabilitySupport::Native,
    });
    // GSK cannot sample pixels painted behind a node.
    let emulated = CapabilityEntry {
        capability: RenderCapability::BackdropBlur,
        support: CapabilitySupport::Emulated,
    };
    RenderCapabilities::new(
        ProtocolVersion::CURRENT,
        native.into_iter().chain([emulated]),
    )
    .expect("GTK capability profile is unique")
}

pub(crate) fn run_application(
    config: LinuxAppConfig,
    application: fn() -> Element,
    application_hash: fn() -> u64,
) -> Result<(), LinuxError> {
    gtk::init().map_err(|error| LinuxError(format!("initialize GTK: {error}")))?;
    let app = gtk::Application::builder()
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let failure = Rc::new(RefCell::new(None));
    {
        let failure = Rc::clone(&failure);
        app.connect_activate(move |app| {
            if let Err(error) = activate(app, &config, application, application_hash) {
                *failure.borrow_mut() = Some(error);
                app.quit();
            }
        });
    }
    app.run_with_args::<&str>(&[]);
    SURFACE.with(|slot| slot.borrow_mut().take());
    APPLICATION.with(|slot| slot.borrow_mut().take());
    match failure.borrow_mut().take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn activate(
    app: &gtk::Application,
    config: &LinuxAppConfig,
    application: fn() -> Element,
    application_hash: fn() -> u64,
) -> Result<(), LinuxError> {
    let surface = NodeWidget::new();
    surface.state_mut().hit_target = true;
    surface.set_hexpand(true);
    surface.set_vexpand(true);
    let [red, green, blue] = config.background_rgb;
    surface.state_mut().background = Some(gdk::RGBA::new(
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        1.0,
    ));
    surface.set_resize_callback(request_frame);
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(config.title.as_str())
        .default_width(config.width)
        .default_height(config.height)
        .child(&surface)
        .build();

    let host = LinuxApplication::new(
        surface.clone(),
        &config.modules,
        application,
        application_hash,
    )?;
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(host));
    SURFACE.with(|slot| *slot.borrow_mut() = Some(surface.clone()));
    with_application(|host| {
        host.modules
            .with_host(|| host.runtime.mount(application))
            .map(|_| ())
            .map_err(|error| LinuxError(format!("mount Whisker application: {error}")))
    })?;
    install_input(&surface);
    window.present();
    request_frame();
    Ok(())
}

fn with_application<T>(
    work: impl FnOnce(&mut LinuxApplication) -> Result<T, LinuxError>,
) -> Result<T, LinuxError> {
    APPLICATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let host = slot
            .as_mut()
            .ok_or_else(|| LinuxError("GTK application is not mounted".into()))?;
        work(host)
    })
}

struct LinuxApplication {
    surface: NodeWidget,
    runtime: RuntimeInstance,
    hot_reload: HotReload,
    measurements: PangoMeasurementProvider,
    frames: GtkFrameSink,
    feedback: SharedScrollFeedback,
    modules: RustModuleRuntime,
    viewport: (f32, f32, f32),
    viewport_epoch: u32,
    environment_epoch: u64,
}

impl LinuxApplication {
    fn new(
        surface: NodeWidget,
        definitions: &[LinuxModuleDefinition],
        application: fn() -> Element,
        application_hash: fn() -> u64,
    ) -> Result<Self, LinuxError> {
        let modules = RustModuleRuntime::new(
            definitions.iter().map(|module| module.service.clone()),
            request_frame,
        )
        .map_err(|error| LinuxError(format!("bind GTK modules: {error}")))?;
        let elements = ElementRegistry::standard_with_linked_providers()
            .map_err(|error| LinuxError(format!("build element registry: {error}")))?;
        let capabilities = host_capabilities()
            .negotiate(ProtocolVersion::CURRENT)
            .map_err(|error| LinuxError(format!("negotiate GTK Host capabilities: {error}")))?;
        let surface_id = SurfaceId::new(1).expect("the GTK surface id is non-zero");
        let viewport = (1.0, 1.0, surface.scale_factor() as f32);
        let runtime_surface = SurfaceRuntime::with_element_registry_and_protocol(
            surface_id,
            StyleEnvironment::new(viewport.0, viewport.1, viewport.2, ROOT_FONT_SIZE),
            elements.clone(),
            capabilities.protocol(),
        );
        let wake = RuntimeWakeHandle::new(|| glib::MainContext::default().invoke(request_frame));
        let views = definitions
            .iter()
            .flat_map(|module| module.views.iter().cloned())
            .collect::<Vec<_>>();
        let measurements = PangoMeasurementProvider::new(crate::text::new_context())
            .with_view_measurements(elements.registrations(), &views);
        let frames = GtkFrameSink::new(
            surface.clone(),
            surface_id,
            elements.registrations(),
            capabilities,
            measurements.prepared(),
            measurements.context().clone(),
            views,
        );
        Ok(Self {
            feedback: frames.feedback(),
            surface,
            runtime: RuntimeInstance::new(runtime_surface, wake.clone()),
            hot_reload: HotReload::new(wake, application, application_hash),
            measurements,
            frames,
            modules,
            viewport,
            viewport_epoch: 1,
            environment_epoch: 1,
        })
    }

    fn drive_frame(&mut self, timestamp_ms: f64) -> Result<bool, LinuxError> {
        let current = (
            self.surface.width() as f32,
            self.surface.height() as f32,
            self.surface.scale_factor() as f32,
        );
        if current.0 <= 0.0 || current.1 <= 0.0 {
            return Ok(false);
        }
        if current != self.viewport {
            self.viewport = current;
            self.viewport_epoch = self.viewport_epoch.wrapping_add(1).max(1);
            self.environment_epoch = self.environment_epoch.wrapping_add(1).max(1);
        }
        if let Err(error) = self
            .modules
            .with_host(|| self.hot_reload.apply(&mut self.runtime))
        {
            eprintln!("whisker: hot reload failed: {error}");
        }
        self.dispatch_provider_events(timestamp_ms)?;
        let drive = self
            .modules
            .with_host(|| {
                self.runtime.drive_frame(
                    timestamp_ms,
                    StyleEnvironment::new(
                        self.viewport.0,
                        self.viewport.1,
                        self.viewport.2,
                        ROOT_FONT_SIZE,
                    ),
                    self.environment_epoch,
                    self.viewport_epoch,
                    &mut self.measurements,
                    &mut self.frames,
                    LayoutOptions::default(),
                )
            })
            .map_err(|error| LinuxError(format!("drive GTK frame: {error}")))?;
        self.hot_reload.record_frame(drive.timing);
        self.modules
            .dispatch_pending_events(&self.runtime)
            .map_err(|error| LinuxError(format!("dispatch GTK module event: {error}")))?;
        Ok(drive.needs_frame)
    }

    fn dispatch_provider_events(&mut self, timestamp_ms: f64) -> Result<(), LinuxError> {
        let events = self.feedback.borrow_mut().take_events();
        for event in events {
            let presentation = self.feedback.borrow_mut().take_presentation_updates();
            self.modules
                .with_host(|| {
                    self.runtime.dispatch_input_with_presentation(
                        &InputEvent {
                            presentation_revision: None,
                            surface: self.runtime.surface().surface(),
                            timestamp_ms,
                            kind: InputEventKind::Named(event.name),
                            pointer: None,
                            target: Some(event.target),
                            detail: event.detail,
                        },
                        &presentation,
                    )
                })
                .map_err(|error| LinuxError(format!("dispatch GTK element event: {error}")))?;
        }
        Ok(())
    }

    fn dispatch_pointer(
        &mut self,
        kind: InputEventKind,
        pointer: PointerInput,
        timestamp_ms: f64,
    ) -> Result<(), LinuxError> {
        let presentation = self.feedback.borrow_mut().take_presentation_updates();
        let event = InputEvent {
            presentation_revision: None,
            surface: self.runtime.surface().surface(),
            timestamp_ms,
            kind,
            pointer: Some(pointer),
            target: None,
            detail: WhiskerValue::Null,
        };
        self.modules
            .with_host(|| {
                self.runtime
                    .dispatch_input_with_presentation(&event, &presentation)
            })
            .map(|_| ())
            .map_err(|error| LinuxError(format!("dispatch GTK pointer input: {error}")))
    }

    /// Scrolls the innermost enabled ScrollView under `point` that can still move.
    fn scroll_at(&mut self, point: InputPoint, delta: [f32; 2]) -> bool {
        let Some(widget) = self.surface.pick(
            f64::from(point.x),
            f64::from(point.y),
            gtk::PickFlags::NON_TARGETABLE,
        ) else {
            return false;
        };
        let mut node = self.frames.node_for_widget(&widget);
        while let Some(id) = node {
            let scroll = self.frames.widget(id).and_then(|widget| {
                widget
                    .state()
                    .scroll
                    .map(|scroll| (scroll, max_scroll_offset(widget)))
            });
            if let Some((scroll, max)) = scroll
                && scroll.enabled
            {
                let axis = usize::from(!scroll.horizontal);
                let amount = if delta[axis] == 0.0 {
                    delta[1 - axis]
                } else {
                    delta[axis]
                };
                let target = (scroll.offset[axis] + amount).clamp(0.0, max[axis]);
                if target != scroll.offset[axis] {
                    let mut offset = scroll.offset;
                    offset[axis] = target;
                    return self.frames.scroll_to(id, offset);
                }
            }
            node = self.frames.parent(id);
        }
        false
    }
}

pub(crate) fn request_frame() {
    if FRAME_SCHEDULED.replace(true) {
        return;
    }
    let surface = SURFACE.with(|slot| slot.borrow().clone());
    let Some(surface) = surface else {
        FRAME_SCHEDULED.set(false);
        return;
    };
    surface.add_tick_callback(|_, clock| {
        FRAME_SCHEDULED.set(false);
        let timestamp_ms = clock.frame_time() as f64 / 1000.0;
        match with_application(|host| host.drive_frame(timestamp_ms)) {
            Ok(true) => request_frame(),
            Ok(false) => {}
            Err(error) => eprintln!("whisker: {error}"),
        }
        glib::ControlFlow::Break
    });
}

fn surface_point(widget: &NodeWidget, event: &gdk::Event) -> Option<InputPoint> {
    let (x, y) = event.position()?;
    let native = widget.native()?;
    let (offset_x, offset_y) = native.surface_transform();
    let point = native.compute_point(
        widget,
        &gtk::graphene::Point::new((x - offset_x) as f32, (y - offset_y) as f32),
    )?;
    Some(InputPoint {
        x: point.x(),
        y: point.y(),
    })
}

fn button_bit(button: u32) -> (i16, u32) {
    match button {
        1 => (0, 1),
        2 => (1, 4),
        3 => (2, 2),
        8 => (3, 8),
        9 => (4, 16),
        _ => (-1, 0),
    }
}

fn held_buttons(state: gdk::ModifierType) -> u32 {
    [
        (gdk::ModifierType::BUTTON1_MASK, 1),
        (gdk::ModifierType::BUTTON2_MASK, 4),
        (gdk::ModifierType::BUTTON3_MASK, 2),
        (gdk::ModifierType::BUTTON4_MASK, 8),
        (gdk::ModifierType::BUTTON5_MASK, 16),
    ]
    .into_iter()
    .filter(|(mask, _)| state.contains(*mask))
    .fold(0, |buttons, (_, bit)| buttons | bit)
}

fn translate_pointer(
    widget: &NodeWidget,
    event: &gdk::Event,
) -> Option<(InputEventKind, PointerInput)> {
    let position = surface_point(widget, event)?;
    let held = held_buttons(event.modifier_state());
    let mouse = |changed_button: i16, buttons: u32| PointerInput {
        id: PointerId::new(MOUSE_POINTER).expect("mouse pointer id is non-zero"),
        kind: PointerKind::Mouse,
        position,
        buttons,
        changed_button,
    };
    let touch = |buttons: u32| {
        let sequence = event.event_sequence();
        let sequence: *const gdk::ffi::GdkEventSequence = sequence.to_glib_none().0;
        let sequence = sequence as usize as u64;
        PointerInput {
            id: PointerId::new(sequence.wrapping_add(2).max(2)).expect("touch id is non-zero"),
            kind: PointerKind::Touch,
            position,
            buttons,
            changed_button: if buttons == 0 { 0 } else { -1 },
        }
    };
    match event.event_type() {
        gdk::EventType::ButtonPress => {
            let button = event.downcast_ref::<gdk::ButtonEvent>()?.button();
            let (changed, bit) = button_bit(button);
            Some((InputEventKind::PointerDown, mouse(changed, held | bit)))
        }
        gdk::EventType::ButtonRelease => {
            let button = event.downcast_ref::<gdk::ButtonEvent>()?.button();
            let (changed, bit) = button_bit(button);
            Some((InputEventKind::PointerUp, mouse(changed, held & !bit)))
        }
        gdk::EventType::MotionNotify => Some((InputEventKind::PointerMove, mouse(-1, held))),
        gdk::EventType::TouchBegin => Some((InputEventKind::PointerDown, touch(1))),
        gdk::EventType::TouchUpdate => Some((InputEventKind::PointerMove, touch(1))),
        gdk::EventType::TouchEnd => Some((InputEventKind::PointerUp, touch(0))),
        gdk::EventType::TouchCancel => Some((InputEventKind::PointerCancel, touch(0))),
        _ => None,
    }
}

fn update_cursor(surface: &NodeWidget, point: InputPoint) {
    let picked = surface.pick(
        f64::from(point.x),
        f64::from(point.y),
        gtk::PickFlags::NON_TARGETABLE,
    );
    let name = picked.and_then(|widget| {
        with_application(|host| Ok(host.frames.cursor_for_widget(&widget)))
            .ok()
            .flatten()
    });
    surface.set_cursor_from_name(name);
}

fn install_input(surface: &NodeWidget) {
    // Some backends, such as Broadway, deliver wheel events without a position.
    let last_pointer = Rc::new(Cell::new(InputPoint { x: 0.0, y: 0.0 }));
    let legacy = gtk::EventControllerLegacy::new();
    {
        let surface = surface.clone();
        let last_pointer = Rc::clone(&last_pointer);
        legacy.connect_event(move |_, event| {
            if let Some((kind, pointer)) = translate_pointer(&surface, event) {
                last_pointer.set(pointer.position);
                if kind == InputEventKind::PointerMove {
                    update_cursor(&surface, pointer.position);
                }
                let timestamp_ms = f64::from(event.time());
                if let Err(error) =
                    with_application(|host| host.dispatch_pointer(kind, pointer, timestamp_ms))
                {
                    eprintln!("whisker: {error}");
                }
            }
            glib::Propagation::Proceed
        });
    }
    surface.add_controller(legacy);

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    let target = surface.clone();
    scroll.connect_scroll(move |controller, dx, dy| {
        let wheel = controller.unit() == gdk::ScrollUnit::Wheel;
        let step = if wheel { 48.0 } else { 1.0 };
        let delta = [dx as f32 * step, dy as f32 * step];
        let point = controller
            .current_event()
            .and_then(|event| surface_point(&target, &event))
            .unwrap_or_else(|| last_pointer.get());
        let moved = with_application(|host| Ok(host.scroll_at(point, delta))).unwrap_or(false);
        if moved {
            request_frame();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    surface.add_controller(scroll);
}
