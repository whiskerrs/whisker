//! Procedural macros for Lyra.
//!
//! - [`main`] — designates the user's app entry. Generates the
//!   `lyra_mobile_app_main` and `lyra_mobile_tick` FFI exports the
//!   native host calls into; the user only writes `fn app() -> Element`.
//! - [`rsx!`] — Dioxus-style declarative element-tree macro that
//!   desugars to [`lyra_runtime::build`] calls.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

mod rsx;

/// Annotates the user's app function (returning `lyra::Element`) and
/// generates the FFI symbols the iOS/Android host expects.
///
/// ```ignore
/// use lyra::prelude::*;
///
/// #[lyra::main]
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
/// pub extern "C" fn lyra_mobile_app_main(engine: *mut std::ffi::c_void) {
///     ::lyra::__main_runtime::run(engine, app);
/// }
///
/// #[no_mangle]
/// pub extern "C" fn lyra_mobile_tick(engine: *mut std::ffi::c_void) {
///     ::lyra::__main_runtime::tick(engine);
/// }
/// ```
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let fn_name = &func.sig.ident;

    let expanded = quote! {
        #func

        #[no_mangle]
        pub extern "C" fn lyra_mobile_app_main(engine: *mut ::std::ffi::c_void) {
            ::lyra::__main_runtime::run(engine, #fn_name);
        }

        #[no_mangle]
        pub extern "C" fn lyra_mobile_tick(engine: *mut ::std::ffi::c_void) {
            ::lyra::__main_runtime::tick(engine);
        }
    };

    expanded.into()
}

/// Build a Lyra [`Element`] tree using a JSX-like syntax.
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
/// `lyra_runtime::Element`. See `crates/lyra-macros/src/rsx.rs` for the
/// full grammar.
#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    rsx::expand(input)
}
