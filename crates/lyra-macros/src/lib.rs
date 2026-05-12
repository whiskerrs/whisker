//! Procedural macros for Lyra.
//!
//! - `#[lyra::main]` — designates the app entry point (`fn app() -> Element`).
//! - `rsx!` — Dioxus-style declarative UI macro (placeholder).

use proc_macro::TokenStream;

/// Entry point macro. Wraps `fn app() -> Element` with the necessary FFI
/// exports so the native runtime can invoke it.
///
/// Usage:
/// ```ignore
/// #[lyra::main]
/// fn app() -> Element {
///     rsx! { view { text { "Hello" } } }
/// }
/// ```
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Placeholder: just emit the input as-is for now.
    item
}
