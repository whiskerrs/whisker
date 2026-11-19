//! Native WinUI 3 Windows Host for Whisker.
//!
//! The C++ Host (`Whisker.WindowsHost` on NuGet, sources under `host/`) drives
//! the application through the mobile C ABI that `#[whisker::main]` exports;
//! this crate links it and opens the window.

#![cfg(target_os = "windows")]
#![warn(missing_docs)]

use std::error::Error;
use std::fmt;

use whisker::runtime::module::{RustModuleDefinition, link_rust_modules};
use whisker::{Element, ElementModuleDefinition};

/// Configuration for one standalone Windows window.
#[derive(Clone, Debug)]
pub struct WindowsAppConfig {
    title: String,
    background_rgb: [u8; 3],
    modules: Vec<WindowsModuleDefinition>,
}

/// Windows implementation of one module, as returned by its Host crate.
#[derive(Clone, Debug, Default)]
pub struct WindowsModuleDefinition {
    service: RustModuleDefinition,
}

impl From<RustModuleDefinition> for WindowsModuleDefinition {
    fn from(service: RustModuleDefinition) -> Self {
        Self { service }
    }
}

impl WindowsAppConfig {
    /// Creates the default window configuration.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            background_rgb: [255, 255, 255],
            modules: Vec::new(),
        }
    }

    /// Sets the window background shown behind Whisker content.
    pub fn with_background_rgb(mut self, red: u8, green: u8, blue: u8) -> Self {
        self.background_rgb = [red, green, blue];
        self
    }

    /// Installs one module's element schema and Host implementation.
    pub fn with_module(
        mut self,
        _elements: ElementModuleDefinition,
        host: impl Into<WindowsModuleDefinition>,
    ) -> Self {
        self.modules.push(host.into());
        self
    }
}

/// Failure while creating or running the WinUI Host.
#[derive(Debug)]
pub struct WindowsError(i32);

impl fmt::Display for WindowsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Windows Host failed (HRESULT {:#010x})", self.0)
    }
}

impl Error for WindowsError {}

#[repr(C)]
struct WhiskerWindowsAppOptions {
    title: *const u8,
    title_len: usize,
    background_rgb: [u8; 3],
}

unsafe extern "C" {
    fn whisker_windows_run(options: *const WhiskerWindowsAppOptions) -> i32;
}

/// Runs the Whisker application in a WinUI window until the window closes.
///
/// The Host reaches the application through the C ABI that
/// `#[whisker::main]` exports, so `application` only fixes which crate links.
pub fn run(config: WindowsAppConfig, application: fn() -> Element) -> Result<(), WindowsError> {
    run_with_application_hash(config, application, || 0)
}

/// Runs a Whisker application; the hash is reserved for Hot Reload.
pub fn run_with_application_hash(
    config: WindowsAppConfig,
    _application: fn() -> Element,
    _application_hash: fn() -> u64,
) -> Result<(), WindowsError> {
    link_rust_modules(config.modules.into_iter().map(|module| module.service));
    let options = WhiskerWindowsAppOptions {
        title: config.title.as_ptr(),
        title_len: config.title.len(),
        background_rgb: config.background_rgb,
    };
    // SAFETY: `options` and the title it borrows outlive the call, which blocks until the window closes.
    let result = unsafe { whisker_windows_run(&options) };
    if result < 0 {
        Err(WindowsError(result))
    } else {
        Ok(())
    }
}
