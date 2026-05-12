// lyra_bridge.h
//
// C ABI bridging Swift / Rust callers to the Lynx C++ engine.
//
// Phase 3c (current): Element PAPI demo — given a LynxView and a text
//                     string, build a single-element tree directly via
//                     ElementManager / FiberElement and flush it.

#ifndef LYRA_BRIDGE_H_
#define LYRA_BRIDGE_H_

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Smoke test, kept around for debugging.
void lyra_bridge_log_hello(void);

// Phase 3b: dispatch a NSLog onto the Lynx TASM thread.
bool lyra_bridge_dispatch_log(void* lynx_view);

// Phase 3c: render `text` as a Lynx-managed element by driving Element
// PAPI directly from the bridge. Returns true if the dispatch succeeded
// (actual render happens asynchronously on the TASM thread).
bool lyra_bridge_render_text(void* lynx_view, const char* text);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // LYRA_BRIDGE_H_
