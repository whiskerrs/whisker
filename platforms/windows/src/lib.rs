//! Native Windows application shell for Whisker.
//!
//! The shared `winit` lifecycle, event translation, retained scene projection,
//! and `wgpu` painting live in `whisker-desktop`. This target crate preserves
//! the Windows-named composition interface and is the seam for future native
//! Windows integrations.

#![cfg(target_os = "windows")]
#![warn(missing_docs)]

/// Configuration for one standalone Windows window.
pub type WindowsAppConfig = whisker_desktop::DesktopAppConfig;
/// Failure while creating or running the native Windows Host.
pub type WindowsError = whisker_desktop::DesktopAppError;
pub use whisker_desktop::{run, run_with_application_hash};
