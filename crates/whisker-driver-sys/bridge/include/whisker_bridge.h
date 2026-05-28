// whisker_bridge.h
//
// C ABI bridging Swift / Rust callers to the Lynx C++ engine.
//
// Threading model
// ---------------
// Element handles produced by `whisker_bridge_create_*` outlive the calling
// thread, but every operation that mutates the engine must run on the
// Lynx TASM thread. Use `whisker_bridge_dispatch` to submit a callback that
// runs on the TASM thread; inside the callback, all element ops are
// safe to call synchronously.
//
// Lifetime
// --------
// Element handles are reference-counted on the C++ side. Calling
// `whisker_bridge_release_element` decrements the count. The engine itself
// is borrowed from a LynxView; release the engine handle before the
// LynxView is deallocated.

#ifndef WHISKER_BRIDGE_H_
#define WHISKER_BRIDGE_H_

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Export attribute for bridge entry points.
//
// On iOS we ship the bridge inside a dynamic `WhiskerDriver.framework`
// and Swift callers (`WhiskerRuntime/Sources/.../WhiskerView.swift`)
// resolve `whisker_bridge_*` through the framework's `.dynsym` at app
// link time. Without `visibility("default")` the Apple linker
// dead-strips these symbols (they have no in-dylib references because
// the Rust crate doesn't call them — only Swift does). `used` keeps
// the compiler from removing them ahead of the link step.
//
// On Android the same functions are called from JNI exports
// (`Java_rs_whisker_runtime_WhiskerView_native…`) inside the same
// `.so`, so the symbols survive `+whole-archive` even without an
// explicit visibility attribute — but applying it uniformly costs
// nothing and keeps both targets consistent.
#if defined(__GNUC__) || defined(__clang__)
#define WHISKER_BRIDGE_EXPORT __attribute__((visibility("default"), used))
#else
#define WHISKER_BRIDGE_EXPORT
#endif

