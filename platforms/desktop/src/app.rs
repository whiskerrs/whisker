use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::accessibility::{DesktopAccessibilityAction, DesktopAccessibilityBridge};
use crate::{
    BuiltInElementModule, DesktopElementFactory, DesktopFrameContext, DesktopModuleDefinition,
    DesktopMouseButton, DesktopPointerAdapter, DesktopPointerPhase, DesktopRuntime,
    DesktopTextInputEvent, DesktopTextInputKey, WhiskerModule,
};
use whisker::runtime::RuntimeWakeHandle;
use whisker::runtime::module::RustModuleDefinition;
use whisker::{Element, ElementModuleDefinition, ElementRegistry, RuntimeInstance, SurfaceRuntime};
use whisker_protocol::{CursorKeyword, InputEvent, InputEventKind, SurfaceId, WhiskerValue};
use whisker_style::StyleEnvironment;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{
    ElementState, Ime, Modifiers, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowAttributes, WindowId};

#[cfg(target_os = "macos")]
const TARGET_NAME: &str = "macOS";
#[cfg(target_os = "windows")]
const TARGET_NAME: &str = "Windows";
#[cfg(target_os = "linux")]
const TARGET_NAME: &str = "Linux";

fn cursor_icon(value: CursorKeyword) -> CursorIcon {
    match value {
        CursorKeyword::Auto | CursorKeyword::Default | CursorKeyword::None => CursorIcon::Default,
        CursorKeyword::ContextMenu => CursorIcon::ContextMenu,
        CursorKeyword::Help => CursorIcon::Help,
        CursorKeyword::Pointer => CursorIcon::Pointer,
        CursorKeyword::Progress => CursorIcon::Progress,
        CursorKeyword::Wait => CursorIcon::Wait,
        CursorKeyword::Cell => CursorIcon::Cell,
        CursorKeyword::Crosshair => CursorIcon::Crosshair,
        CursorKeyword::Text => CursorIcon::Text,
        CursorKeyword::VerticalText => CursorIcon::VerticalText,
        CursorKeyword::Alias => CursorIcon::Alias,
        CursorKeyword::Copy => CursorIcon::Copy,
        CursorKeyword::Move => CursorIcon::Move,
        CursorKeyword::NoDrop => CursorIcon::NoDrop,
        CursorKeyword::NotAllowed => CursorIcon::NotAllowed,
        CursorKeyword::Grab => CursorIcon::Grab,
        CursorKeyword::Grabbing => CursorIcon::Grabbing,
        CursorKeyword::ColResize => CursorIcon::ColResize,
        CursorKeyword::RowResize => CursorIcon::RowResize,
        CursorKeyword::NResize => CursorIcon::NResize,
        CursorKeyword::EResize => CursorIcon::EResize,
        CursorKeyword::SResize => CursorIcon::SResize,
        CursorKeyword::WResize => CursorIcon::WResize,
        CursorKeyword::NeResize => CursorIcon::NeResize,
        CursorKeyword::NwResize => CursorIcon::NwResize,
        CursorKeyword::SeResize => CursorIcon::SeResize,
        CursorKeyword::SwResize => CursorIcon::SwResize,
        CursorKeyword::EwResize => CursorIcon::EwResize,
        CursorKeyword::NsResize => CursorIcon::NsResize,
        CursorKeyword::NeswResize => CursorIcon::NeswResize,
        CursorKeyword::NwseResize => CursorIcon::NwseResize,
        CursorKeyword::ZoomIn => CursorIcon::ZoomIn,
        CursorKeyword::ZoomOut => CursorIcon::ZoomOut,
    }
}

fn mouse_button(value: MouseButton) -> DesktopMouseButton {
    match value {
        MouseButton::Left => DesktopMouseButton::Primary,
        MouseButton::Middle => DesktopMouseButton::Auxiliary,
        MouseButton::Right => DesktopMouseButton::Secondary,
        MouseButton::Back => DesktopMouseButton::Back,
        MouseButton::Forward => DesktopMouseButton::Forward,
        MouseButton::Other(_) => DesktopMouseButton::Other,
    }
}

