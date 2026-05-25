//! Backend driver for Whisker.
//!
//! Today there is one backend ([`lynx`]) that talks to the C++ Lynx
//! bridge shipped under `whisker-driver-sys/bridge/`. Future backends
//! (web, wgpu, …) would
//! land as sibling modules behind cfg gates. Users never touch this
//! crate directly — `#[whisker::main]` re-exports the `run` / `tick`
//! helpers from here as `whisker::__main_runtime::{run,tick}`.

pub mod element_ref;
pub mod lynx;
pub mod module;

pub use element_ref::{element_ref, ElementRef, RefError};
pub use lynx::bootstrap;
pub use lynx::renderer::BridgeRenderer;

use std::ffi::CString;

use whisker_driver_sys as ffi;
use whisker_driver_sys::WhiskerElement;
use whisker_runtime::view::{platform_component_ptr, Element};

use crate::module::{from_raw, RawBuilder, WhiskerValue};

/// Synchronously invoke `method` on the platform component identified
/// by `handle`. Routes through the C bridge
/// (`whisker_bridge_invoke_element_method`) — itself a stub in
/// Phase 7-Φ.H.2.5, returning a Lynx-fork-pending Error until
/// Phase 7-Φ.H.2.7 wires in the real dispatch.
///
/// Direct callers are mostly the proc macros — `ElementRef::invoke`
/// and the `#[whisker::element_methods]`-emitted bodies. User code
/// uses the typed wrappers, not this entry point.
pub fn invoke_element_method(
    handle: Element,
    method: &str,
    args: Vec<WhiskerValue>,
) -> WhiskerValue {
    let ptr_usize = platform_component_ptr(handle);
    if ptr_usize == 0 {
        return WhiskerValue::Error(format!(
            "invoke_element_method({method}): no platform component for handle {} \
             (renderer not installed, or element released)",
            handle.id()
        ));
    }
    let method_c = match CString::new(method) {
        Ok(c) => c,
        Err(_) => return WhiskerValue::Error("method name contained NUL byte".into()),
    };

    let mut builder = RawBuilder::default();
    let raw_args: Vec<ffi::WhiskerValueRaw> = args.iter().map(|v| builder.encode(v)).collect();

    let raw_result = unsafe {
        ffi::whisker_bridge_invoke_element_method(
            ptr_usize as *mut WhiskerElement,
            method_c.as_ptr(),
            if raw_args.is_empty() {
                std::ptr::null()
            } else {
                raw_args.as_ptr()
            },
            raw_args.len(),
        )
    };

    let result = unsafe { from_raw(&raw_result) };
    unsafe {
        let mut mutable = raw_result;
        ffi::whisker_bridge_value_release(&mut mutable as *mut _);
    }
    drop(builder);
    result
}
