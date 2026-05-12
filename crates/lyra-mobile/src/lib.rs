//! C ABI exposed to Swift (iOS) and Kotlin/JNI (Android).
//!
//! Functions here are the entry points the native runtime libraries call.
//! Keep the surface narrow and the lifetimes obvious — every pointer that
//! crosses the FFI boundary needs a documented owner.

use std::ffi::c_char;

/// Returns a NUL-terminated UTF-8 greeting from Rust.
///
/// The pointer references static storage and is valid for the lifetime of
/// the loaded library. The caller MUST NOT free it.
#[no_mangle]
pub extern "C" fn lyra_mobile_greeting() -> *const c_char {
    // Static C-string literal: NUL-terminated, 'static lifetime, never freed.
    b"Hello from Rust\0".as_ptr() as *const c_char
}