fn touch_phase(value: TouchPhase) -> DesktopPointerPhase {
    match value {
        TouchPhase::Started => DesktopPointerPhase::Down,
        TouchPhase::Moved => DesktopPointerPhase::Move,
        TouchPhase::Ended => DesktopPointerPhase::Up,
        TouchPhase::Cancelled => DesktopPointerPhase::Cancel,
    }
}

fn scroll_delta(value: MouseScrollDelta, scale: f64) -> [f32; 2] {
    match value {
        MouseScrollDelta::LineDelta(x, y) => [-x * 40.0, -y * 40.0],
        MouseScrollDelta::PixelDelta(position) => {
            let logical = position.to_logical::<f32>(scale);
            [-logical.x, -logical.y]
        }
    }
}

/// Configuration for one standalone native Desktop window.
#[derive(Clone, Debug)]
pub struct DesktopAppConfig {
    /// Window and application display name.
    pub title: String,
    /// Initial logical width in points.
    pub width: f64,
    /// Initial logical height in points.
    pub height: f64,
    /// Static Host background visible before and behind Whisker content.
    background_rgb: [u8; 3],
    /// Modules selected for this target, paired with their portable schema.
    modules: Vec<DesktopModuleInstallation>,
    element_factories: Vec<DesktopElementFactory>,
    module_services: Vec<RustModuleDefinition>,
}

#[derive(Clone, Debug)]
struct DesktopModuleInstallation {
    elements: ElementModuleDefinition,
    host: DesktopModuleDefinition,
}

impl DesktopAppConfig {
    /// Creates the default standalone Desktop window configuration.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 1024.0,
            height: 720.0,
            background_rgb: [255, 255, 255],
            modules: Vec::new(),
            element_factories: Vec::new(),
            module_services: Vec::new(),
        }
    }

    /// Sets the static native-window background.
    ///
    /// Application code normally configures this through `app.background(...)`
    /// in `whisker.rs`; generated Desktop Hosts call this method.
    pub fn with_background_rgb(mut self, red: u8, green: u8, blue: u8) -> Self {
        self.background_rgb = [red, green, blue];
        self
    }

    /// Installs one module's portable element schema and Desktop implementation.
    ///
    /// Keeping the pair together makes it impossible for generated Hosts to
    /// accidentally install only one side of a module.
    pub fn with_module(
        mut self,
        elements: ElementModuleDefinition,
        host: DesktopModuleDefinition,
    ) -> Self {
        self.modules
            .push(DesktopModuleInstallation { elements, host });
        self
    }
}

/// Failure while creating or running the native Desktop Host.
#[derive(Debug)]
pub struct DesktopAppError(String);

impl fmt::Display for DesktopAppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for DesktopAppError {}

/// Runs a standalone Whisker application in a native Desktop window.
pub fn run(config: DesktopAppConfig, application: fn() -> Element) -> Result<(), DesktopAppError> {
    run_with_application_hash(config, application, || 0)
}

/// Runs a standalone Whisker application with its generated source hash.
///
/// The hash is ignored in normal builds. With the `hot-reload` feature it
/// distinguishes an edited application root from a component-only update.
pub fn run_with_application_hash(
    mut config: DesktopAppConfig,
    application: fn() -> Element,
    application_hash: fn() -> u64,
) -> Result<(), DesktopAppError> {
    let built_ins = BuiltInElementModule::definition();
    config
        .module_services
        .push(built_ins.service_definition().clone());
    let mut element_factories = built_ins.into_factories();
    let elements = ElementRegistry::standard_builder()
        .register_modules(config.modules.iter().map(|module| module.elements.clone()))
        .build()
        .map_err(|error| DesktopAppError(format!("build element registry: {error}")))?;
    for module in config.modules.drain(..) {
        let definition = module.host;
        config
            .module_services
            .push(definition.service_definition().clone());
        element_factories.extend(definition.into_factories());
    }
    config.element_factories = element_factories;
    let event_loop = EventLoop::<HostEvent>::with_user_event()
        .build()
        .map_err(|error| DesktopAppError(format!("create {TARGET_NAME} event loop: {error}")))?;
    let proxy = event_loop.create_proxy();
    let mut application =
        DesktopApplication::new(config, elements, application, application_hash, proxy);
    event_loop
        .run_app(&mut application)
        .map_err(|error| DesktopAppError(format!("run {TARGET_NAME} event loop: {error}")))
}

