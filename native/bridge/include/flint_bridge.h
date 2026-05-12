// flint_bridge.h
//
// Stable C ABI between Rust (flint-ffi-lynx) and Lynx Element PAPI.
// All functions are thread-safe at the entry point: they post work onto
// the Lynx TASM thread via LynxEngineProxy::DispatchTaskToLynxEngine.
//
// Element handles are opaque pointers (RefPtr-managed on the C++ side).
// The Rust side must call flint_bridge_release_element to drop them.

#ifndef FLINT_BRIDGE_H_
#define FLINT_BRIDGE_H_

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ---- Engine handle -------------------------------------------------------

typedef struct FlintEngine FlintEngine;

// Wrap an existing Lynx shell pointer (passed in from the JNI/Obj-C++ side
// after LynxView is initialized). The engine retains a borrowed reference
// to the shell; the caller is responsible for keeping the shell alive.
FlintEngine* flint_bridge_engine_attach(void* lynx_shell_ptr);

void flint_bridge_engine_detach(FlintEngine* engine);

// ---- Element tree (placeholders) -----------------------------------------

typedef struct FlintElement FlintElement;

FlintElement* flint_bridge_create_page(FlintEngine* engine);
FlintElement* flint_bridge_create_view(FlintEngine* engine);
FlintElement* flint_bridge_create_text(FlintEngine* engine);

void flint_bridge_append(FlintEngine* engine,
                         FlintElement* parent,
                         FlintElement* child);

void flint_bridge_remove(FlintEngine* engine,
                         FlintElement* parent,
                         FlintElement* child);

void flint_bridge_set_attribute(FlintEngine* engine,
                                FlintElement* elem,
                                const char* key,
                                const uint8_t* value_msgpack,
                                size_t value_len);

void flint_bridge_flush(FlintEngine* engine, FlintElement* root);

void flint_bridge_release_element(FlintElement* elem);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // FLINT_BRIDGE_H_
