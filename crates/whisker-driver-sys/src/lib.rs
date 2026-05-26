//! Raw `extern "C"` declarations matching `bridge/include/whisker_bridge.h`.
//!
//! Everything here is `unsafe` to call. Safe wrappers (and the host shim
//! `whisker_app_main` / `whisker_tick` exports) live in `whisker-driver`. Users
//! never depend on this crate directly.

use std::ffi::{c_char, c_void};

#[repr(C)]
pub struct WhiskerEngine {
    _private: [u8; 0],
}

#[repr(C)]
pub struct WhiskerElement {
    _private: [u8; 0],
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum WhiskerElementTag {
    Page = 1,
    View = 2,
    Text = 3,
    RawText = 4,
    Image = 5,
    ScrollView = 6,
}

pub type WhiskerTasmCallback = extern "C" fn(user_data: *mut c_void);
pub type WhiskerEventCallback = extern "C" fn(user_data: *mut c_void);
/// Value-payload event callback. `payload` is a `WhiskerValueRaw`
/// tree (never NULL — the bridge normalises a missing body to a
/// `WHISKER_VALUE_NULL` value), owned by the bridge and only valid
/// for the duration of the call (copy out via `from_raw`). See
/// `whisker_bridge_set_event_listener_with_value`.
pub type WhiskerEventValueCallback =
    extern "C" fn(user_data: *mut c_void, payload: *const WhiskerValueRaw);

// ----- Platform module invocation (Phase 7-Φ.E) ------------------------------
//
// `#[repr(C)]` mirror of the C tagged-union in `whisker_bridge.h`.
// Each variant has its own pure-Rust struct so the layout matches
// the C compiler's union member layout byte-for-byte — without the
// opaque-storage approach the E.1 draft tried (which silently
// disagreed on total size with the C side).
//
// Native callers (Rust runtime, proc-macro-generated proxies)
// don't touch this `Raw` form directly — `whisker-runtime::view::
// module` exposes a typed `WhiskerValue` enum with conversions in
// both directions.

/// Discriminant for [`WhiskerValueRaw::type_`]. Must stay in lock
/// step with `enum WhiskerValueType` in `whisker_bridge.h`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiskerValueType {
    Null = 0,
    Bool = 1,
    Int = 2,
    Float = 3,
    String = 4,
    Bytes = 5,
    Array = 6,
    Map = 7,
    Error = 8,
}

/// `struct { const char* ptr; size_t len; }` member of the C union
/// (also used as the `key` field of `WhiskerKeyValueRaw`).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerStringRef {
    pub ptr: *const c_char,
    pub len: usize,
}

/// `struct { const uint8_t* ptr; size_t len; }` member.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerBytesRef {
    pub ptr: *const u8,
    pub len: usize,
}

/// `struct WhiskerValueArrayRec` — pointer to `count`
/// `WhiskerValueRaw` items.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerValueArray {
    pub items: *mut WhiskerValueRaw,
    pub count: usize,
}

/// `struct WhiskerValueMapRec` — pointer to `count`
/// `WhiskerKeyValueRaw` entries.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerValueMap {
    pub entries: *mut WhiskerKeyValueRaw,
    pub count: usize,
}

/// `#[repr(C)] union` mirroring the inner anonymous union of
/// `WhiskerValueRec`. Field access is unsafe — discriminate on the
/// outer struct's [`type_`](WhiskerValueRaw::type_) before reading.
#[repr(C)]
#[derive(Copy, Clone)]
pub union WhiskerValueUnion {
    pub b: bool,
    pub i: i64,
    pub f: f64,
    pub s: WhiskerStringRef,
    pub bytes: WhiskerBytesRef,
    pub array: WhiskerValueArray,
    pub map: WhiskerValueMap,
}

/// Raw FFI form of `WhiskerValue` — byte-for-byte compatible with
/// the C `struct WhiskerValueRec`. Total size = 24 bytes
/// (1 discriminant + 7 padding + 16 union = 24).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerValueRaw {
    /// Discriminant — cast to [`WhiskerValueType`] before use.
    pub type_: u8,
    /// Padding to align the union on the natural 8-byte boundary
    /// the C compiler picks for `{ptr, len}` members.
    pub _pad: [u8; 7],
    /// Variant payload — see [`WhiskerValueUnion`].
    pub v: WhiskerValueUnion,
}

/// `struct WhiskerKeyValueRec` — string-keyed entry of the `map`
/// variant.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerKeyValueRaw {
    pub key: WhiskerStringRef,
    pub value: WhiskerValueRaw,
}

/// Callback type for `whisker_bridge_invoke_module_async`. The
/// `result` pointer is borrowed for the duration of the call only —
/// the bridge frees the underlying allocations once the callback
/// returns, so closures that need to retain the value must copy
/// into Rust-owned storage via the `whisker-runtime` wrapper.
pub type WhiskerModuleCallback =
    extern "C" fn(user_data: *mut c_void, result: *const WhiskerValueRaw);

/// Per-module dispatch function — the platform-side Swift Macro or
/// KSP processor emits one of these per `@WhiskerModule`-annotated
/// class. The bridge stores `(module_name → dispatch_fn)` in a
/// lookup table; `whisker_bridge_invoke_module` then routes calls
/// through the registered function directly (Phase 7-Φ.F).
pub type WhiskerModuleDispatchFn = extern "C" fn(
    method_name: *const c_char,
    args: *const WhiskerValueRaw,
    arg_count: usize,
) -> WhiskerValueRaw;