#[derive(Debug)]
enum HostEvent {
    RequestFrame,
    Accessibility(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for HostEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}

struct DesktopApplication {
    config: DesktopAppConfig,
    elements: ElementRegistry,
    application: fn() -> Element,
    application_hash: fn() -> u64,
    hot_reload: Option<DesktopHotReload>,
    proxy: EventLoopProxy<HostEvent>,
    window: Option<Arc<Window>>,
    accessibility_adapter: Option<accesskit_winit::Adapter>,
    accessibility_bridge: DesktopAccessibilityBridge,
    runtime: Option<RuntimeInstance>,
    host: Option<DesktopRuntime>,
    viewport: PhysicalSize<u32>,
    viewport_epoch: u32,
    environment_epoch: u64,
    started_at: Instant,
    frame_failed: bool,
    pointer: DesktopPointerAdapter,
    pending_scroll_settle: Option<(Instant, [f32; 2])>,
    modifiers: Modifiers,
    clipboard: Option<arboard::Clipboard>,
}

impl DesktopApplication {
    fn new(
        config: DesktopAppConfig,
        elements: ElementRegistry,
        application: fn() -> Element,
        application_hash: fn() -> u64,
        proxy: EventLoopProxy<HostEvent>,
    ) -> Self {
        Self {
            config,
            elements,
            application,
            application_hash,
            hot_reload: None,
            proxy,
            window: None,
            accessibility_adapter: None,
            accessibility_bridge: DesktopAccessibilityBridge::default(),
            runtime: None,
            host: None,
            viewport: PhysicalSize::new(1, 1),
            viewport_epoch: 1,
            environment_epoch: 1,
            started_at: Instant::now(),
            frame_failed: false,
            pointer: DesktopPointerAdapter::new(SurfaceId::new(1).unwrap()),
            pending_scroll_settle: None,
            modifiers: Modifiers::default(),
            clipboard: arboard::Clipboard::new().ok(),
        }
    }

