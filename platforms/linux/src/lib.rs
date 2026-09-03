//! Native Linux application shell for Whisker.
//!
//! The shared `winit` lifecycle, event translation, retained scene projection,
//! and `wgpu` painting live in `whisker-desktop`. This target crate preserves
//! the Linux-named composition interface and is the seam for future native
//! Linux integrations.

#![cfg(target_os = "linux")]
#![warn(missing_docs)]

/// Configuration for one standalone Linux window.
pub type LinuxAppConfig = whisker_desktop::DesktopAppConfig;
/// Failure while creating or running the native Linux Host.
pub type LinuxError = whisker_desktop::DesktopAppError;
pub use whisker_desktop::{run, run_with_application_hash};
