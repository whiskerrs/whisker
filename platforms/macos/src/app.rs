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
use whisker_protocol::SurfaceId;
use whisker_style::StyleEnvironment;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

/// Configuration for one standalone macOS window.
#[derive(Clone, Debug)]
pub struct MacosAppConfig {
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

impl MacosAppConfig {
    /// Creates the default standalone macOS window configuration.
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

/// Failure while creating or running the native macOS Host.
#[derive(Debug)]
pub struct MacosError(String);

impl fmt::Display for MacosError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for MacosError {}

/// Runs a standalone Whisker application in a native macOS window.
pub fn run(mut config: MacosAppConfig, application: fn() -> Element) -> Result<(), MacosError> {
    let mut element_factories = BuiltInElementModule::definition().into_factories();
    let elements = ElementRegistry::standard_builder()
        .register_modules(config.element_modules.drain(..))
        .build()
        .map_err(|error| MacosError(format!("build element registry: {error}")))?;
    element_factories.extend(
        config
            .module_definitions
            .drain(..)
            .flat_map(DesktopModuleDefinition::into_factories),
    );
    config.element_factories = element_factories;
    let event_loop = EventLoop::<HostEvent>::with_user_event()
        .build()
        .map_err(|error| MacosError(format!("create macOS event loop: {error}")))?;
    let proxy = event_loop.create_proxy();
    let mut application = MacosApplication::new(config, elements, application, proxy);
    event_loop
        .run_app(&mut application)
        .map_err(|error| MacosError(format!("run macOS event loop: {error}")))
}

#[derive(Clone, Copy, Debug)]
enum HostEvent {
    RequestFrame,
}

struct MacosApplication {
    config: MacosAppConfig,
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

impl MacosApplication {
    fn new(
        config: MacosAppConfig,
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

    fn mount(&mut self, event_loop: &ActiveEventLoop) -> Result<(), MacosError> {
        if self.window.is_some() {
            return Ok(());
        }
        let attributes = WindowAttributes::default()
            .with_title(self.config.title.clone())
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| MacosError(format!("create macOS window: {error}")))?,
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
        .map_err(|error| MacosError(error.to_string()))?;
        let mut runtime = RuntimeInstance::new(surface, wake);
        runtime
            .mount(self.application)
            .map_err(|error| MacosError(format!("mount Whisker application: {error}")))?;
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
                eprintln!("whisker macOS frame failed: {error}");
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

impl ApplicationHandler<HostEvent> for MacosApplication {
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
            _ => {}
        }
    }
}