#ifdef __cplusplus
extern "C" {
#endif

// ---- Opaque handles -------------------------------------------------------

typedef struct WhiskerEngine WhiskerEngine;
typedef struct WhiskerElement WhiskerElement;

// Tag type passed to create functions that need a tag string. Numeric
// IDs map to HTML-style tag names ("view", "text", "image", ...).
typedef enum {
    WhiskerElementTagPage       = 1,
    WhiskerElementTagView       = 2,
    WhiskerElementTagText       = 3,
    WhiskerElementTagRawText    = 4,
    WhiskerElementTagImage      = 5,
    WhiskerElementTagScrollView = 6,
} WhiskerElementTag;

// ---- Engine lifecycle -----------------------------------------------------

// Attach the bridge to an existing LynxView (`lynx_view_ptr` is treated
// as `LynxView*`). The caller keeps ownership of the LynxView; the
// returned engine handle is only valid while the LynxView is alive.
// Returns NULL on failure (e.g. no LynxShell available yet).
WHISKER_BRIDGE_EXPORT WhiskerEngine* whisker_bridge_engine_attach(void* lynx_view_ptr);

// Free the engine handle. Does NOT touch the underlying LynxView.
WHISKER_BRIDGE_EXPORT void whisker_bridge_engine_release(WhiskerEngine* engine);

// ---- Thread dispatch ------------------------------------------------------

// Run `callback(user_data)` on the Lynx TASM thread. Returns immediately
// (the callback may not have executed yet). All `whisker_bridge_*_element`
// and other element ops MUST be called from inside the callback.
typedef void (*WhiskerTasmCallback)(void* user_data);
WHISKER_BRIDGE_EXPORT bool whisker_bridge_dispatch(WhiskerEngine* engine,
                          WhiskerTasmCallback callback,
                          void* user_data);

// ---- Element creation (must be called from inside whisker_bridge_dispatch) --

WHISKER_BRIDGE_EXPORT WhiskerElement* whisker_bridge_create_element(WhiskerEngine* engine, WhiskerElementTag tag);

// Phase 7: tag-by-name element creation for custom / xelement-style
// tags ("x-input", "x-camera-preview", …) not covered by the
// `WhiskerElementTag` enum. Delegates to Lynx's
// `ElementManager::CreateFiberNode(tag_name)`, which routes both
// built-in and registered custom tags through the same factory.
// `tag_name` must be NUL-terminated UTF-8. Returns NULL if the tag
// isn't registered with Lynx's behaviour registry.
WHISKER_BRIDGE_EXPORT WhiskerElement* whisker_bridge_create_element_by_name(WhiskerEngine* engine, const char* tag_name);

// Decrement the element's ref count. Always safe to call multiple times
// (idempotent on NULL).
WHISKER_BRIDGE_EXPORT void whisker_bridge_release_element(WhiskerElement* element);

// ---- Element manipulation (TASM thread only) -----------------------------

// Set a string attribute. `key` and `value` must be NUL-terminated UTF-8.
WHISKER_BRIDGE_EXPORT void whisker_bridge_set_attribute(WhiskerElement* element,
                               const char* key,
                               const char* value);

// Apply a raw inline-style string ("font-size: 32px; color: black;").
WHISKER_BRIDGE_EXPORT void whisker_bridge_set_inline_styles(WhiskerElement* element, const char* css);

// Tell a `<list>` element how many items it has so Lynx's decoupled
// native list can build its `update-list-info` insert-all map (with
// positional item-keys `w_<i>`). The list builder calls this once at
// `__h()` finalize and pairs it with matching `item-key` attrs on
// each child appended via its `child()` override.
WHISKER_BRIDGE_EXPORT void whisker_bridge_list_set_item_count(WhiskerElement* element, int32_t count);

// Install a native item provider on a `<list>` element. The provider's
// `component_at_index` callback is invoked on demand by Lynx's list
// machinery for each item the viewport needs; `enqueue_component` is
// invoked when an item leaves the viewport so the provider can pool or
// release it. `user_data` is opaque; the bridge holds it until the
// list is destroyed (or another provider replaces this one), then
// calls `user_data_free` to release it. Passing `component_at_index =
// NULL` clears any previously installed provider.
//
// Provides Lynx's full `<list>` virtualisation to non-JS embedders
// (e.g. Whisker's `ListMount`). See `whiskerrs/lynx#9` for the
// underlying capi this wraps.
WHISKER_BRIDGE_EXPORT void whisker_bridge_list_set_native_item_provider(
    WhiskerElement* element,
    int32_t (*component_at_index)(uint32_t index, int64_t operation_id,
                                  int reuse_notification, void* user_data),
    void (*enqueue_component)(int32_t sign, void* user_data),
    void* user_data,
    void (*user_data_free)(void* user_data));

// Append `child` after the parent's last child.
WHISKER_BRIDGE_EXPORT void whisker_bridge_append_child(WhiskerElement* parent, WhiskerElement* child);

// Remove `child` from `parent`. No-op if not present.
WHISKER_BRIDGE_EXPORT void whisker_bridge_remove_child(WhiskerElement* parent, WhiskerElement* child);

// Register a native event listener on `element`. When the event fires,
// `callback(user_data)` is invoked from the Lynx TASM thread. The bridge
// also wires the corresponding gesture/handler so the platform UI layer
// (UIView / UIButton) actually delivers the event.
//
// `user_data` is passed back to `callback` opaquely. Caller is responsible
// for keeping it alive as long as the listener is registered. Calling
// this function more than once for the same `name` replaces the prior
// listener.
typedef void (*WhiskerEventCallback)(void* user_data);
WHISKER_BRIDGE_EXPORT void whisker_bridge_set_event_listener(WhiskerElement* element,
                                    const char* event_name,
                                    WhiskerEventCallback callback,
                                    void* user_data);

// The value-carrying event-listener variant
// (`whisker_bridge_set_event_listener_with_value`) is declared below,
// after `WhiskerValueRaw` is defined — its callback hands the event
// body across as a `WhiskerValueRaw` tree rather than a JSON string.

// ---- Pipeline (TASM thread only) -----------------------------------------

// Make `page` the engine's root element. Must be a Page element produced
// via `whisker_bridge_create_element(WhiskerElementTagPage)`.
WHISKER_BRIDGE_EXPORT void whisker_bridge_set_root(WhiskerEngine* engine, WhiskerElement* page);

// Run resolve / layout / paint and submit to the painting context.
// Call after all element mutations for the current frame are complete.
WHISKER_BRIDGE_EXPORT void whisker_bridge_flush(WhiskerEngine* engine);

// ---- Native module invocation (Phase 7-Φ.E) ------------------------------
//
// Non-UI platform-API surface — module classes registered on the
// platform side (Obj-C class on iOS, Java class on Android,
// subclassing Lynx's `LynxModule`) callable from Rust through the
// bridge. Conceptually parallel to React Native's NativeModules but
// without a JS engine in the loop — args + return are passed as
// `WhiskerValue` tagged unions (no JSON marshalling).
//
// Bridge expects each registered module to have a string name and
// a set of methods identifiable by string. Dispatch is reflective
// (Obj-C `NSInvocation` / cached `jmethodID` + `CallObjectMethodA`),
// so registration cost is one-time per module class. Per-call
// overhead is ~200-500ns plus method body cost — about 20-200×
// faster than JSON for typical payloads, and effectively zero-copy
// for `WHISKER_VALUE_BYTES` payloads (image data, file blobs).

typedef enum {
    WHISKER_VALUE_NULL = 0,
    WHISKER_VALUE_BOOL = 1,
    WHISKER_VALUE_INT  = 2,   // int64_t
    WHISKER_VALUE_FLOAT = 3,  // double
    WHISKER_VALUE_STRING = 4, // UTF-8, NOT NUL-terminated; uses `len`
    WHISKER_VALUE_BYTES = 5,  // opaque byte sequence; uses `len`
    WHISKER_VALUE_ARRAY = 6,  // homogeneous-or-heterogeneous list
    WHISKER_VALUE_MAP = 7,    // string-keyed map
    WHISKER_VALUE_ERROR = 8,  // method threw / dispatch failed; string carries the message
} WhiskerValueType;

// Forward declaration so `WhiskerValueArray`/`Map` can hold
// `WhiskerValueRaw`s recursively.
struct WhiskerValueRec;
struct WhiskerKeyValueRec;

// `{ptr, len}` member-typedefs. Promoted from anonymous structs so
// Swift's Clang importer can see them as named types (Swift's
// `WhiskerValue.swift` references `WhiskerStringRef` /
// `WhiskerBytesRef` from these declarations).
typedef struct WhiskerStringRefRec {
    const char* ptr;
    size_t len;
} WhiskerStringRef;

typedef struct WhiskerBytesRefRec {
    const uint8_t* ptr;
    size_t len;
} WhiskerBytesRef;

typedef struct WhiskerValueArrayRec {
    struct WhiskerValueRec* items;  // length = count
    size_t count;
} WhiskerValueArray;

typedef struct WhiskerValueMapRec {
    struct WhiskerKeyValueRec* entries;  // length = count
    size_t count;
} WhiskerValueMap;

// Tagged union — by-value passing across the C ABI. For variable-
// size types (string/bytes/array/map) the inner pointer is borrowed
// from the producer (Rust → C call: Rust owns; C → Rust return: C
// owns + provides matching `whisker_bridge_value_release` so the
// callee can free after copying).
//
// Typedef name is `WhiskerValueRaw` rather than the more obvious
// `WhiskerValue` to avoid clashing with Swift's `enum WhiskerValue`
// (the higher-level typed wrapper Swift / Kotlin module code uses
// — `WhiskerValueRaw` is the FFI form).
typedef struct WhiskerValueRec {
    uint8_t type;  // WhiskerValueType
    uint8_t _pad[7];
    union {
        bool b;
        int64_t i;
        double f;
        WhiskerStringRef s;
        WhiskerBytesRef bytes;
        WhiskerValueArray array;
        WhiskerValueMap map;
    } v;
} WhiskerValueRaw;

typedef struct WhiskerKeyValueRec {
    WhiskerStringRef key;
    WhiskerValueRaw value;
} WhiskerKeyValueRaw;

// NOTE (Phase 5): event listeners no longer live in the bridge. Lynx's
// reporter hook fires once at the hit-tested target, *before* the
// engine walks its capture/bubble chain (whose per-element firings go
// to the absent JS runtime), so the bridge can't observe Lynx-native
// propagation. Whisker instead reconstructs propagation in Rust: the
// `whisker-driver` renderer owns the element tree + per-element
// listeners (with their bind/catch/capture type) and replays
// Lynx's capture→bubble→catch algorithm. The two functions below are
// retained as no-op stubs for ABI stability; the Rust driver no longer
// calls them. Dispatch flows through `whisker_bridge_register_event_dispatcher`.
typedef void (*WhiskerEventValueCallback)(void* user_data, const WhiskerValueRaw* payload);
WHISKER_BRIDGE_EXPORT void whisker_bridge_set_event_listener_with_value(
    WhiskerElement* element,
    const char* event_name,
    WhiskerEventValueCallback callback,
    void* user_data);

// The Rust-side event dispatcher. The platform reporter hook calls it
// (via `whisker_bridge_internal_dispatch_event`) with the hit-tested
// target's element sign, the event name, and the event body. The
// driver walks its own element tree from `target_sign` up to the root,
// runs the capture phase (root→target) then the bubble phase
// (target→root) honoring each listener's bind/catch/capture type, and
// returns whether the event was consumed (so the reporter can tell
// Lynx to skip its own native chain). `body` is borrowed for the call.
typedef bool (*WhiskerEventDispatcher)(int32_t target_sign,
                                       const char* event_name,
                                       const WhiskerValueRaw* body);

// Register (or clear, with NULL) the Rust event dispatcher. Called once
// by the driver at bootstrap.
WHISKER_BRIDGE_EXPORT void whisker_bridge_register_event_dispatcher(
    WhiskerEventDispatcher dispatcher);

// The Lynx element sign (impl id) for `element` — the same identifier
// the reporter reports as the event target, and the key the driver
// uses for its tree + listener maps. Returns 0 for a null element.
WHISKER_BRIDGE_EXPORT int32_t whisker_bridge_element_sign(WhiskerElement* element);

// Register a bubble-phase event handler for `event_name` on `element`,
// populating its Lynx event set. Lynx's UI components only EMIT their
// component-specific events (scroll / layout / uiappear / …) when a
// handler is bound for that name — so the driver calls this for
// non-gesture events. (Touch/gesture events reach the reporter through
// the gesture pipeline regardless, so they don't need this.) The
// handler is a sentinel; Whisker still observes the fire via the
// reporter → dispatcher path.
WHISKER_BRIDGE_EXPORT void whisker_bridge_set_native_event_handler(
    WhiskerElement* element,
    const char* event_name);

// Per-module dispatch function — the platform-side Swift Macro or
// KSP processor emits one of these per `@WhiskerModule`-annotated
// class. The bridge stores `(module_name → dispatch_fn)` in an
// internal lookup table; `whisker_bridge_invoke_module` then routes
// calls through the registered function directly, without going
// through Obj-C `NSInvocation` or JNI per-call reflection
// (Phase 7-Φ.F).
//
// Args are borrowed for the duration of the call; the returned
// `WhiskerValueRaw` may carry heap allocations owned by the dispatch
// function. Caller's `whisker_bridge_value_release` frees them.
typedef WhiskerValueRaw (*WhiskerModuleDispatchFn)(
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count);

// Register `dispatch` as the per-method router for `module_name`.
// Called by the platform-side generated code at app launch (Swift
// Macro emits a `@_cdecl` function and a matching registration
// call; KSP emits a JNI wrapper that does the equivalent).
//
// Last write wins on duplicate registration. Passing `dispatch=NULL`
// unregisters (the table entry is dropped).
WHISKER_BRIDGE_EXPORT void whisker_bridge_register_module_dispatch(
    const char* module_name,
    WhiskerModuleDispatchFn dispatch);

// Invoke a registered module's method synchronously.
//
// `module_name` and `method_name` are NUL-terminated UTF-8. `args`
// points to `arg_count` `WhiskerValue`s (caller owns the storage —
// arg strings/bytes are borrowed for the duration of the call).
//
// The returned `WhiskerValue` may carry a heap allocation (string,
// bytes, nested array/map) owned by the bridge. The caller MUST
// invoke `whisker_bridge_value_release` on the returned value once
// done reading it. A scalar return (`null`/`bool`/`int`/`float`)
// is safe to drop without releasing.
//
// On error (unknown module, missing method, dispatch failed) the
// return is a `WHISKER_VALUE_ERROR` whose `v.s` payload carries a
// UTF-8 description.
WHISKER_BRIDGE_EXPORT WhiskerValueRaw whisker_bridge_invoke_module(
    const char* module_name,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count);

// Async variant — returns immediately, the bridge will call
// `callback(user_data, &result)` once the method completes.
// `result` is borrowed inside the callback only (same ownership
// rules as the sync return — caller must inspect / copy before
// returning). The bridge frees the result automatically once the
// callback returns.
typedef void (*WhiskerModuleCallback)(void* user_data,
                                      const WhiskerValueRaw* result);
WHISKER_BRIDGE_EXPORT bool whisker_bridge_invoke_module_async(
    const char* module_name,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count,
    WhiskerModuleCallback callback,
    void* user_data);

// Free any heap allocations the bridge attached to `value` (string
// content, bytes content, recursive array/map contents). Safe to
// call on a value whose `type` doesn't carry heap data — it's a
// no-op for scalars. NULL pointer is also safe.
WHISKER_BRIDGE_EXPORT void whisker_bridge_value_release(WhiskerValueRaw* value);

// Invoke a Lynx UI method on a mounted element synchronously.
// Phase 7-Φ.H.2.5 stub — currently always returns
// `WHISKER_VALUE_ERROR` because the Lynx fork doesn't yet expose
// C wrappers over `LynxShell::GetUIOwner` / `LynxUIOwner::
// FindUIBySign` / `LynxUIMethodProcessor::InvokeMethod`. Phase
// 7-Φ.H.2.7 fills in the real dispatch.
//
// `element` is the handle originally returned by
// `whisker_bridge_create_element_by_name`. `args` matches
// `invoke_module`'s shape (flat `WhiskerValueRaw[]`). The platform
// side decodes them back to `[WhiskerValue]` inside the
// `@WhiskerUIMethod`-emitted forwarder.
//
// Return ownership matches `invoke_module` — caller MUST eventually
// call `whisker_bridge_value_release` on the result.
WHISKER_BRIDGE_EXPORT WhiskerValueRaw whisker_bridge_invoke_element_method(
    WhiskerElement* element,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count);

// Invoke a built-in Lynx UI method (`scrollTo`, `scrollBy`,
// `autoScroll`, `scrollIntoView`, `requestUIInfo`, ...) whose arguments
// are read as *named fields* of the params object rather than from the
// `{"args": [...]}` wrapper `whisker_bridge_invoke_element_method`
// builds. `params` is a single `WHISKER_VALUE_MAP` value — it (and any
// nested maps / arrays) is passed through as the params object
// directly. Fire-and-forget; returns `WHISKER_VALUE_NULL` once dispatch
// is scheduled, or `WHISKER_VALUE_ERROR`. NULL / non-map `params`
// degrades to an empty object so the method runs with its defaults.
WHISKER_BRIDGE_EXPORT WhiskerValueRaw whisker_bridge_invoke_element_method_with_params(
    WhiskerElement* element,
    const char* method_name,
    const WhiskerValueRaw* params);

// Async variant — the **result-returning** element-method path used
// for `boundingClientRect` / `takeScreenshot` etc. Lynx routes the UI
// method to the main thread and the result arrives via a callback, so
// (unlike the sync fire-and-forget variant above) this is the only
// way to read a method's return value. Returns immediately; the
// bridge calls `callback(user_data, &result)` once the method
// completes (typically on the UI thread). `result` is borrowed inside
// the callback only — the bridge frees it once the callback returns
// (same ownership as `whisker_bridge_invoke_module_async`).
//
// Returns `true` if the call was scheduled. On a precondition failure
// (NULL element/shell, no sign) or where the platform lacks the
// result-async wrapper (Android, pending a Lynx fork release), the
// bridge invokes `callback` synchronously with a
// `WHISKER_VALUE_ERROR` and returns `false`.
WHISKER_BRIDGE_EXPORT bool whisker_bridge_invoke_element_method_async(
    WhiskerElement* element,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count,
    WhiskerModuleCallback callback,
    void* user_data);

// The unified element-method dispatch: `params` (a single
// `WHISKER_VALUE_MAP`) is passed through as the method's params object
// directly (named fields for built-in Lynx methods, or `{"args": […]}`
// for Whisker module elements — the caller builds the shape), and the
// result arrives via `callback`. This is the one entry the Rust
// `ElementRef::invoke` family builds on; both fire-and-forget actions
// (callback ignored) and result methods route through it.
WHISKER_BRIDGE_EXPORT bool whisker_bridge_invoke_element_method_async_with_params(
    WhiskerElement* element,
    const char* method_name,
    const WhiskerValueRaw* params,
    WhiskerModuleCallback callback,
    void* user_data);

// ---- Phase 0–3 leftovers (kept temporarily for compatibility) ------------

WHISKER_BRIDGE_EXPORT void whisker_bridge_log_hello(void);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // WHISKER_BRIDGE_H_
