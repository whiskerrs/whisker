//! Procedural macros for Whisker.
//!
//! - [`main`] — designates the user's app entry. Generates the
//!   `whisker_app_main` and `whisker_tick` FFI exports the native host calls
//!   into; the user only writes `fn app() -> Element`.
//! - [`rsx!`] — Dioxus-style declarative element-tree macro that
//!   desugars to [`whisker_runtime::build`] calls.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

mod rsx;

/// Annotates the user's app function (returning `whisker::Element`) and
/// generates the FFI symbols the iOS/Android host expects.
///
/// ```ignore
/// use whisker::prelude::*;
///
/// #[whisker::main]
/// fn app() -> Element {
///     rsx! { page { text { "Hello" } } }
/// }
/// ```
///
/// Expands to (roughly):
///
/// ```ignore
/// fn app() -> Element { /* user body */ }
///
/// #[no_mangle]
/// pub extern "C" fn whisker_app_main(
///     engine: *mut std::ffi::c_void,
///     request_frame: Option<extern "C" fn(*mut std::ffi::c_void)>,
///     request_frame_data: *mut std::ffi::c_void,
/// ) {
///     ::whisker::__main_runtime::run(engine, request_frame, request_frame_data, app);
/// }
///
/// #[no_mangle]
/// pub extern "C" fn whisker_tick(engine: *mut std::ffi::c_void) -> bool {
///     ::whisker::__main_runtime::tick(engine)
/// }
/// ```
///
/// `request_frame` is the host's "wake up the render loop" callback. The
/// runtime invokes it when a signal update marks the tree dirty so the
/// host can unpause its `CADisplayLink` (or equivalent) to schedule the
/// next tick. Pass `None` to opt into an unconditional 60Hz loop.
///
/// `whisker_tick` returns `true` when the runtime is idle after the tick;
/// the host can pause its render loop until the next `request_frame`
/// fires.
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let fn_name = &func.sig.ident;

    let expanded = quote! {
        #func

        // The app fn the runtime invokes every frame. Unconditionally
        // routes through `whisker::__main_runtime::call_user_app`, which
        // is `#[inline(always)]` so the wrapper body lands in the user
        // crate's compilation unit. Whether the wrapper actually
        // dispatches through `subsecond::call` (Tier 1 / hot-reload
        // on) or just invokes `#fn_name()` directly (release) is
        // decided by `whisker`'s own `hot-reload` feature flag — the
        // user crate doesn't need a matching feature of its own.
        fn __whisker_app_dispatch() -> ::whisker::Element {
            ::whisker::__main_runtime::call_user_app(#fn_name)
        }

        #[no_mangle]
        pub extern "C" fn whisker_app_main(
            engine: *mut ::std::ffi::c_void,
            request_frame: ::std::option::Option<
                extern "C" fn(*mut ::std::ffi::c_void),
            >,
            request_frame_data: *mut ::std::ffi::c_void,
        ) {
            ::whisker::__main_runtime::run(
                engine,
                request_frame,
                request_frame_data,
                __whisker_app_dispatch,
            );
        }

        #[no_mangle]
        pub extern "C" fn whisker_tick(engine: *mut ::std::ffi::c_void) -> bool {
            ::whisker::__main_runtime::tick(engine)
        }

        // `subsecond::apply_patch` uses an exported `main` symbol as
        // a sentinel for the patch dylib's base address (see
        // subsecond-0.7.9 lib.rs:526 — `lib.get(b"main").ok().unwrap()`).
        // Dioxus apps satisfy this because their user crate is a `bin`
        // with a real `main`; Whisker's user crate is a library, so we
        // synthesize one here. The stub never runs (Whisker is loaded
        // via `System.loadLibrary` / JNI, never executed as a process
        // entry point), but it has to be exported in both the host
        // dylib's and every patch dylib's `.dynsym` so subsecond's
        // sentinel lookup succeeds on both sides.
        //
        // Gated on `not(test)` so `cargo test --lib` (which links
        // libtest's own `main` into the test runner) doesn't see two
        // `main` symbols and fail with "entry symbol main declared
        // multiple times".
        #[cfg(not(test))]
        #[no_mangle]
        pub extern "C" fn main() -> ::std::ffi::c_int { 0 }
    };

    expanded.into()
}

/// Build a Whisker [`Element`] tree using a JSX-like syntax.
///
/// ```ignore
/// rsx! {
///     view { class: "row",
///         text { style: "font-size: 16px;",
///             "Hello, {name}"
///         }
///     }
/// }
/// ```
///
/// Desugars to a chained builder expression returning an
/// `whisker_runtime::Element`. See `crates/whisker-macros/src/rsx.rs` for the
/// full grammar.
#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    rsx::expand(input)
}
