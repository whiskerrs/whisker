//! Native GTK4 Linux Host for Whisker.
//!
//! Each scene node is a GTK widget placed with Rust-computed geometry and
//! painted through `gtk::Snapshot`; text is measured and drawn with Pango.

#![cfg(target_os = "linux")]
#![warn(missing_docs)]

use std::error::Error;
use std::fmt;

use whisker::runtime::module::RustModuleDefinition;
use whisker::{Element, ElementModuleDefinition};

mod app;
mod color;
mod cursor;
mod node;
mod paint;
mod scene;
mod text;

#[cfg(all(test, feature = "host-conformance"))]
#[path = "../tests/host_conformance/mod.rs"]
mod host_conformance_tests;

/// Configuration for one standalone Linux window.
#[derive(Clone, Debug)]
pub struct LinuxAppConfig {
    title: String,
    width: i32,
    height: i32,
    background_rgb: [u8; 3],
    modules: Vec<LinuxModuleDefinition>,
}

/// Linux implementation of one module, as returned by its Host crate.
#[derive(Clone, Debug, Default)]
pub struct LinuxModuleDefinition {
    pub(crate) service: RustModuleDefinition,
}

impl From<RustModuleDefinition> for LinuxModuleDefinition {
    fn from(service: RustModuleDefinition) -> Self {
        Self { service }
    }
}

impl LinuxAppConfig {
    /// Creates the default window configuration.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 1024,
            height: 720,
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
        host: impl Into<LinuxModuleDefinition>,
    ) -> Self {
        self.modules.push(host.into());
        self
    }
}

/// Failure while creating or running the GTK Host.
#[derive(Debug)]
pub struct LinuxError(pub(crate) String);

impl fmt::Display for LinuxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for LinuxError {}

/// Runs a Whisker application in a GTK window until the window closes.
pub fn run(config: LinuxAppConfig, application: fn() -> Element) -> Result<(), LinuxError> {
    run_with_application_hash(config, application, || 0)
}

/// Runs a Whisker application; the hash is reserved for Hot Reload.
pub fn run_with_application_hash(
    config: LinuxAppConfig,
    application: fn() -> Element,
    _application_hash: fn() -> u64,
) -> Result<(), LinuxError> {
    app::run_application(config, application)
}
