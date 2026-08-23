//! Native Windows application shell for Whisker.
//!
//! This crate owns Windows window lifecycle and event translation.
//! Measurement, retained scene projection, and `wgpu` painting live in
//! `whisker-desktop`.

#![cfg(target_os = "windows")]
#![warn(missing_docs)]

mod app;

pub use app::{WindowsAppConfig, WindowsError, run};
