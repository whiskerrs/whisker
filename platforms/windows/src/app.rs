use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use whisker::runtime::RuntimeWakeHandle;
use whisker::{Element, ElementModuleDefinition, ElementRegistry, RuntimeInstance, SurfaceRuntime};
use whisker_desktop::{
    BuiltInElementModule, DesktopElementFactory, DesktopFrameContext, DesktopModuleDefinition,
    DesktopRuntime, WhiskerModule,
};
use whisker_protocol::{CursorKeyword, SurfaceId};
use whisker_style::StyleEnvironment;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{CursorIcon, Window, WindowAttributes, WindowId};

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

/// Configuration for one standalone Windows window.
#[derive(Clone, Debug)]
pub struct WindowsAppConfig {
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
}

impl WindowsAppConfig {
    /// Creates the default standalone Windows window configuration.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 1024.0,
            height: 720.0,
            module_definitions: Vec::new(),
            element_modules: Vec::new(),
            element_factories: Vec::new(),
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

/// Failure while creating or running the native Windows Host.
#[derive(Debug)]
pub struct WindowsError(String);

impl fmt::Display for WindowsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for WindowsError {}

/// Runs a standalone Whisker application in a native Windows window.
pub fn run(mut config: WindowsAppConfig, application: fn() -> Element) -> Result<(), WindowsError> {
    let mut element_factories = BuiltInElementModule::definition().into_factories();
    let elements = ElementRegistry::standard_builder()
        .register_modules(config.element_modules.drain(..))
        .build()
        .map_err(|error| WindowsError(format!("build element registry: {error}")))?;
    element_factories.extend(
        config
            .module_definitions
            .drain(..)
            .flat_map(DesktopModuleDefinition::into_factories),
    );
    config.element_factories = element_factories;
    let event_loop = EventLoop::<HostEvent>::with_user_event()
        .build()
        .map_err(|error| WindowsError(format!("create Windows event loop: {error}")))?;
    let proxy = event_loop.create_proxy();
    let mut application = WindowsApplication::new(config, elements, application, proxy);
    event_loop
        .run_app(&mut application)
        .map_err(|error| WindowsError(format!("run Windows event loop: {error}")))
}

#[derive(Clone, Copy, Debug)]
enum HostEvent {
    RequestFrame,
}

struct WindowsApplication {
    config: WindowsAppConfig,
    elements: ElementRegistry,
    application: fn() -> Element,
    proxy: EventLoopProxy<HostEvent>,
    window: Option<Arc<Window>>,
    runtime: Option<RuntimeInstance>,
    host: Option<DesktopRuntime>,
    viewport: PhysicalSize<u32>,
    viewport_epoch: u32,
    environment_epoch: u64,
    started_at: Instant,
    frame_failed: bool,
}

impl WindowsApplication {
    fn new(
        config: WindowsAppConfig,
        elements: ElementRegistry,
        application: fn() -> Element,
        proxy: EventLoopProxy<HostEvent>,
    ) -> Self {
        Self {
            config,
            elements,
            application,
            proxy,
            window: None,
            runtime: None,
            host: None,
            viewport: PhysicalSize::new(1, 1),
            viewport_epoch: 1,
            environment_epoch: 1,
            started_at: Instant::now(),
            frame_failed: false,
        }
    }

    fn mount(&mut self, event_loop: &ActiveEventLoop) -> Result<(), WindowsError> {
        if self.window.is_some() {
            return Ok(());
        }
        let attributes = WindowAttributes::default()
            .with_title(self.config.title.clone())
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| WindowsError(format!("create Windows window: {error}")))?,
        );
        self.viewport = window.inner_size();
        let scale = window.scale_factor() as f32;
        let logical = self.viewport.to_logical::<f32>(window.scale_factor());
        let surface_id = SurfaceId::new(1).expect("the standalone surface id is non-zero");
        let surface = SurfaceRuntime::with_element_registry(
            surface_id,
            StyleEnvironment::new(logical.width, logical.height, scale, 14.0),
            self.elements.clone(),
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
            wake.clone(),
        ))
        .map_err(|error| WindowsError(error.to_string()))?;
        let mut runtime = RuntimeInstance::new(surface, wake);
        runtime
            .mount(self.application)
            .map_err(|error| WindowsError(format!("mount Whisker application: {error}")))?;
        self.host = Some(host);
        self.runtime = Some(runtime);
        self.window = Some(window);
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
            (&self.window, &self.runtime, &mut self.host)
        else {
            return;
        };
        let scale = window.scale_factor();
        let logical = self.viewport.to_logical::<f32>(scale);
        match host.drive_frame(
            runtime,
            DesktopFrameContext {
                timestamp_ms: self.started_at.elapsed().as_secs_f64() * 1000.0,
                logical_width: logical.width,
                logical_height: logical.height,
                scale: scale as f32,
                environment_epoch: self.environment_epoch,
                viewport_epoch: self.viewport_epoch,
            },
        ) {
            Ok(result) => {
                if result.needs_frame {
                    window.request_redraw();
                }
            }
            Err(error) => {
                self.frame_failed = true;
                eprintln!("whisker Windows frame failed: {error}");
            }
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
}

impl ApplicationHandler<HostEvent> for WindowsApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.mount(event_loop) {
            eprintln!("{error}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: HostEvent) {
        match event {
            HostEvent::RequestFrame => self.request_frame(),
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
        match event {
            WindowEvent::CloseRequested => {
                if let Some(runtime) = &mut self.runtime {
                    let _ = runtime.unmount();
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
                if let (Some(window), Some(host)) = (&self.window, &self.host) {
                    let logical = position.to_logical::<f32>(window.scale_factor());
                    let cursor = host
                        .cursor_at([logical.x, logical.y])
                        .unwrap_or(CursorKeyword::Default);
                    window.set_cursor_visible(cursor != CursorKeyword::None);
                    if cursor != CursorKeyword::None {
                        window.set_cursor(cursor_icon(cursor));
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
}
