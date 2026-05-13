// tuft_bridge.h
//
// C ABI bridging Swift / Rust callers to the Lynx C++ engine.
//
// Threading model
// ---------------
// Element handles produced by `tuft_bridge_create_*` outlive the calling
// thread, but every operation that mutates the engine must run on the
// Lynx TASM thread. Use `tuft_bridge_dispatch` to submit a callback that
// runs on the TASM thread; inside the callback, all element ops are
// safe to call synchronously.
//
// Lifetime
// --------
// Element handles are reference-counted on the C++ side. Calling
// `tuft_bridge_release_element` decrements the count. The engine itself
// is borrowed from a LynxView; release the engine handle before the
// LynxView is deallocated.

#ifndef TUFT_BRIDGE_H_
#define TUFT_BRIDGE_H_

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// ---- Opaque handles -------------------------------------------------------

typedef struct TuftEngine TuftEngine;
typedef struct TuftElement TuftElement;

// Tag type passed to create functions that need a tag string. Numeric
// IDs map to HTML-style tag names ("view", "text", "image", ...).
typedef enum {
    TuftElementTagPage       = 1,
    TuftElementTagView       = 2,
    TuftElementTagText       = 3,
    TuftElementTagRawText    = 4,
    TuftElementTagImage      = 5,
    TuftElementTagScrollView = 6,
} TuftElementTag;

// ---- Engine lifecycle -----------------------------------------------------

// Attach the bridge to an existing LynxView (`lynx_view_ptr` is treated
// as `LynxView*`). The caller keeps ownership of the LynxView; the
// returned engine handle is only valid while the LynxView is alive.
// Returns NULL on failure (e.g. no LynxShell available yet).
TuftEngine* tuft_bridge_engine_attach(void* lynx_view_ptr);

// Free the engine handle. Does NOT touch the underlying LynxView.
void tuft_bridge_engine_release(TuftEngine* engine);

// ---- Thread dispatch ------------------------------------------------------

// Run `callback(user_data)` on the Lynx TASM thread. Returns immediately
// (the callback may not have executed yet). All `tuft_bridge_*_element`
// and other element ops MUST be called from inside the callback.
typedef void (*TuftTasmCallback)(void* user_data);
bool tuft_bridge_dispatch(TuftEngine* engine,
                          TuftTasmCallback callback,
                          void* user_data);

// ---- Element creation (must be called from inside tuft_bridge_dispatch) --

TuftElement* tuft_bridge_create_element(TuftEngine* engine, TuftElementTag tag);

// Decrement the element's ref count. Always safe to call multiple times
// (idempotent on NULL).
void tuft_bridge_release_element(TuftElement* element);

// ---- Element manipulation (TASM thread only) -----------------------------

// Set a string attribute. `key` and `value` must be NUL-terminated UTF-8.
void tuft_bridge_set_attribute(TuftElement* element,
                               const char* key,
                               const char* value);

// Apply a raw inline-style string ("font-size: 32px; color: black;").
void tuft_bridge_set_inline_styles(TuftElement* element, const char* css);

// Append `child` after the parent's last child.
void tuft_bridge_append_child(TuftElement* parent, TuftElement* child);

// Remove `child` from `parent`. No-op if not present.
void tuft_bridge_remove_child(TuftElement* parent, TuftElement* child);

// Register a native event listener on `element`. When the event fires,
// `callback(user_data)` is invoked from the Lynx TASM thread. The bridge
// also wires the corresponding gesture/handler so the platform UI layer
// (UIView / UIButton) actually delivers the event.
//
// `user_data` is passed back to `callback` opaquely. Caller is responsible
// for keeping it alive as long as the listener is registered. Calling
// this function more than once for the same `name` replaces the prior
// listener.
typedef void (*TuftEventCallback)(void* user_data);
void tuft_bridge_set_event_listener(TuftElement* element,
                                    const char* event_name,
                                    TuftEventCallback callback,
                                    void* user_data);

// ---- Pipeline (TASM thread only) -----------------------------------------

// Make `page` the engine's root element. Must be a Page element produced
// via `tuft_bridge_create_element(TuftElementTagPage)`.
void tuft_bridge_set_root(TuftEngine* engine, TuftElement* page);

// Run resolve / layout / paint and submit to the painting context.
// Call after all element mutations for the current frame are complete.
void tuft_bridge_flush(TuftEngine* engine);

// ---- Phase 0–3 leftovers (kept temporarily for compatibility) ------------

void tuft_bridge_log_hello(void);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // TUFT_BRIDGE_H_
