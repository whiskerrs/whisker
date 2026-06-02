//! Raw `extern "C"` declarations matching `bridge/include/whisker_bridge.h`.
//!
//! Everything here is `unsafe` to call. Safe wrappers (and the host shim
//! `whisker_app_main` / `whisker_tick` exports) live in `whisker-driver`. Users
//! never depend on this crate directly.

use std::ffi::{c_char, c_int, c_void};

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

// ----- List native item provider --------------------------------------------
//
// C-ABI callback set for `whisker_bridge_list_set_native_item_provider`.
// Mirrors `lynx_list_*` typedefs in `whiskerrs/lynx#9` — the bridge wires
// these through to `ListNativeItemProvider` on the C++ ListElement.
//
// Whisker users do NOT construct these directly. A higher-level safe
// wrapper in `whisker-driver::lynx::list_provider` (boxed `FnMut` +
// lifetime management) is the supported surface.

/// Called by Lynx's list machinery when it needs the element for `index`.
/// Returns the FiberElement's `impl_id` (sign) or
/// [`LYNX_LIST_INVALID_INDEX`] on failure. `reuse_notification` is 1 if
/// the embedder may reuse an existing element for this index.
pub type LynxListComponentAtIndexFn = extern "C" fn(
    index: u32,
    operation_id: i64,
    reuse_notification: c_int,
    user_data: *mut c_void,
) -> i32;

/// Called when the element at `sign` leaves the viewport. The provider
/// may pool or release it.
pub type LynxListEnqueueComponentFn = extern "C" fn(sign: i32, user_data: *mut c_void);

/// Free-callback for the `user_data` cookie. Invoked by the bridge when
/// the list element is destroyed (or the provider is replaced) so a Rust
/// `Box<dyn FnMut>` packed into `user_data` can be dropped.
pub type LynxUserDataFreeFn = extern "C" fn(user_data: *mut c_void);

/// Mirror of `LYNX_LIST_INVALID_INDEX` (the C macro in
/// `lynx_native_renderer_capi.h`) — returned by
/// [`LynxListComponentAtIndexFn`] to signal "no element could be
/// produced for this index". Matches Lynx's
/// `lynx::tasm::list::kInvalidIndex`; 0 is a real `impl_id` and
/// would be silently consumed.
pub const LYNX_LIST_INVALID_INDEX: i32 = -1;
/// Value-payload event callback. `payload` is a `WhiskerValueRaw`
/// tree (never NULL — the bridge normalises a missing body to a
/// `WHISKER_VALUE_NULL` value), owned by the bridge and only valid
/// for the duration of the call (copy out via `from_raw`). See
/// `whisker_bridge_set_event_listener_with_value`.
pub type WhiskerEventValueCallback =
    extern "C" fn(user_data: *mut c_void, payload: *const WhiskerValueRaw);

/// The Rust event dispatcher the bridge calls when its reporter hook
/// fires. Receives the hit-tested target's element sign, the event
/// name (NUL-terminated), and the event body (`WhiskerValueRaw` tree,
/// never NULL). Returns whether the event was consumed (so the
/// reporter can tell Lynx to skip its native chain). See
/// `whisker_bridge_register_event_dispatcher`.
pub type WhiskerEventDispatcher = extern "C" fn(
    target_sign: i32,
    event_name: *const c_char,
    body: *const WhiskerValueRaw,
) -> bool;

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

/// Callback type for module event subscriptions. Fired by the bridge
/// when a registered `(module, event)` pair receives a
/// `whisker_bridge_module_send_event` call. `payload` is borrowed —
/// the bridge frees its allocations once the callback returns.
pub type WhiskerModuleEventCallback =
    extern "C" fn(user_data: *mut c_void, payload: *const WhiskerValueRaw);

