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
WhiskerEngine* whisker_bridge_engine_attach(void* lynx_view_ptr);

// Free the engine handle. Does NOT touch the underlying LynxView.
void whisker_bridge_engine_release(WhiskerEngine* engine);

// ---- Thread dispatch ------------------------------------------------------

// Run `callback(user_data)` on the Lynx TASM thread. Returns immediately
// (the callback may not have executed yet). All `whisker_bridge_*_element`
// and other element ops MUST be called from inside the callback.
typedef void (*WhiskerTasmCallback)(void* user_data);
bool whisker_bridge_dispatch(WhiskerEngine* engine,
                          WhiskerTasmCallback callback,
                          void* user_data);

// ---- Element creation (must be called from inside whisker_bridge_dispatch) --

WhiskerElement* whisker_bridge_create_element(WhiskerEngine* engine, WhiskerElementTag tag);

// Decrement the element's ref count. Always safe to call multiple times
// (idempotent on NULL).
void whisker_bridge_release_element(WhiskerElement* element);

// ---- Element manipulation (TASM thread only) -----------------------------

// Set a string attribute. `key` and `value` must be NUL-terminated UTF-8.
void whisker_bridge_set_attribute(WhiskerElement* element,
                               const char* key,
                               const char* value);

// Apply a raw inline-style string ("font-size: 32px; color: black;").
void whisker_bridge_set_inline_styles(WhiskerElement* element, const char* css);

// Append `child` after the parent's last child.
void whisker_bridge_append_child(WhiskerElement* parent, WhiskerElement* child);

// Remove `child` from `parent`. No-op if not present.
void whisker_bridge_remove_child(WhiskerElement* parent, WhiskerElement* child);

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
void whisker_bridge_set_event_listener(WhiskerElement* element,
                                    const char* event_name,
                                    WhiskerEventCallback callback,
                                    void* user_data);

// ---- Pipeline (TASM thread only) -----------------------------------------

// Make `page` the engine's root element. Must be a Page element produced
// via `whisker_bridge_create_element(WhiskerElementTagPage)`.
void whisker_bridge_set_root(WhiskerEngine* engine, WhiskerElement* page);

// Run resolve / layout / paint and submit to the painting context.
// Call after all element mutations for the current frame are complete.
void whisker_bridge_flush(WhiskerEngine* engine);

// ---- Phase 0–3 leftovers (kept temporarily for compatibility) ------------

void whisker_bridge_log_hello(void);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // WHISKER_BRIDGE_H_
