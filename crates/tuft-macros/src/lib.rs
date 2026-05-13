//! Procedural macros for Tuft.
//!
//! - [`main`] — designates the user's app entry. Generates the
//!   `tuft_mobile_app_main` and `tuft_mobile_tick` FFI exports the
//!   native host calls into; the user only writes `fn app() -> Element`.
//! - [`rsx!`] — Dioxus-style declarative element-tree macro that
//!   desugars to [`tuft_runtime::build`] calls.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

mod rsx;

/// Annotates the user's app function (returning `tuft::Element`) and
/// generates the FFI symbols the iOS/Android host expects.
///
/// ```ignore
/// use tuft::prelude::*;
///
/// #[tuft::main]
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
/// pub extern "C" fn tuft_mobile_app_main(
///     engine: *mut std::ffi::c_void,
///     request_frame: Option<extern "C" fn(*mut std::ffi::c_void)>,
///     request_frame_data: *mut std::ffi::c_void,
/// ) {
///     ::tuft::__main_runtime::run(engine, request_frame, request_frame_data, app);
/// }
///
/// #[no_mangle]
/// pub extern "C" fn tuft_mobile_tick(engine: *mut std::ffi::c_void) -> bool {
///     ::tuft::__main_runtime::tick(engine)
/// }
/// ```
///
/// `request_frame` is the host's "wake up the render loop" callback. The
/// runtime invokes it when a signal update marks the tree dirty so the
/// host can unpause its `CADisplayLink` (or equivalent) to schedule the
/// next tick. Pass `None` to opt into an unconditional 60Hz loop.
///
/// `tuft_mobile_tick` returns `true` when the runtime is idle after the
/// tick; the host can pause its render loop until the next
/// `request_frame` fires.
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let fn_name = &func.sig.ident;

    let expanded = quote! {
        #func

        #[no_mangle]
        pub extern "C" fn tuft_mobile_app_main(
            engine: *mut ::std::ffi::c_void,
            request_frame: ::std::option::Option<
                extern "C" fn(*mut ::std::ffi::c_void),
            >,
            request_frame_data: *mut ::std::ffi::c_void,
        ) {
            ::tuft::__main_runtime::run(engine, request_frame, request_frame_data, #fn_name);
        }

        #[no_mangle]
        pub extern "C" fn tuft_mobile_tick(engine: *mut ::std::ffi::c_void) -> bool {
            ::tuft::__main_runtime::tick(engine)
        }
    };

    expanded.into()
}

/// Build a Tuft [`Element`] tree using a JSX-like syntax.
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
/// `tuft_runtime::Element`. See `crates/tuft-macros/src/rsx.rs` for the
/// full grammar.
#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    rsx::expand(input)
}
