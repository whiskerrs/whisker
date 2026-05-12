// lyra_mobile.h
//
// C ABI for the Rust-side mobile runtime. Hand-written for Phase 1; we will
// switch to cbindgen-generated headers once the surface grows.

#ifndef LYRA_MOBILE_H_
#define LYRA_MOBILE_H_

#ifdef __cplusplus
extern "C" {
#endif

// Returns a static, NUL-terminated UTF-8 greeting from the Rust side.
// The pointer is valid for the lifetime of the loaded library — DO NOT free.
const char* lyra_mobile_greeting(void);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // LYRA_MOBILE_H_
