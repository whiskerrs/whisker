//! Procedural macros for Lyra.
//!
//! - `#[lyra::main]` — designates the user's app entry. Phase 4 stub for
//!   now; Phase 8 runtime will use it to generate the FFI export.
//! - [`rsx!`] — Dioxus-style declarative element-tree macro that desugars
//!   to [`lyra_runtime::build`] calls.

use proc_macro::TokenStream;

mod rsx;

/// Entry-point attribute. Currently a no-op (returns the input as-is)
/// while we wire up the C ABI in `lyra-mobile`.
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
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
