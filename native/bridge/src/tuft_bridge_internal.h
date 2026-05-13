// Private header shared between tuft_bridge_common.cc and the
// platform-specific glue (tuft_bridge_ios.mm, tuft_bridge_android.cc).
//
// Anything declared here is an *implementation detail* of the bridge —
// callers (Swift/Kotlin/Rust) only see the public C ABI in
// `tuft_bridge.h`.

#ifndef TUFT_BRIDGE_INTERNAL_H_
#define TUFT_BRIDGE_INTERNAL_H_

#include <cstdint>

namespace lynx {
namespace shell { class LynxShell; }
}

// Opaque to platform glue — internals defined in tuft_bridge_common.cc.
struct TuftEngine;

// Construct a TuftEngine attached to an already-resolved LynxShell.
// Used by the platform-specific `engine_attach` entry points; they're
// responsible for extracting the shell from a LynxView (iOS) or
// LynxView Java object (Android) before calling in here.
TuftEngine* tuft_bridge_internal_engine_create(lynx::shell::LynxShell* shell);

// Mark the engine's event reporter as installed (so subsequent calls
// don't re-install). The platform glue calls this after wiring its
// event hook; common code reads the flag to decide whether to skip.
void tuft_bridge_internal_mark_event_reporter_installed(TuftEngine* engine);
bool tuft_bridge_internal_is_event_reporter_installed(const TuftEngine* engine);

// Look up a registered (element_sign, event_name) callback and invoke
// it. Returns true if a callback was found and fired (caller should
// consume the event in the host event chain).
bool tuft_bridge_internal_dispatch_event(int32_t element_sign,
                                        const char* event_name);

#endif  // TUFT_BRIDGE_INTERNAL_H_
