//! Safe FFI Host adapter for Whisker's retained Rust runtime.
//!
//! Android and iOS link this crate at their native boundary. It owns the
//! runtime instance behind an opaque C handle, translates borrowed ABI values,
//! and adapts native callbacks to core's [`whisker_runtime`] Host interfaces.
//! Rust-native Hosts such as Web and Desktop use those interfaces directly and
//! do not depend on this crate.

#[doc(hidden)]
pub mod abi;
#[doc(hidden)]
pub mod ffi_module;
#[doc(hidden)]
pub mod ffi_runtime;

/// Forces Android's JNI glue archive into the final application library.
#[cfg(target_os = "android")]
#[doc(hidden)]
pub fn ensure_mobile_bridge_linked() {
    unsafe { whisker_driver_sys::whisker_mobile_bridge_anchor() }
}

#[cfg(not(target_os = "android"))]
#[doc(hidden)]
pub fn ensure_mobile_bridge_linked() {}
