//! Lynx backend.
//!
//! Wraps the raw C ABI in `whisker-driver-sys` with the safe
//! [`renderer::BridgeRenderer`] and exposes the host shim entry points
//! in [`bootstrap`].

pub mod bootstrap;
pub mod renderer;
