//! Thin macOS adapter for Whisker's shared native Desktop Host.
//!
//! Window lifecycle, retained scene projection, intrinsic text measurement,
//! and `wgpu` painting live in `whisker-desktop`. This crate remains the
//! stable composition dependency for CNG-generated `gen/macos` applications
//! and is the home for genuinely macOS-specific services as they are added.

#![cfg(target_os = "macos")]
#![warn(missing_docs)]

pub use whisker_desktop::{DesktopAppConfig as MacosAppConfig, DesktopHostError as MacosHostError};

use whisker::Element;

/// Runs a standalone Whisker application in a native macOS window.
pub fn run(config: MacosAppConfig, application: fn() -> Element) -> Result<(), MacosHostError> {
    whisker_desktop::run(config, application)
}
