// whisker.h
//
// FFI symbols the user's Whisker app exports for the host (Swift / Kotlin)
// to call. The actual implementations are produced by the
// `#[whisker::main]` proc-macro, which expands a function like
//
//     #[whisker::main]
//     fn app() -> Element { rsx! { ... } }
//
// into wrappers around `whisker_driver::bootstrap::{run, tick}`.

#ifndef WHISKER_H_
#define WHISKER_H_

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// Host wake-up callback. The Rust runtime invokes this whenever a signal
// update marks the tree dirty so the host can unpause its render loop
// (CADisplayLink on iOS, Choreographer on Android) to schedule the next
// `whisker_tick`. `user_data` is the pointer passed in to `whisker_app_main`.
typedef void (*WhiskerRequestFrameCallback)(void* user_data);

// Bootstraps the Rust runtime against an engine handle obtained from
// `whisker_bridge_engine_attach`. Returns immediately; the actual mount
// happens asynchronously on the Lynx TASM thread.
//
// `engine` is opaque to this header (declared `void*` to avoid pulling
// in `whisker_bridge.h`); both sides agree on the underlying struct.
//
// `request_frame` (may be NULL) is fired by the runtime when signal
// updates require a re-render. Hosts that prefer an unconditional render
// loop can pass NULL and ignore the wake-up mechanism.
void whisker_app_main(void* engine,
                   WhiskerRequestFrameCallback request_frame,
                   void* request_frame_data);

// Drive one frame of the Rust runtime. Hosts call this from their
// display-link / choreographer callback. Returns `true` when the runtime
// is idle after this tick (nothing left to render); the host can pause
// its render loop until the next `request_frame` callback fires.
bool whisker_tick(void* engine);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // WHISKER_H_
