use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::accessibility::{DesktopAccessibilityAction, DesktopAccessibilityBridge};
use crate::{
    BuiltInElementModule, DesktopElementFactory, DesktopFrameContext, DesktopModuleDefinition,
    DesktopMouseButton, DesktopPointerAdapter, DesktopPointerPhase, DesktopRuntime, WhiskerModule,
};
use whisker::runtime::RuntimeWakeHandle;
use whisker::runtime::module::RustModuleDefinition;
use whisker::{Element, ElementModuleDefinition, ElementRegistry, RuntimeInstance, SurfaceRuntime};
use whisker_protocol::{CursorKeyword, InputEvent, InputEventKind, SurfaceId, WhiskerValue};
use whisker_style::StyleEnvironment;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
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
    /// Element modules selected for this target.
    pub module_definitions: Vec<DesktopModuleDefinition>,
    /// Host-independent element schemas selected from Rust module crates.
    pub element_modules: Vec<ElementModuleDefinition>,
    element_factories: Vec<DesktopElementFactory>,
    module_services: Vec<RustModuleDefinition>,
}

impl DesktopAppConfig {
    /// Creates the default standalone Desktop window configuration.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 1024.0,
            height: 720.0,
            module_definitions: Vec::new(),
            element_modules: Vec::new(),
            element_factories: Vec::new(),
            module_services: Vec::new(),
        }
    }

    /// Adds one Rust element definition with its matching Desktop factory.
    pub fn with_module_definition(mut self, definition: DesktopModuleDefinition) -> Self {
        self.module_definitions.push(definition);
        self
    }

    /// Adds one Host-independent Rust element module for bootstrap negotiation.
    pub fn with_element_module(mut self, definition: ElementModuleDefinition) -> Self {
        self.element_modules.push(definition);
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
        .register_modules(config.element_modules.drain(..))
        .build()
        .map_err(|error| DesktopAppError(format!("build element registry: {error}")))?;
    for definition in config.module_definitions.drain(..) {
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
        let host = pollster::block_on(DesktopRuntime::new(
            window.clone(),
            [self.viewport.width, self.viewport.height],
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
        self.window
            .as_ref()
            .expect("mounted Desktop window")
            .set_visible(true);
        self.request_frame();
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
        assert!(config.module_definitions.is_empty());
        assert!(config.element_modules.is_empty());
        assert!(config.element_factories.is_empty());
        assert!(config.module_services.is_empty());
    }
}
