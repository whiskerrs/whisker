//! Native Linux application shell for Whisker.
//!
//! This crate owns Linux window lifecycle and event translation. Measurement,
//! retained scene projection, and `wgpu` painting live in `whisker-desktop`.

#![cfg(target_os = "linux")]
#![warn(missing_docs)]

mod app;

pub use app::{LinuxAppConfig, LinuxHostError, run};
