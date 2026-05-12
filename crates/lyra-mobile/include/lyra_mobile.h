// lyra_mobile.h
//
// C ABI for the Rust-side mobile runtime. Hand-written for now; we will
// switch to cbindgen-generated headers once the surface grows.

#ifndef LYRA_MOBILE_H_
#define LYRA_MOBILE_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Returns a static, NUL-terminated UTF-8 greeting from the Rust side.
// The pointer is valid for the lifetime of the loaded library — DO NOT free.
const char* lyra_mobile_greeting(void);

// Bootstraps the Rust runtime against an engine handle obtained from
// `lyra_bridge_engine_attach`. The handle is opaque to this module —
// declared as `void*` to avoid a cross-header dependency on
// `lyra_bridge.h`. Returns immediately; rendering is done asynchronously
// on the Lynx TASM thread.
void lyra_mobile_app_main(void* engine);

// Drive one frame of the Rust runtime. Hosts call this on whatever
// cadence they want (Swift `Timer`, Choreographer, etc.). Returns
// immediately; the actual frame work happens asynchronously on the
// Lynx TASM thread.
void lyra_mobile_tick(void* engine);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // LYRA_MOBILE_H_
