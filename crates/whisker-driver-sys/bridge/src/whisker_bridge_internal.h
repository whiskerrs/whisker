// Private header shared between whisker_bridge_common.cc and the
// platform-specific glue (whisker_bridge_ios.mm, whisker_bridge_android.cc).
//
// Anything declared here is an *implementation detail* of the bridge —
// callers (Swift/Kotlin/Rust) only see the public C ABI in
// `whisker_bridge.h`.

#ifndef WHISKER_BRIDGE_INTERNAL_H_
#define WHISKER_BRIDGE_INTERNAL_H_

#include <cstdint>

// Opaque to platform glue — internals defined in whisker_bridge_common.cc.
struct WhiskerEngine;

// Construct a WhiskerEngine attached to an already-resolved native
// shell pointer. The platform glue extracts the raw `void*` (via JNI
// reflection on Android, Obj-C ivar access on iOS) and hands it here
// — the common code then runs it through Lynx's C ABI
// (`lynx_shell_from_native_ptr`). Returns NULL if the input is NULL.
WhiskerEngine* whisker_bridge_internal_engine_create(void* native_shell_ptr);

// Mark the engine's event reporter as installed (so subsequent calls
// don't re-install). The platform glue calls this after wiring its
// event hook; common code reads the flag to decide whether to skip.
void whisker_bridge_internal_mark_event_reporter_installed(WhiskerEngine* engine);
bool whisker_bridge_internal_is_event_reporter_installed(const WhiskerEngine* engine);

// Look up a registered (element_sign, event_name) callback and invoke
// it. Returns true if a callback was found and fired (caller should
// consume the event in the host event chain).
bool whisker_bridge_internal_dispatch_event(int32_t element_sign,
                                        const char* event_name);

#endif  // WHISKER_BRIDGE_INTERNAL_H_
