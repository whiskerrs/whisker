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

pub use element_ref::{
    element_ref, BoundingClientRect, ElementHandle, ElementRef, ImageHandle, RefError, ScrollInfo,
    ScrollViewHandle, TextBoundingRect, TextHandle, UiInfo,
};
pub use lynx::bootstrap;
pub use lynx::renderer::BridgeRenderer;

use std::ffi::{c_void, CString};

use whisker_driver_sys as ffi;
use whisker_driver_sys::WhiskerElement;
use whisker_runtime::view::{module_component_ptr, Element};

use crate::module::{async_trampoline, from_raw, RawBuilder, WhiskerValue};

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
    let ptr_usize = module_component_ptr(handle);
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

/// Fire-and-forget invoke of a built-in Lynx UI method whose arguments
/// are read as *named fields* of the params object (`scrollTo`,
/// `scrollBy`, `autoScroll`, `scrollIntoView`, …) rather than from the
/// `{"args": […]}` wrapper [`invoke_element_method`] builds for Whisker
/// module methods. `params` is a single `WhiskerValue` — typically a
/// [`WhiskerValue::map`] — passed through (with any nested maps /
/// arrays) as the params object directly. Routes through
/// `whisker_bridge_invoke_element_method_with_params`.
pub fn invoke_element_method_with_params(
    handle: Element,
    method: &str,
    params: WhiskerValue,
) -> WhiskerValue {
    let ptr_usize = module_component_ptr(handle);
    if ptr_usize == 0 {
        return WhiskerValue::Error(format!(
            "invoke_element_method_with_params({method}): no platform component \
             for handle {} (renderer not installed, or element released)",
            handle.id()
        ));
    }
    let method_c = match CString::new(method) {
        Ok(c) => c,
        Err(_) => return WhiskerValue::Error("method name contained NUL byte".into()),
    };

    let mut builder = RawBuilder::default();
    let raw_params = builder.encode(&params);

    let raw_result = unsafe {
        ffi::whisker_bridge_invoke_element_method_with_params(
            ptr_usize as *mut WhiskerElement,
            method_c.as_ptr(),
            &raw_params as *const _,
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

/// Async, **result-returning** element-method invoke — the path for
/// `boundingClientRect` / `takeScreenshot` etc., whose return value
/// arrives via Lynx's UI-method callback (typically on the UI thread)
/// rather than synchronously. Resolves once the bridge fires the C
/// callback.
///
/// The bridge invokes the callback exactly once (synchronously with a
/// `WhiskerValue::Error` on a precondition / unsupported-platform
/// failure, asynchronously with the result on success), so the boxed
/// oneshot sender is always consumed by [`async_trampoline`] — the
/// caller never recovers it.
pub async fn invoke_element_method_async(
    handle: Element,
    method: &str,
    args: Vec<WhiskerValue>,
) -> WhiskerValue {
    let ptr_usize = module_component_ptr(handle);
    if ptr_usize == 0 {
        return WhiskerValue::Error(format!(
            "invoke_element_method_async({method}): no platform component for handle {} \
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

    let (tx, rx) = futures_channel::oneshot::channel::<WhiskerValue>();
    let tx_box: Box<Option<futures_channel::oneshot::Sender<WhiskerValue>>> = Box::new(Some(tx));
    let tx_ptr = Box::into_raw(tx_box) as *mut c_void;

    // Return value is advisory — `async_trampoline` always consumes the
    // boxed sender (the bridge guarantees one callback either way), so
    // we just await the channel.
    let _scheduled = unsafe {
        ffi::whisker_bridge_invoke_element_method_async(
            ptr_usize as *mut WhiskerElement,
            method_c.as_ptr(),
            if raw_args.is_empty() {
                std::ptr::null()
            } else {
                raw_args.as_ptr()
            },
            raw_args.len(),
            async_trampoline,
            tx_ptr,
        )
    };
    drop(builder);

    rx.await
        .unwrap_or_else(|_| WhiskerValue::Error("element-method async callback never fired".into()))
}

/// The unified element-method dispatch: `params` (a single
/// `WhiskerValue`, typically a [`WhiskerValue::map`]) is passed through
/// as the method's params object directly, and the result arrives via
/// the async callback. This is what [`ElementRef::invoke`] /
/// [`invoke_typed`](crate::ElementRef::invoke_typed) build on — both
/// built-in Lynx methods (named-field params) and Whisker module
/// elements (the caller builds `{"args": […]}`). Routes through
/// `whisker_bridge_invoke_element_method_async_with_params`.
pub async fn invoke_element_method_async_with_params(
    handle: Element,
    method: &str,
    params: WhiskerValue,
) -> WhiskerValue {
    let ptr_usize = module_component_ptr(handle);
    if ptr_usize == 0 {
        return WhiskerValue::Error(format!(
            "invoke_element_method_async_with_params({method}): no platform component \
             for handle {} (renderer not installed, or element released)",
            handle.id()
        ));
    }
    let method_c = match CString::new(method) {
        Ok(c) => c,
        Err(_) => return WhiskerValue::Error("method name contained NUL byte".into()),
    };

    let mut builder = RawBuilder::default();
    let raw_params = builder.encode(&params);

    let (tx, rx) = futures_channel::oneshot::channel::<WhiskerValue>();
    let tx_box: Box<Option<futures_channel::oneshot::Sender<WhiskerValue>>> = Box::new(Some(tx));
    let tx_ptr = Box::into_raw(tx_box) as *mut c_void;

    let _scheduled = unsafe {
        ffi::whisker_bridge_invoke_element_method_async_with_params(
            ptr_usize as *mut WhiskerElement,
            method_c.as_ptr(),
            &raw_params as *const _,
            async_trampoline,
            tx_ptr,
        )
    };
    drop(builder);

    rx.await
        .unwrap_or_else(|_| WhiskerValue::Error("element-method async callback never fired".into()))
}
