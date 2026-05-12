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

#ifdef __cplusplus
extern "C" {
#endif

// Bootstraps the Rust runtime against an engine handle obtained from
// `lyra_bridge_engine_attach`. Returns immediately; the actual mount
// happens asynchronously on the Lynx TASM thread.
//
// `engine` is opaque to this header (declared `void*` to avoid pulling
// in `lyra_bridge.h`); both sides agree on the underlying struct.
void lyra_mobile_app_main(void* engine);

// Drive one frame of the Rust runtime. Hosts call this on whatever
// cadence they want (Swift `Timer`, Choreographer, etc.). Frames where
// no signal is dirty short-circuit cheaply.
void lyra_mobile_tick(void* engine);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // LYRA_MOBILE_H_
