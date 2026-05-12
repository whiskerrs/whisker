// lyra_bridge.h
//
// C ABI bridging Swift / Rust callers to the Lynx C++ engine.
//
// Phase 3b (current): smoke test PLUS reach into LynxView → engineProxy
//                     and dispatch a task onto the Lynx TASM thread.
// Phase 3c (next):    Element PAPI surface (CreatePage, CreateText,
//                     SetAttribute, FlushElementTree, …).

#ifndef LYRA_BRIDGE_H_
#define LYRA_BRIDGE_H_

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Smoke test from Phase 3a.
void lyra_bridge_log_hello(void);

// Hand a LynxView pointer (typed `void*` to keep the C ABI clean) to the
// bridge. The bridge resolves the view's engine proxy and dispatches a
// log task onto the Lynx TASM thread. Returns true on success.
//
// Phase 3b only — exists to verify the engine-thread bridge works before
// we layer Element PAPI on top.
bool lyra_bridge_dispatch_log(void* lynx_view);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // LYRA_BRIDGE_H_
