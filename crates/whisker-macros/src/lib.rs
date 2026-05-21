//! Procedural macros for Whisker.
//!
//! - [`main`] — designates the user's app entry. Generates the
//!   `whisker_app_main` and `whisker_tick` FFI exports the native
//!   host calls into; the user writes `fn app() -> Element`.
//! - [`render!`] — fine-grained renderer macro. Emits imperative
//!   `view::*` dispatch + `effect`s for dynamic parts. See
//!   `crates/whisker-macros/src/render.rs` for the grammar.
//! - [`component`] — wraps a function so it runs inside a fresh
//!   reactive owner. The owner is registered against the function's
//!   fn pointer so the Strategy C hot-reload path (Phase 6.5a A6)
//!   can find it. See `docs/reactivity-design.md`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

mod component;
mod render;

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
        fn __whisker_app_dispatch() -> ::whisker::runtime::view::Element {
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

/// Phase 6.5a fine-grained renderer macro. Emits imperative
/// element-creation code that calls into
/// [`whisker::runtime::view`] through the thread-local installed
/// renderer, and returns an [`Element`].
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
/// The macro takes the user's `fn xxx(a: A, b: B) -> Element`
/// and emits both:
///
/// 1. A `XxxProps` struct (Pascal-cased function name + `Props`)
///    derived from the parameter list, with
///    `#[derive(::typed_builder::TypedBuilder)]` so callers can
///    construct it via `XxxProps::builder().a(...).b(...).build()`.
///    Each field gets `#[builder(setter(into))]` for `Into` coercion
///    on the call side (`&str` → `String`, `i32` → `f64`, …).
///    `Option<T>` props get `strip_option` + `default` so callers may
///    omit them. `Children` props get a default empty closure. A
///    `#[prop(default = expr)]` attribute on a parameter is forwarded
///    to typed-builder as the field's default.
///
/// 2. A rewritten `fn xxx(__props: XxxProps) -> Element` whose
///    body destructures the props back into local variables and runs
///    the user's original `#block` inside the existing
///    `mount_component_remountable` machinery (per-component
///    remount + subsecond hot-reload integration).
///
/// The signature change is deliberate: positional `xxx(a, b)`
/// invocations no longer compile. User components are now invoked
/// exclusively through `render!`'s `xxx { a: …, b: … }` syntax,
/// which the `render!` macro lowers to
/// `xxx(XxxProps::builder().a(…).b(…).build())`. This unifies the
/// call-site shape with built-in elements (`view { … }`).
///
/// ```ignore
/// use whisker::prelude::*;
///
/// #[component]
/// fn counter(initial: i32) -> Element {
///     let (count, set_count) = signal(initial);
///     render! { /* ... */ }
/// }
///
/// // Call site (always through `render!`):
/// render! { counter { initial: 0 } }
/// ```
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    component::expand(item.into()).into()
}
