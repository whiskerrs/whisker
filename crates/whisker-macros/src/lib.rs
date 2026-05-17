//! Procedural macros for Whisker.
//!
//! - [`main`] — designates the user's app entry. Generates the
//!   `whisker_app_main` and `whisker_tick` FFI exports the native
//!   host calls into; the user writes `fn app() -> ElementHandle`.
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

    // Per-prop capture + per-invocation clone. Each prop's value is
    // captured by-move into the body closure factory, then re-cloned
    // on every body invocation so re-mount runs (hot-reload) hand
    // the user code a fresh owned value.
    //
    // Limitations: this requires every prop to implement `Clone`.
    // For Copy types (numbers, signal handles), `.clone()` is the
    // same as a copy — no cost. For Clone-not-Copy (`String`,
    // `Vec`, etc.) the clone happens once per remount, never during
    // normal operation. For non-Clone non-Copy types the
    // resulting code fails to compile with a clear bound error —
    // user can wrap in `Rc<T>` / `Arc<T>` if Clone is genuinely
    // impossible.
    let prop_idents: Vec<syn::Ident> = sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => {
                if let syn::Pat::Ident(pi) = &*pat_type.pat {
                    Some(pi.ident.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    let captures: Vec<proc_macro2::TokenStream> = prop_idents
        .iter()
        .map(|ident| {
            let cap = quote::format_ident!("__whisker_prop_{}", ident);
            quote! { let #cap = #ident; }
        })
        .collect();

    let restores: Vec<proc_macro2::TokenStream> = prop_idents
        .iter()
        .map(|ident| {
            let cap = quote::format_ident!("__whisker_prop_{}", ident);
            // `.clone()` is sufficient: for Copy types this is the
            // same as a copy; for Clone types it gives an owned
            // value matching the function's parameter type. Calling
            // `Clone::clone` via the trait path (rather than method
            // syntax) gives a clearer error message when the type
            // does not implement Clone.
            quote! {
                let #ident = ::std::clone::Clone::clone(&#cap);
            }
        })
        .collect();

    let expanded = quote! {
        #(#attrs)*
        #vis #sig {
            #(#captures)*

            let __body: ::std::boxed::Box<
                dyn ::std::ops::Fn() -> ::whisker::runtime::view::ElementHandle + 'static,
            > = ::std::boxed::Box::new(move || {
                #(#restores)*
                #block
            });
            // True per-component remount: the runtime wraps `__body`
            // in a permanent `view` element, stores the body Rc in
            // a side table, and on each subsecond patch re-invokes
            // the body inside a fresh owner. The wrapper element
            // returned here is what the parent's `render!` attaches.
            // Wrapper survives across remounts → parent's element
            // tree is untouched → navigation / scroll position /
            // sibling order all preserved.
            let __wrapper =
                ::whisker::runtime::reactive::mount_component_remountable(
                    #fn_name as *const (),
                    __body,
                );
            __wrapper
        }
    };

    expanded.into()
}
