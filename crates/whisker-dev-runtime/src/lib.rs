//! Development-only runtime extensions for Whisker.
//!
//! Compiled into Whisker apps only when the umbrella crate is built
//! with `--features hot-reload`. Release builds end up with an empty
//! crate (no tokio / no WebSocket / no subsecond).

#[cfg(feature = "hot-reload")]
pub mod hot_reload;

#[cfg(feature = "hot-reload")]
pub mod log_capture;

#[cfg(feature = "hot-reload")]
pub use hot_reload::{
    NativeCodeUpdate, NativeHotReload, NativeHotReloadRegistration, devlog,
    register_native_runtime, start_receiver,
};

#[cfg(feature = "hot-reload")]
pub use log_capture::start_log_capture;
