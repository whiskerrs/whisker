//! `css!` is the argument-only adapter over the common compose lowering.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use whisker_macro_syntax::compose::ComposeArguments;

pub fn expand(input: TokenStream2) -> TokenStream2 {
    match syn::parse2::<ComposeArguments>(input) {
        Ok(input) => {
            super::compose::arguments_to_tokens(quote! { Css::builder() }, &input.arguments)
        }
        Err(error) => error.to_compile_error(),
    }
}
