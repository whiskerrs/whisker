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
    element_ref, BoundingClientRect, ElementHandle, ElementRef, RefError, ScrollInfo,
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

/// Typed timing options for [`animate_start`]. Field names mirror the
/// JS Web-Animations options object (and Lynx's `Element::Animate`
/// `animation_data` table).
#[derive(Clone, Debug)]
pub struct AnimateOptions {
    /// Duration in milliseconds.
    pub duration_ms: u32,
    /// Easing string — `"linear"`, `"ease-in"`, `"ease-out"`,
    /// `"ease-in-out"`, or a `"cubic-bezier(...)"` literal.
    pub easing: String,
    /// Iteration count. Use `f64::INFINITY` for `"infinite"` —
    /// the bridge serialises that through Lynx's special-case.
    pub iterations: f64,
    /// `"normal" | "reverse" | "alternate" | "alternate-reverse"`.
    pub direction: String,
    /// `"none" | "forwards" | "backwards" | "both"`.
    pub fill: String,
    /// Delay before the animation starts, milliseconds.
    pub delay_ms: u32,
}

impl Default for AnimateOptions {
    fn default() -> Self {
        Self {
            duration_ms: 300,
            easing: "linear".into(),
            iterations: 1.0,
            direction: "normal".into(),
            fill: "forwards".into(),
            delay_ms: 0,
        }
    }
}

/// High-level wrapper around [`invoke_element_animate`] for the
/// `START` operation: starts a named animation with explicit
/// keyframes + timing.
///
/// `keyframes` is a slice of `(offset, css_props)` where `offset` is
/// a percent string like `"0%"` / `"50%"` / `"100%"` and `css_props`
/// is the property → value map applied at that frame. Order matches
/// the slice (Lynx is offset-driven, not order-driven, but stable
/// order keeps the serialised JSON readable).
///
/// Returns `Ok(())` on dispatch success; `Err(message)` if the bridge
/// reports a precondition failure.
pub fn animate_start(
    handle: Element,
    animation_name: &str,
    keyframes: &[(&str, &[(&str, &str)])],
    options: &AnimateOptions,
) -> Result<(), String> {
    let kf_map: std::collections::BTreeMap<String, WhiskerValue> = keyframes
        .iter()
        .map(|(offset, props)| {
            let prop_map: std::collections::BTreeMap<String, WhiskerValue> = props
                .iter()
                .map(|(k, v)| ((*k).to_string(), WhiskerValue::String((*v).to_string())))
                .collect();
            ((*offset).to_string(), WhiskerValue::Map(prop_map))
        })
        .collect();
    let kf = WhiskerValue::Map(kf_map);

    let mut opt_map = std::collections::BTreeMap::new();
    opt_map.insert(
        "name".to_string(),
        WhiskerValue::String(animation_name.into()),
    );
    opt_map.insert(
        "duration".to_string(),
        WhiskerValue::Int(options.duration_ms as i64),
    );
    opt_map.insert(
        "easing".to_string(),
        WhiskerValue::String(options.easing.clone()),
    );
    opt_map.insert(
        "iterations".to_string(),
        WhiskerValue::Float(options.iterations),
    );
    opt_map.insert(
        "direction".to_string(),
        WhiskerValue::String(options.direction.clone()),
    );
    opt_map.insert(
        "fill".to_string(),
        WhiskerValue::String(options.fill.clone()),
    );
    opt_map.insert(
        "delay".to_string(),
        WhiskerValue::Int(options.delay_ms as i64),
    );
    let opt = WhiskerValue::Map(opt_map);

    match invoke_element_animate(handle, AnimateOp::Start as i32, animation_name, kf, opt) {
        WhiskerValue::Null => Ok(()),
        WhiskerValue::Error(e) => Err(e),
        other => Err(format!("unexpected animate return: {:?}", other)),
    }
}

/// Cancel a running named animation — drops styles, clears state.
pub fn animate_cancel(handle: Element, animation_name: &str) -> Result<(), String> {
    match invoke_element_animate(
        handle,
        AnimateOp::Cancel as i32,
        animation_name,
        WhiskerValue::Null,
        WhiskerValue::Null,
    ) {
        WhiskerValue::Null => Ok(()),
        WhiskerValue::Error(e) => Err(e),
        other => Err(format!("unexpected animate return: {:?}", other)),
    }
}

/// Lynx-side animation lifecycle operations exposed by
/// `Element::Animate`. Matches `JavaScriptElement::AnimationOperation`
/// in the Lynx fork — same numeric values, same semantics.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AnimateOp {
    /// Start (or restart) a named animation with keyframes + options.
    Start = 0,
    /// Resume a paused animation.
    Play = 1,
    /// Pause a running animation in place.
    Pause = 2,
    /// Cancel — clear styles, drop the animation entirely.
    Cancel = 3,
    /// Snap to the animation's end state and stop.
    Finish = 4,
}

/// Element-level animation dispatch — wraps `lynx_element_animate`
/// via [`whisker_bridge_element_animate`](ffi::whisker_bridge_element_animate).
///
/// This is the DOM-layer animation entry point (`element.animate(...)` in
/// JS), distinct from [`invoke_element_method_with_params`] which targets
/// the UI layer below. `operation` follows the Lynx enum:
///
///   0 = START, 1 = PLAY, 2 = PAUSE, 3 = CANCEL, 4 = FINISH.
///
/// For `START` the caller must supply `keyframes` (`WhiskerValue::Map` of
/// `"0%" / "50%" / "100%"` → CSS-prop map) and `options` (`WhiskerValue::Map`
/// of `name` / `duration` / `easing` / `iterations` / `direction` / `fill` /
/// `delay`). Other operations only consult `animation_name` — pass
/// [`WhiskerValue::Null`] for the rest.
///
/// Returns [`WhiskerValue::Null`] on dispatch success;
/// [`WhiskerValue::Error`] on precondition failure.
pub fn invoke_element_animate(
    handle: Element,
    operation: i32,
    animation_name: &str,
    keyframes: WhiskerValue,
    options: WhiskerValue,
) -> WhiskerValue {
    let ptr_usize = module_component_ptr(handle);
    if ptr_usize == 0 {
        return WhiskerValue::Error(format!(
            "invoke_element_animate: no platform component for handle {} \
             (renderer not installed, or element released)",
            handle.id()
        ));
    }
    let name_c = match CString::new(animation_name) {
        Ok(c) => c,
        Err(_) => return WhiskerValue::Error("animation_name contained NUL byte".into()),
    };

    let mut builder = RawBuilder::default();
    let raw_kf = builder.encode(&keyframes);
    let raw_opt = builder.encode(&options);
    let kf_ptr = match keyframes {
        WhiskerValue::Null => std::ptr::null(),
        _ => &raw_kf as *const _,
    };
    let opt_ptr = match options {
        WhiskerValue::Null => std::ptr::null(),
        _ => &raw_opt as *const _,
    };

    let raw_result = unsafe {
        ffi::whisker_bridge_element_animate(
            ptr_usize as *mut WhiskerElement,
            operation,
            name_c.as_ptr(),
            kf_ptr,
            opt_ptr,
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
