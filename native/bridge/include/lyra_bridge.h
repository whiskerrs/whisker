// lyra_bridge.h
//
// Stable C ABI between Rust (lyra-ffi-lynx) and Lynx Element PAPI.
// All functions are thread-safe at the entry point: they post work onto
// the Lynx TASM thread via LynxEngineProxy::DispatchTaskToLynxEngine.
//
// Element handles are opaque pointers (RefPtr-managed on the C++ side).
// The Rust side must call lyra_bridge_release_element to drop them.

#ifndef LYRA_BRIDGE_H_
#define LYRA_BRIDGE_H_

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ---- Engine handle -------------------------------------------------------

typedef struct LyraEngine LyraEngine;

// Wrap an existing Lynx shell pointer (passed in from the JNI/Obj-C++ side
// after LynxView is initialized). The engine retains a borrowed reference
// to the shell; the caller is responsible for keeping the shell alive.
LyraEngine* lyra_bridge_engine_attach(void* lynx_shell_ptr);

void lyra_bridge_engine_detach(LyraEngine* engine);

// ---- Element tree (placeholders) -----------------------------------------

typedef struct LyraElement LyraElement;

LyraElement* lyra_bridge_create_page(LyraEngine* engine);
LyraElement* lyra_bridge_create_view(LyraEngine* engine);
LyraElement* lyra_bridge_create_text(LyraEngine* engine);

void lyra_bridge_append(LyraEngine* engine,
                         LyraElement* parent,
                         LyraElement* child);

void lyra_bridge_remove(LyraEngine* engine,
                         LyraElement* parent,
                         LyraElement* child);

void lyra_bridge_set_attribute(LyraEngine* engine,
                                LyraElement* elem,
                                const char* key,
                                const uint8_t* value_msgpack,
                                size_t value_len);

void lyra_bridge_flush(LyraEngine* engine, LyraElement* root);

void lyra_bridge_release_element(LyraElement* elem);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // LYRA_BRIDGE_H_
