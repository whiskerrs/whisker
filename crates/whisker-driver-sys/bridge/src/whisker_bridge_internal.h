// Private header shared between whisker_bridge_common.cc and the
// platform-specific glue (whisker_bridge_ios.mm, whisker_bridge_android.cc).
//
// Anything declared here is an *implementation detail* of the bridge —
// callers (Swift/Kotlin/Rust) only see the public C ABI in
// `whisker_bridge.h`.

#ifndef WHISKER_BRIDGE_INTERNAL_H_
#define WHISKER_BRIDGE_INTERNAL_H_

#include <cstdint>

// `WhiskerValueRaw` is the FFI tagged-union (defined in the public
// `whisker_bridge.h`); event payloads cross to the dispatch registry
// as a pointer to one.
struct WhiskerValueRec;

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
// it. `payload` is the event body as a `WhiskerValueRaw` tree (or a
// `WHISKER_VALUE_NULL` value when the event carries no detail); the
// bridge does NOT take ownership and the pointer is only valid for
// the duration of this call. `payload` may be NULL, treated as "no
// body". Returns true if a callback was found and fired (caller
// should consume the event in the host event chain).
//
// Each registered listener is either a no-payload (`WhiskerEventCallback`)
// or value-payload (`WhiskerEventValueCallback`) variant; the bridge
// routes the call to the matching arm.
bool whisker_bridge_internal_dispatch_event(int32_t element_sign,
                                        const char* event_name,
                                        const struct WhiskerValueRec* payload);

#endif  // WHISKER_BRIDGE_INTERNAL_H_
