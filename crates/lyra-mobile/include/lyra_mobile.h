// lyra_mobile.h
//
// FFI symbols the user's Lyra app exports for the host (Swift / Kotlin)
// to call. The actual implementations are produced by the
// `#[lyra::main]` proc-macro, which expands a function like
//
//     #[lyra::main]
//     fn app() -> Element { rsx! { ... } }
//
// into wrappers around `lyra_mobile::bootstrap::run` and
// `lyra_mobile::bootstrap::tick`.

#ifndef LYRA_MOBILE_H_
#define LYRA_MOBILE_H_

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Host wake-up callback. The Rust runtime invokes this whenever a signal
// update marks the tree dirty so the host can unpause its render loop
// (CADisplayLink on iOS, Choreographer on Android) to schedule the next
// `lyra_mobile_tick`. `user_data` is the pointer passed in to
// `lyra_mobile_app_main`.
typedef void (*LyraRequestFrameCallback)(void* user_data);

// Bootstraps the Rust runtime against an engine handle obtained from
// `lyra_bridge_engine_attach`. Returns immediately; the actual mount
// happens asynchronously on the Lynx TASM thread.
//
// `engine` is opaque to this header (declared `void*` to avoid pulling
// in `lyra_bridge.h`); both sides agree on the underlying struct.
//
// `request_frame` (may be NULL) is fired by the runtime when signal
// updates require a re-render. Hosts that prefer an unconditional render
// loop can pass NULL and ignore the wake-up mechanism.
void lyra_mobile_app_main(void* engine,
                          LyraRequestFrameCallback request_frame,
                          void* request_frame_data);

// Drive one frame of the Rust runtime. Hosts call this from their
// display-link / choreographer callback. Returns `true` when the runtime
// is idle after this tick (nothing left to render); the host can pause
// its render loop until the next `request_frame` callback fires.
bool lyra_mobile_tick(void* engine);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // LYRA_MOBILE_H_
