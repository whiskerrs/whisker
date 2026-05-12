// Private header shared between lyra_bridge_common.cc and the
// platform-specific glue (lyra_bridge_ios.mm, lyra_bridge_android.cc).
//
// Anything declared here is an *implementation detail* of the bridge —
// callers (Swift/Kotlin/Rust) only see the public C ABI in
// `lyra_bridge.h`.

#ifndef LYRA_BRIDGE_INTERNAL_H_
#define LYRA_BRIDGE_INTERNAL_H_

#include <cstdint>

namespace lynx {
namespace shell { class LynxShell; }
}

// Opaque to platform glue — internals defined in lyra_bridge_common.cc.
struct LyraEngine;

// Construct a LyraEngine attached to an already-resolved LynxShell.
// Used by the platform-specific `engine_attach` entry points; they're
// responsible for extracting the shell from a LynxView (iOS) or
// LynxView Java object (Android) before calling in here.
LyraEngine* lyra_bridge_internal_engine_create(lynx::shell::LynxShell* shell);

// Mark the engine's event reporter as installed (so subsequent calls
// don't re-install). The platform glue calls this after wiring its
// event hook; common code reads the flag to decide whether to skip.
void lyra_bridge_internal_mark_event_reporter_installed(LyraEngine* engine);
bool lyra_bridge_internal_is_event_reporter_installed(const LyraEngine* engine);

// Look up a registered (element_sign, event_name) callback and invoke
// it. Returns true if a callback was found and fired (caller should
// consume the event in the host event chain).
bool lyra_bridge_internal_dispatch_event(int32_t element_sign,
                                        const char* event_name);

#endif  // LYRA_BRIDGE_INTERNAL_H_
