//! Native macOS application shell for Whisker.
//!
//! This crate owns macOS window lifecycle and event translation. Measurement,
//! retained scene projection, and `wgpu` painting live in `whisker-desktop`.

#![cfg(target_os = "macos")]
#![warn(missing_docs)]

mod app;

pub use app::{MacosAppConfig, MacosHostError, run};
