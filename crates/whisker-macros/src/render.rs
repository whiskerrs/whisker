//! `render!` is the UI-root adapter over the common compose lowering.

use proc_macro::TokenStream;

pub fn expand(input: TokenStream) -> TokenStream {
    super::compose::expand_root(input.into()).into()
}