extern "C" {
    pub fn whisker_bridge_engine_attach(lynx_view_ptr: *mut c_void) -> *mut WhiskerEngine;
    pub fn whisker_bridge_engine_release(engine: *mut WhiskerEngine);

    pub fn whisker_bridge_dispatch(
        engine: *mut WhiskerEngine,
        callback: WhiskerTasmCallback,
        user_data: *mut c_void,
    ) -> bool;

    pub fn whisker_bridge_create_element(
        engine: *mut WhiskerEngine,
        tag: WhiskerElementTag,
    ) -> *mut WhiskerElement;
    pub fn whisker_bridge_create_element_by_name(
        engine: *mut WhiskerEngine,
        tag_name: *const c_char,
    ) -> *mut WhiskerElement;
    pub fn whisker_bridge_release_element(element: *mut WhiskerElement);

    pub fn whisker_bridge_set_attribute(
        element: *mut WhiskerElement,
        key: *const c_char,
        value: *const c_char,
    );
    pub fn whisker_bridge_set_inline_styles(element: *mut WhiskerElement, css: *const c_char);

    pub fn whisker_bridge_append_child(parent: *mut WhiskerElement, child: *mut WhiskerElement);
    pub fn whisker_bridge_remove_child(parent: *mut WhiskerElement, child: *mut WhiskerElement);

    pub fn whisker_bridge_set_event_listener(
        element: *mut WhiskerElement,
        event_name: *const c_char,
        callback: WhiskerEventCallback,
        user_data: *mut c_void,
    );

    pub fn whisker_bridge_set_event_listener_with_value(
        element: *mut WhiskerElement,
        event_name: *const c_char,
        callback: WhiskerEventValueCallback,
        user_data: *mut c_void,
    );

    pub fn whisker_bridge_set_root(engine: *mut WhiskerEngine, page: *mut WhiskerElement);
    pub fn whisker_bridge_flush(engine: *mut WhiskerEngine);

    /// Invoke a registered Whisker platform module's method,
    /// synchronously. See `whisker_bridge.h` for ownership rules
    /// around the returned `WhiskerValueRaw`.
    pub fn whisker_bridge_invoke_module(
        module_name: *const c_char,
        method_name: *const c_char,
        args: *const WhiskerValueRaw,
        arg_count: usize,
    ) -> WhiskerValueRaw;

    /// Async variant. Caller-supplied `callback` fires once the
    /// method completes. `user_data` is opaque — caller owns
    /// lifetime / dropping.
    pub fn whisker_bridge_invoke_module_async(
        module_name: *const c_char,
        method_name: *const c_char,
        args: *const WhiskerValueRaw,
        arg_count: usize,
        callback: WhiskerModuleCallback,
        user_data: *mut c_void,
    ) -> bool;

    /// Free any heap allocations the bridge attached to `value` —
    /// caller of `whisker_bridge_invoke_module` MUST eventually
    /// call this on the returned value (no-op for scalars).
    pub fn whisker_bridge_value_release(value: *mut WhiskerValueRaw);

    /// Register a dispatch function for `module_name`. Called by
    /// the platform-side generated code at app launch (Swift Macro
    /// emits a `@_cdecl` fn + registration call; KSP emits a JNI
    /// wrapper that does the equivalent). Phase 7-Φ.F.
    pub fn whisker_bridge_register_module_dispatch(
        module_name: *const c_char,
        dispatch: WhiskerModuleDispatchFn,
    );

    /// Invoke a Lynx UI method on a mounted element. Synchronous —
    /// dispatches through Lynx's `LynxUIMethodProcessor` (iOS) /
    /// `LynxUIMethodsExecutor` (Android), which in turn calls the
    /// `@WhiskerUIMethod`-emitted forwarder on the element's
    /// `WhiskerUI<View>` subclass.
    ///
    /// `element` is the `WhiskerElement*` originally returned by
    /// `whisker_bridge_create_element_by_name`. The bridge looks up
    /// the Lynx UI sign from this element and routes the method
    /// call to the matching mounted `LynxUI`.
    ///
    /// `args` matches the `invoke_module` shape — a flat
    /// `WhiskerValueRaw[]` the platform side decodes into
    /// `[WhiskerValue]` before dispatch.
    ///
    /// Returns `WhiskerValueRaw` whose ownership matches
    /// `invoke_module` — caller MUST eventually pass it to
    /// `whisker_bridge_value_release`. A bridge-side failure (no
    /// such method, element not mounted, args wrong shape, …)
    /// surfaces as `WHISKER_VALUE_ERROR`.
    ///
    /// Phase 7-Φ.H.2.5: implementation is currently a stub
    /// returning `WHISKER_VALUE_ERROR` — the real wiring lives in
    /// Phase 7-Φ.H.2.7 once the Lynx fork exposes the C wrappers
    /// over `LynxShell::GetUIOwner` / `LynxUIOwner::FindUIBySign` /
    /// `LynxUIMethodProcessor::InvokeMethod`.
    pub fn whisker_bridge_invoke_element_method(
        element: *mut WhiskerElement,
        method_name: *const c_char,
        args: *const WhiskerValueRaw,
        arg_count: usize,
    ) -> WhiskerValueRaw;
}