    fn mount(&mut self, event_loop: &ActiveEventLoop) -> Result<(), DesktopAppError> {
        if self.window.is_some() {
            return Ok(());
        }
        let attributes = WindowAttributes::default()
            .with_title(self.config.title.clone())
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height))
            .with_visible(false);
        let window =
            Arc::new(event_loop.create_window(attributes).map_err(|error| {
                DesktopAppError(format!("create {TARGET_NAME} window: {error}"))
            })?);
        let accessibility_adapter = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        self.viewport = window.inner_size();
        let scale = window.scale_factor() as f32;
        let logical = self.viewport.to_logical::<f32>(window.scale_factor());
        let surface_id = SurfaceId::new(1).expect("the standalone surface id is non-zero");
        let capabilities = crate::capabilities::host_capabilities()
            .negotiate(whisker_protocol::ProtocolVersion::CURRENT)
            .map_err(|error| DesktopAppError(format!("negotiate Desktop Host: {error}")))?;
        let surface = SurfaceRuntime::with_element_registry_and_protocol(
            surface_id,
            StyleEnvironment::new(logical.width, logical.height, scale, 14.0),
            self.elements.clone(),
            capabilities.protocol(),
        );
        let element_registrations = surface.element_registrations();
        let wake_proxy = self.proxy.clone();
        let wake = RuntimeWakeHandle::new(move || {
            let _ = wake_proxy.send_event(HostEvent::RequestFrame);
        });
        let host = pollster::block_on(DesktopRuntime::new_with_surface_config(
            window.clone(),
            crate::DesktopSurfaceConfig::new(
                [self.viewport.width, self.viewport.height],
                self.config.background_rgb,
            ),
            surface_id,
            &element_registrations,
            &self.config.element_factories,
            self.config.module_services.clone(),
            wake.clone(),
        ))
        .map_err(|error| DesktopAppError(error.to_string()))?;
        let mut runtime = RuntimeInstance::new(surface, wake.clone());
        host.with_modules(|| runtime.mount(self.application))
            .map_err(|error| DesktopAppError(format!("mount Whisker application: {error}")))?;
        self.hot_reload = Some(DesktopHotReload::new(
            wake,
            self.application,
            self.application_hash,
        ));
        self.host = Some(host);
        self.runtime = Some(runtime);
        self.accessibility_adapter = Some(accessibility_adapter);
        self.window = Some(window);
        // Present the configured Host background and first Whisker scene while
        // the native window is still hidden. Showing an unpresented swapchain
        // here would briefly expose an OS-specific default color.
        self.drive_frame();
        self.window
            .as_ref()
            .expect("mounted Desktop window")
            .set_visible(true);
        if !self.frame_failed {
            self.request_frame();
        }
        Ok(())
    }

    fn request_frame(&self) {
        if !self.frame_failed
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
    }

    fn drive_frame(&mut self) {
        if self.frame_failed || self.viewport.width == 0 || self.viewport.height == 0 {
            return;
        }
        let (Some(window), Some(runtime), Some(host)) =
            (&self.window, &mut self.runtime, &mut self.host)
        else {
            return;
        };
        if let Some(hot_reload) = &mut self.hot_reload
            && let Err(error) = host.with_modules(|| hot_reload.apply(runtime))
        {
            eprintln!("whisker {TARGET_NAME} hot reload failed: {error}");
        }
        let scale = window.scale_factor();
        let logical = self.viewport.to_logical::<f32>(scale);
        let frame_result = host.drive_frame(
            runtime,
            DesktopFrameContext {
                timestamp_ms: self.started_at.elapsed().as_secs_f64() * 1000.0,
                logical_width: logical.width,
                logical_height: logical.height,
                scale: scale as f32,
                environment_epoch: self.environment_epoch,
                viewport_epoch: self.viewport_epoch,
            },
        );
        match frame_result {
            Ok(result) => {
                if result.needs_frame {
                    window.request_redraw();
                }
            }
            Err(error) => {
                self.frame_failed = true;
                eprintln!("whisker {TARGET_NAME} frame failed: {error}");
            }
        }
        if !self.frame_failed {
            self.update_accessibility(false);
            self.sync_text_input();
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.viewport = size;
        if let Some(host) = &mut self.host {
            host.resize([size.width, size.height]);
        }
        self.viewport_epoch = self.viewport_epoch.wrapping_add(1).max(1);
        self.environment_epoch = self.environment_epoch.wrapping_add(1).max(1);
        self.frame_failed = false;
        self.request_frame();
    }

    fn dispatch_input(&mut self, mut event: InputEvent) {
        if let Some(host) = &self.host {
            host.target_input(&mut event);
        }
        let Some(runtime) = &self.runtime else {
            return;
        };
        if let Err(error) = self
            .host
            .as_ref()
            .expect("mounted Desktop runtime has a Host")
            .with_modules(|| runtime.dispatch_input(&event))
        {
            self.frame_failed = true;
            eprintln!("dispatch {TARGET_NAME} input failed: {error}");
        } else {
            self.request_frame();
        }
    }

    fn sync_text_input(&self) {
        let (Some(window), Some(host)) = (&self.window, &self.host) else {
            return;
        };
        let rect = host.focused_text_input_rect();
        window.set_ime_allowed(rect.is_some());
        if let Some(rect) = rect {
            window.set_ime_cursor_area(
                winit::dpi::LogicalPosition::new(rect.x as f64, (rect.y + rect.height) as f64),
                winit::dpi::LogicalSize::new(
                    rect.width.max(1.0) as f64,
                    rect.height.max(1.0) as f64,
                ),
            );
        }
    }

    fn dispatch_text_input(&mut self, event: DesktopTextInputEvent) {
        if self
            .host
            .as_mut()
            .is_some_and(|host| host.dispatch_text_input(&event))
        {
            self.request_frame();
            self.sync_text_input();
        }
    }

    fn handle_keyboard(&mut self, event: winit::event::KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let shift = self.modifiers.state().shift_key();
        let command = if cfg!(target_os = "macos") {
            self.modifiers.state().super_key()
        } else {
            self.modifiers.state().control_key()
        };
        if command {
            if let Key::Character(character) = &event.logical_key {
                match character.to_lowercase().as_str() {
                    "a" => self.dispatch_text_input(DesktopTextInputEvent::SelectAll),
                    "c" => {
                        if let Some(text) =
                            self.host.as_ref().and_then(DesktopRuntime::selected_text)
                        {
                            if let Some(clipboard) = &mut self.clipboard {
                                let _ = clipboard.set_text(text);
                            }
                        }
                    }
                    "x" => {
                        if let Some(text) =
                            self.host.as_ref().and_then(DesktopRuntime::selected_text)
                        {
                            if let Some(clipboard) = &mut self.clipboard {
                                let _ = clipboard.set_text(text);
                            }
                            self.dispatch_text_input(DesktopTextInputEvent::Cut);
                        }
                    }
                    "v" => {
                        if let Some(clipboard) = &mut self.clipboard
                            && let Ok(text) = clipboard.get_text()
                        {
                            self.dispatch_text_input(DesktopTextInputEvent::Paste(text));
                        }
                    }
                    _ => {}
                }
            }
            return;
        }
        let key = match event.logical_key {
            Key::Named(NamedKey::Backspace) => Some(DesktopTextInputKey::Backspace),
            Key::Named(NamedKey::Delete) => Some(DesktopTextInputKey::Delete),
            Key::Named(NamedKey::ArrowLeft) => Some(DesktopTextInputKey::ArrowLeft),
            Key::Named(NamedKey::ArrowRight) => Some(DesktopTextInputKey::ArrowRight),
            Key::Named(NamedKey::Home) => Some(DesktopTextInputKey::Home),
            Key::Named(NamedKey::End) => Some(DesktopTextInputKey::End),
            Key::Named(NamedKey::Enter) => Some(DesktopTextInputKey::Enter),
            _ => None,
        };
        if let Some(key) = key {
            self.dispatch_text_input(DesktopTextInputEvent::Key { key, shift });
        }
    }

    fn update_accessibility(&mut self, force_full: bool) {
        let (Some(window), Some(host), Some(adapter)) = (
            self.window.as_ref(),
            self.host.as_ref(),
            self.accessibility_adapter.as_mut(),
        ) else {
            return;
        };
        let scale = window.scale_factor();
        let logical = self.viewport.to_logical::<f32>(scale);
        let title = self.config.title.as_str();
        let bridge = &mut self.accessibility_bridge;
        adapter.update_if_active(|| {
            bridge.update(
                host.accessibility_snapshot(),
                title,
                [logical.width, logical.height],
                scale as f32,
                force_full,
            )
        });
    }

    fn handle_accessibility_event(&mut self, event: accesskit_winit::Event) {
        if self.window.as_ref().map(|window| window.id()) != Some(event.window_id) {
            return;
        }
        match event.window_event {
            accesskit_winit::WindowEvent::InitialTreeRequested => {
                self.update_accessibility(true);
            }
            accesskit_winit::WindowEvent::ActionRequested(request) => {
                let action = self.accessibility_bridge.handle_action(&request);
                if let DesktopAccessibilityAction::Click(target) = action {
                    self.dispatch_input(InputEvent {
                        surface: SurfaceId::new(1).expect("standalone surface id"),
                        timestamp_ms: self.started_at.elapsed().as_secs_f64() * 1000.0,
                        kind: InputEventKind::Click,
                        pointer: None,
                        target: Some(target),
                        detail: WhiskerValue::Null,
                    });
                }
                if action != DesktopAccessibilityAction::Ignored {
                    self.update_accessibility(false);
                }
            }
            accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                self.accessibility_bridge.reset();
            }
        }
    }
}