/// Callback type for `OnStartObserving` / `OnStopObserving` hooks.
/// The bridge fires these on the 0↔1 listener-count transition for
/// a `(module, event)` pair. Both `module_name` and `event_name` are
/// borrowed (NUL-terminated UTF-8) — copy if you need to retain them
/// past the call. `module_name` lets a shared platform-side
/// trampoline index its own per-module table.
pub type WhiskerModuleObserverHook =
    extern "C" fn(module_name: *const c_char, event_name: *const c_char);

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

    pub fn whisker_bridge_list_set_item_count(element: *mut WhiskerElement, count: i32);

    pub fn whisker_bridge_list_set_native_item_provider(
        element: *mut WhiskerElement,
        component_at_index: LynxListComponentAtIndexFn,
        enqueue_component: LynxListEnqueueComponentFn,
        user_data: *mut c_void,
        user_data_free: LynxUserDataFreeFn,
    );

    // Diagnostic only (Android bridge logs the int as ERROR-level under
    // the given tag). Stub on iOS — symbol present but no-op.
    pub fn whisker_bridge_debug_log_i32(tag: *const c_char, value: i32);

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

    /// Register the Rust event dispatcher (the reporter hook forwards
    /// to it). Called once by the driver at bootstrap. See
    /// [`WhiskerEventDispatcher`].
    pub fn whisker_bridge_register_event_dispatcher(dispatcher: WhiskerEventDispatcher);

    /// The Lynx element sign for `element` — same id the reporter
    /// reports as the event target, used as the key for the driver's
    /// tree + listener maps. Returns 0 for a null element.
    pub fn whisker_bridge_element_sign(element: *mut WhiskerElement) -> i32;

    /// Register a bubble-phase event handler for `event_name` on
    /// `element`, populating its Lynx event set so the UI component
    /// emits the event. The driver calls this for non-gesture
    /// (component) events; touch/gesture events don't need it.
    pub fn whisker_bridge_set_native_event_handler(
        element: *mut WhiskerElement,
        event_name: *const c_char,
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

    // ------------------------------------------------------------------
    // Module event subscription (Phase L-2c)
    // ------------------------------------------------------------------

    /// Register `callback` against `(module_name, event_name)`.
    /// Returns a positive listener id on success, or <= 0 on a
    /// precondition failure. The Rust wrapper hands the caller a
    /// `ModuleSubscription` that calls
    /// `whisker_bridge_module_remove_event_listener` on drop.
    pub fn whisker_bridge_module_add_event_listener(
        module_name: *const c_char,
        event_name: *const c_char,
        callback: WhiskerModuleEventCallback,
        user_data: *mut c_void,
    ) -> i32;

    /// Remove a previously-registered listener. No-op if `listener_id`
    /// is unknown. The bridge does not free the caller's `user_data`.
    pub fn whisker_bridge_module_remove_event_listener(listener_id: i32);

    /// Dispatch `payload` to every listener registered against
    /// `(module_name, event_name)`. Called from the native module
    /// side (`sendEvent` on Swift / Kotlin). `payload` may be NULL
    /// for an unparameterised ping.
    pub fn whisker_bridge_module_send_event(
        module_name: *const c_char,
        event_name: *const c_char,
        payload: *const WhiskerValueRaw,
    );

    /// Register OnStart / OnStopObserving hooks for `module_name`.
    /// The bridge calls `started(event)` on the 0→1 listener-count
    /// transition and `stopped(event)` on 1→0, so the native module
    /// can spin up / tear down an expensive source (e.g. an
    /// `OnBackInvokedCallback` registration) only while needed.
    pub fn whisker_bridge_module_register_observer_hooks(
        module_name: *const c_char,
        started: WhiskerModuleObserverHook,
        stopped: WhiskerModuleObserverHook,
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

    /// Fire-and-forget dispatch of a built-in Lynx UI method whose
    /// arguments are named fields of the params object (`scrollTo`,
    /// `scrollBy`, `autoScroll`, `scrollIntoView`, `requestUIInfo`, …)
    /// rather than the `{"args": […]}` wrapper
    /// `whisker_bridge_invoke_element_method` builds. `params` is a
    /// single `WHISKER_VALUE_MAP`; it (and any nested maps / arrays) is
    /// passed through as the params object directly. Returns
    /// `WHISKER_VALUE_NULL` once dispatch is scheduled (caller still
    /// passes it to `whisker_bridge_value_release`).
    pub fn whisker_bridge_invoke_element_method_with_params(
        element: *mut WhiskerElement,
        method_name: *const c_char,
        params: *const WhiskerValueRaw,
    ) -> WhiskerValueRaw;

    /// Element-level animation dispatch — `element.animate(...)` shape.
    ///
    /// Wraps the new `lynx_element_animate` capi (DOM layer, distinct
    /// from `lynx_ui_invoke_method_*`). `operation` follows
    /// `JavaScriptElement::AnimationOperation`:
    ///   0 = START, 1 = PLAY, 2 = PAUSE, 3 = CANCEL, 4 = FINISH.
    ///
    /// For `START` the full quartet is required: `animation_name` plus a
    /// `WHISKER_VALUE_MAP` of `"0%"/"50%"/"100%"` → CSS-prop map for
    /// `keyframes`, and a `WHISKER_VALUE_MAP` of
    /// `name`/`duration`/`easing`/`iterations`/`direction`/`fill`/`delay`
    /// for `options`. Other operations only consult `animation_name` —
    /// pass NULL for `keyframes` / `options`.
    ///
    /// Returns `WHISKER_VALUE_NULL` on dispatch success;
    /// `WHISKER_VALUE_ERROR` on precondition failure.
    pub fn whisker_bridge_element_animate(
        element: *mut WhiskerElement,
        operation: i32,
        animation_name: *const c_char,
        keyframes: *const WhiskerValueRaw,
        options: *const WhiskerValueRaw,
    ) -> WhiskerValueRaw;

    /// Async, result-returning element-method dispatch
    /// (`boundingClientRect` / `takeScreenshot`). Returns immediately;
    /// `callback(user_data, &result)` fires once the method completes
    /// (typically on the UI thread). On precondition failure / an
    /// unsupported platform the bridge invokes `callback` synchronously
    /// with a `WHISKER_VALUE_ERROR` and returns `false`.
    pub fn whisker_bridge_invoke_element_method_async(
        element: *mut WhiskerElement,
        method_name: *const c_char,
        args: *const WhiskerValueRaw,
        arg_count: usize,
        callback: WhiskerModuleCallback,
        user_data: *mut c_void,
    ) -> bool;

    /// The unified element-method dispatch: `params` (a single
    /// `WHISKER_VALUE_MAP`) is passed through as the method's params
    /// object directly, and the result arrives via `callback`. The one
    /// entry `ElementRef::invoke` / `invoke_typed` build on — both
    /// fire-and-forget actions (callback ignored) and result methods.
    pub fn whisker_bridge_invoke_element_method_async_with_params(
        element: *mut WhiskerElement,
        method_name: *const c_char,
        params: *const WhiskerValueRaw,
        callback: WhiskerModuleCallback,
        user_data: *mut c_void,
    ) -> bool;
}
