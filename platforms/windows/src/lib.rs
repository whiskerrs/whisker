//! Native WinUI 3 Windows Host for Whisker.
//!
//! The C++ Host (`Whisker.WindowsHost` on NuGet, sources under `host/`) drives
//! the application through the mobile C ABI that `#[whisker::main]` exports;
//! this crate links it and opens the window.

#![cfg(target_os = "windows")]
#![warn(missing_docs)]

use std::error::Error;
use std::fmt;

use whisker::Element;

/// Configuration for one standalone Windows window.
#[derive(Clone, Debug)]
pub struct WindowsAppConfig {
    title: String,
    background_rgb: [u8; 3],
}

impl WindowsAppConfig {
    /// Creates the default window configuration.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            background_rgb: [255, 255, 255],
        }
    }

    /// Sets the window background shown behind Whisker content.
    pub fn with_background_rgb(mut self, red: u8, green: u8, blue: u8) -> Self {
        self.background_rgb = [red, green, blue];
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