#[cfg(feature = "hot-reload")]
type DesktopHotReload = whisker_dev_runtime::NativeHotReload;

#[cfg(not(feature = "hot-reload"))]
struct DesktopHotReload;

#[cfg(not(feature = "hot-reload"))]
impl DesktopHotReload {
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
}

impl ApplicationHandler<HostEvent> for DesktopApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.mount(event_loop) {
            eprintln!("{error}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: HostEvent) {
        match event {
            HostEvent::RequestFrame => self.request_frame(),
            HostEvent::Accessibility(event) => self.handle_accessibility_event(event),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(|window| window.id()) != Some(window_id) {
            return;
        }
        if let (Some(adapter), Some(window)) =
            (&mut self.accessibility_adapter, self.window.as_ref())
        {
            adapter.process_event(window, &event);
        }
        match event {
            WindowEvent::CloseRequested => {
                if let Some(runtime) = &mut self.runtime {
                    if let Some(host) = &self.host {
                        let _ = host.with_modules(|| runtime.unmount());
                    }
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => self.resize(size),
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = &self.window {
                    self.resize(window.inner_size());
                }
            }
            WindowEvent::RedrawRequested => self.drive_frame(),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers,
            WindowEvent::KeyboardInput { event, .. } => self.handle_keyboard(event),
            WindowEvent::Ime(Ime::Commit(text)) => {
                if !text.is_empty() {
                    self.dispatch_text_input(DesktopTextInputEvent::Commit(text));
                }
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                self.dispatch_text_input(DesktopTextInputEvent::Preedit { text, cursor });
            }
            WindowEvent::Ime(Ime::Enabled | Ime::Disabled) => {}
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(window) = &self.window {
                    let logical = position.to_logical::<f32>(window.scale_factor());
                    let input = self.pointer.cursor_moved(
                        self.started_at.elapsed().as_secs_f64() * 1000.0,
                        [logical.x, logical.y],
                    );
                    self.dispatch_input(input);
                    if let (Some(window), Some(host)) = (&self.window, &self.host) {
                        let cursor = host
                            .cursor_at([logical.x, logical.y])
                            .unwrap_or(CursorKeyword::Default);
                        window.set_cursor_visible(cursor != CursorKeyword::None);
                        if cursor != CursorKeyword::None {
                            window.set_cursor(cursor_icon(cursor));
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(input) = self.pointer.mouse_button(
                    self.started_at.elapsed().as_secs_f64() * 1000.0,
                    mouse_button(button),
                    state == ElementState::Pressed,
                ) {
                    self.dispatch_input(input);
                }
                if button == MouseButton::Left
                    && state == ElementState::Pressed
                    && let (Some(position), Some(host)) =
                        (self.pointer.mouse_position(), self.host.as_mut())
                    && host.focus_text_input_at([position.x, position.y])
                {
                    self.request_frame();
                    self.sync_text_input();
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                if let (Some(window), Some(position), Some(host)) =
                    (&self.window, self.pointer.mouse_position(), &mut self.host)
                {
                    let point = [position.x, position.y];
                    let changed = host.scroll_at(point, scroll_delta(delta, window.scale_factor()));
                    let settled = if phase == TouchPhase::Ended {
                        self.pending_scroll_settle = None;
                        host.settle_scroll_at(point)
                    } else {
                        if changed {
                            self.pending_scroll_settle =
                                Some((Instant::now() + Duration::from_millis(100), point));
                        }
                        false
                    };
                    if changed || settled {
                        self.request_frame();
                    }
                }
            }
            WindowEvent::Touch(touch) => {
                if let Some(window) = &self.window {
                    let logical = touch.location.to_logical::<f32>(window.scale_factor());
                    if let Some(input) = self.pointer.touch(
                        self.started_at.elapsed().as_secs_f64() * 1000.0,
                        touch.id,
                        touch_phase(touch.phase),
                        [logical.x, logical.y],
                    ) {
                        self.dispatch_input(input);
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(window) = &self.window {
                    window.set_cursor_visible(true);
                    window.set_cursor(CursorIcon::Default);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some((deadline, point)) = self.pending_scroll_settle else {
            return;
        };
        if Instant::now() < deadline {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            return;
        }
        self.pending_scroll_settle = None;
        if self
            .host
            .as_mut()
            .is_some_and(|host| host.settle_scroll_at(point))
        {
            self.request_frame();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopAppConfig;

    #[test]
    fn default_config_preserves_the_standalone_window_contract() {
        let config = DesktopAppConfig::new("Whisker");

        assert_eq!(config.title, "Whisker");
        assert_eq!(config.width, 1024.0);
        assert_eq!(config.height, 720.0);
        assert_eq!(config.background_rgb, [255, 255, 255]);
        assert!(config.modules.is_empty());
        assert!(config.element_factories.is_empty());
        assert!(config.module_services.is_empty());
    }

    #[test]
    fn generated_host_can_set_static_background() {
        let config = DesktopAppConfig::new("Whisker").with_background_rgb(16, 16, 24);
        assert_eq!(config.background_rgb, [16, 16, 24]);
    }
}
