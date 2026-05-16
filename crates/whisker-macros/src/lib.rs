//! Procedural macros for Whisker.
//!
//! - [`main`] — designates the user's app entry. Generates the
//!   `whisker_app_main` and `whisker_tick` FFI exports the native host calls
//!   into; the user only writes `fn app() -> Element`.
//! - [`rsx!`] — Dioxus-style declarative element-tree macro that
//!   desugars to [`whisker_runtime::build`] calls. (Will be replaced
//!   by `render!` in Phase 6.5a A3.)
//! - [`component`] — wraps a function so it runs inside a fresh
//!   reactive owner. The owner is registered against the function's
//!   fn pointer so the Strategy C hot-reload path (Phase 6.5a A6) can
//!   find it. See `docs/reactivity-design.md`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

mod render;
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
        fn __whisker_app_dispatch() -> ::whisker::runtime::view::ElementHandle {
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

        // Anchor symbol used by Whisker's vendored subsecond fork to
        // compute the ASLR slide between this dylib's static layout
        // (cached server-side) and its runtime load address. Both the
        // host dylib and every patch dylib must export this so
        // `dlsym(RTLD_DEFAULT, "whisker_aslr_anchor")` resolves
        // unambiguously inside the user's `.so`.
        //
        // Why a unique name instead of `main` (upstream subsecond's
        // sentinel): on Android, Whisker is loaded via
        // `System.loadLibrary` into a process whose linker namespace
        // already contains several `main` symbols
        // (`app_process64`'s, plus any prior memfd patches), so a
        // dlsym for `main` returns the wrong one and the slide math
        // computes garbage. A unique name only exists in the user's
        // `.so`, so the lookup is collision-free regardless of
        // namespace order.
        //
        // The stub never runs — Whisker is JNI-loaded, never executed
        // as a process entry point. It only needs to exist in the
        // export list at a known static address.
        #[no_mangle]
        pub extern "C" fn whisker_aslr_anchor() -> ::std::ffi::c_int { 0 }
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

/// Phase 6.5a fine-grained renderer macro. Emits imperative
/// element-creation code that calls into
/// [`whisker::runtime::view`] through the thread-local installed
/// renderer, and returns an [`ElementHandle`].
///
/// ```ignore
/// use whisker::prelude::*;
///
/// let handle = render! {
///     view {
///         style: "padding: 16px;",
///         on_tap: || println!("tapped"),
///         text { "Hello, world" }
///     }
/// };
/// ```
///
/// See `crates/whisker-macros/src/render.rs` for the full grammar
/// (matches `rsx!`) and the differences from `rsx!`. `{expr}`
/// interpolation lands in Phase 6.5a A3 Step 3; for now use string
/// literals or assign to a variable outside the macro.
#[proc_macro]
pub fn render(input: TokenStream) -> TokenStream {
    render::expand(input)
}

/// Mark a function as a Whisker reactive component.
///
/// Wraps the body so it runs inside a fresh reactive owner. The
/// owner is created on every call (each invocation = a new mount)
/// and is registered against the function's fn-pointer in the
/// runtime's `component_owners` map — that's what the hot-reload
/// path (Strategy C — Phase 6.5a A6) uses to find the live owners
/// that came from a function whose body subsecond just patched.
///
/// ```ignore
/// use whisker::prelude::*;
///
/// #[component]
/// fn counter(initial: i32) -> impl IntoView {
///     let count = signal(initial);
///     let doubled = memo(move || count.0.get() * 2);
///     render! { /* ... */ }
/// }
/// ```
///
/// Expansion (roughly):
///
/// ```ignore
/// fn counter(initial: i32) -> impl IntoView {
///     let (_owner, __result) = ::whisker::runtime::reactive::mount_component(
///         counter as *const (),
///         move || { /* user body */ },
///     );
///     __result
/// }
/// ```
///
/// Notes:
///
/// - Props become positional function parameters as written; no
///   props struct is generated. Pass them in by value.
/// - The owner stays alive after the component fn returns. Parent
///   ownership / disposal is the renderer's responsibility (A3 + A6).
/// - For the `_owner` variable to drop cleanly inside a tracking
///   parent we don't unmount here — `unmount_component` is called
///   from the parent when this component is removed from its view.
/// - Calling `#[component]` on a function with no body (declaration
///   only) is a compile error from `syn::parse` — same behaviour as
///   `#[whisker::main]`.
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);

    let attrs = &input.attrs;
    let vis = &input.vis;
    let sig = &input.sig;
    let block = &input.block;
    let fn_name = &sig.ident;

    let expanded = quote! {
        #(#attrs)*
        #vis #sig {
            // The closure has to be `move` so prop bindings (which
            // were moved into this function's stack frame) transfer
            // into the body's reactive scope. The fn pointer of
            // `#fn_name` inside its own body is stable across
            // subsecond patches.
            let (__owner, __result) =
                ::whisker::runtime::reactive::mount_component(
                    #fn_name as *const (),
                    move || #block,
                );
            // Bind to suppress "unused variable" — but in production
            // the renderer (A3) will read this through a side channel
            // or by walking owner children; the fn return type is
            // unchanged from what the user wrote.
            let _ = __owner;
            __result
        }
    };

    expanded.into()
}
