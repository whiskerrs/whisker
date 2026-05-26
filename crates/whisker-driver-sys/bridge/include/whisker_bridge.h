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

// Register a native event listener that receives the event body as a
// `WhiskerValueRaw` tree — the same tagged-union wire as module
// args/returns, no JSON round-trip. When the event fires the bridge
// builds the value from the platform-side event body (Lynx's
// `LynxEvent.generateEventBody` dict on iOS, the event-params map on
// Android) and hands `callback(user_data, &value)` from the TASM
// thread.
//
// `payload` is owned by the bridge and only valid for the duration of
// the call (the callback copies out via the Rust `from_raw`). For a
// bodyless event the bridge passes a `WHISKER_VALUE_NULL` value, never
// NULL pointer. Calling more than once for the same `(element,
// event_name)` replaces the prior listener.
typedef void (*WhiskerEventValueCallback)(void* user_data, const WhiskerValueRaw* payload);
WHISKER_BRIDGE_EXPORT void whisker_bridge_set_event_listener_with_value(
    WhiskerElement* element,
    const char* event_name,
    WhiskerEventValueCallback callback,
    void* user_data);

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

// ---- Phase 0–3 leftovers (kept temporarily for compatibility) ------------

WHISKER_BRIDGE_EXPORT void whisker_bridge_log_hello(void);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // WHISKER_BRIDGE_H_
