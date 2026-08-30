//! Native macOS application shell for Whisker.
//!
//! The shared `winit` lifecycle, event translation, retained scene projection,
//! and `wgpu` painting live in `whisker-desktop`. This target crate preserves
//! the macOS-named composition interface and is the seam for future native
//! macOS integrations.

#![cfg(target_os = "macos")]
#![warn(missing_docs)]

/// Configuration for one standalone macOS window.
pub type MacosAppConfig = whisker_desktop::DesktopAppConfig;
/// Failure while creating or running the native macOS Host.
pub type MacosError = whisker_desktop::DesktopAppError;
pub use whisker_desktop::run;
