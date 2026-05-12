//! C ABI exposed to Swift (iOS) and Kotlin/JNI (Android).
//!
//! Two responsibilities:
//!   1. Implement [`lyra_runtime::renderer::Renderer`] on top of the
//!      `liblyra_bridge` C ABI (declared in [`bridge_ffi`]).
//!   2. Expose Rust entry points (`lyra_mobile_*`) the host Swift/Obj-C
//!      code calls into to bootstrap the runtime.

mod app_logic;
mod bridge_ffi;
mod bridge_renderer;
mod entry;

pub use bridge_renderer::BridgeRenderer;

use std::ffi::c_char;

/// Returns a NUL-terminated UTF-8 greeting from Rust.
///
/// The pointer references static storage and is valid for the lifetime of
/// the loaded library. The caller MUST NOT free it.
#[no_mangle]
pub extern "C" fn lyra_mobile_greeting() -> *const c_char {
    b"Hello from Rust\0".as_ptr() as *const c_char
}
