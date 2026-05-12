// lyra_bridge.h
//
// C ABI bridging Swift / Rust callers to the Lynx C++ engine.
//
// Threading model
// ---------------
// Element handles produced by `lyra_bridge_create_*` outlive the calling
// thread, but every operation that mutates the engine must run on the
// Lynx TASM thread. Use `lyra_bridge_dispatch` to submit a callback that
// runs on the TASM thread; inside the callback, all element ops are
// safe to call synchronously.
//
// Lifetime
// --------
// Element handles are reference-counted on the C++ side. Calling
// `lyra_bridge_release_element` decrements the count. The engine itself
// is borrowed from a LynxView; release the engine handle before the
// LynxView is deallocated.

#ifndef LYRA_BRIDGE_H_
#define LYRA_BRIDGE_H_

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// ---- Opaque handles -------------------------------------------------------

typedef struct LyraEngine LyraEngine;
typedef struct LyraElement LyraElement;

// Tag type passed to create functions that need a tag string. Numeric
// IDs map to HTML-style tag names ("view", "text", "image", ...).
typedef enum {
    LyraElementTagPage     = 1,
    LyraElementTagView     = 2,
    LyraElementTagText     = 3,
    LyraElementTagRawText  = 4,
    LyraElementTagImage    = 5,
} LyraElementTag;

// ---- Engine lifecycle -----------------------------------------------------

// Attach the bridge to an existing LynxView (`lynx_view_ptr` is treated
// as `LynxView*`). The caller keeps ownership of the LynxView; the
// returned engine handle is only valid while the LynxView is alive.
// Returns NULL on failure (e.g. no LynxShell available yet).
LyraEngine* lyra_bridge_engine_attach(void* lynx_view_ptr);

// Free the engine handle. Does NOT touch the underlying LynxView.
void lyra_bridge_engine_release(LyraEngine* engine);

// ---- Thread dispatch ------------------------------------------------------

// Run `callback(user_data)` on the Lynx TASM thread. Returns immediately
// (the callback may not have executed yet). All `lyra_bridge_*_element`
// and other element ops MUST be called from inside the callback.
typedef void (*LyraTasmCallback)(void* user_data);
bool lyra_bridge_dispatch(LyraEngine* engine,
                          LyraTasmCallback callback,
                          void* user_data);

// ---- Element creation (must be called from inside lyra_bridge_dispatch) --

LyraElement* lyra_bridge_create_element(LyraEngine* engine, LyraElementTag tag);

// Decrement the element's ref count. Always safe to call multiple times
// (idempotent on NULL).
void lyra_bridge_release_element(LyraElement* element);

// ---- Element manipulation (TASM thread only) -----------------------------

// Set a string attribute. `key` and `value` must be NUL-terminated UTF-8.
void lyra_bridge_set_attribute(LyraElement* element,
                               const char* key,
                               const char* value);

// Apply a raw inline-style string ("font-size: 32px; color: black;").
void lyra_bridge_set_inline_styles(LyraElement* element, const char* css);

// Append `child` after the parent's last child.
void lyra_bridge_append_child(LyraElement* parent, LyraElement* child);

// Remove `child` from `parent`. No-op if not present.
void lyra_bridge_remove_child(LyraElement* parent, LyraElement* child);

// ---- Pipeline (TASM thread only) -----------------------------------------

// Make `page` the engine's root element. Must be a Page element produced
// via `lyra_bridge_create_element(LyraElementTagPage)`.
void lyra_bridge_set_root(LyraEngine* engine, LyraElement* page);

// Run resolve / layout / paint and submit to the painting context.
// Call after all element mutations for the current frame are complete.
void lyra_bridge_flush(LyraEngine* engine);

// ---- Phase 0–3 leftovers (kept temporarily for compatibility) ------------

void lyra_bridge_log_hello(void);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // LYRA_BRIDGE_H_
