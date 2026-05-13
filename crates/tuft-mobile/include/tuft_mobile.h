// tuft_mobile.h
//
// FFI symbols the user's Tuft app exports for the host (Swift / Kotlin)
// to call. The actual implementations are produced by the
// `#[tuft::main]` proc-macro, which expands a function like
//
//     #[tuft::main]
//     fn app() -> Element { rsx! { ... } }
//
// into wrappers around `tuft_mobile::bootstrap::run` and
// `tuft_mobile::bootstrap::tick`.

#ifndef TUFT_MOBILE_H_
#define TUFT_MOBILE_H_

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Host wake-up callback. The Rust runtime invokes this whenever a signal
// update marks the tree dirty so the host can unpause its render loop
// (CADisplayLink on iOS, Choreographer on Android) to schedule the next
// `tuft_mobile_tick`. `user_data` is the pointer passed in to
// `tuft_mobile_app_main`.
typedef void (*TuftRequestFrameCallback)(void* user_data);

// Bootstraps the Rust runtime against an engine handle obtained from
// `tuft_bridge_engine_attach`. Returns immediately; the actual mount
// happens asynchronously on the Lynx TASM thread.
//
// `engine` is opaque to this header (declared `void*` to avoid pulling
// in `tuft_bridge.h`); both sides agree on the underlying struct.
//
// `request_frame` (may be NULL) is fired by the runtime when signal
// updates require a re-render. Hosts that prefer an unconditional render
// loop can pass NULL and ignore the wake-up mechanism.
void tuft_mobile_app_main(void* engine,
                          TuftRequestFrameCallback request_frame,
                          void* request_frame_data);

// Drive one frame of the Rust runtime. Hosts call this from their
// display-link / choreographer callback. Returns `true` when the runtime
// is idle after this tick (nothing left to render); the host can pause
// its render loop until the next `request_frame` callback fires.
bool tuft_mobile_tick(void* engine);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // TUFT_MOBILE_H_
