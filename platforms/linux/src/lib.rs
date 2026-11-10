//! Native GTK4 Linux Host for Whisker.
//!
//! Each scene node is a GTK widget placed with Rust-computed geometry and
//! painted through `gtk::Snapshot`; text is measured and drawn with Pango.

#![cfg(target_os = "linux")]
#![warn(missing_docs)]

use std::error::Error;
use std::fmt;

use whisker::Element;

mod app;
mod color;
mod cursor;
mod node;
mod paint;
mod scene;
mod text;

/// Configuration for one standalone Linux window.
#[derive(Clone, Debug)]
pub struct LinuxAppConfig {
    title: String,
    width: i32,
    height: i32,
    background_rgb: [u8; 3],
}

impl LinuxAppConfig {
    /// Creates the default window configuration.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 1024,
            height: 720,
            background_rgb: [255, 255, 255],
        }
    }

    /// Sets the window background shown behind Whisker content.
    pub fn with_background_rgb(mut self, red: u8, green: u8, blue: u8) -> Self {
        self.background_rgb = [red, green, blue];
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
