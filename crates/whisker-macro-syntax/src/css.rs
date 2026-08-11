//! Parse AST for the `css!` macro — `name: value` kwargs.
//!
//! The codegen (in `whisker-macros`) lowers a parsed [`CssInput`] to a
//! `Css::new().name(value).…` method chain. This crate holds only the
//! parse side so the formatter can re-parse and pretty-print `css!`
//! bodies.

use proc_macro2::TokenStream as TokenStream2;
use syn::parse::{Parse, ParseStream, Result as SynResult};
use syn::{Expr, Ident, Token};

/// One `name: value` (or just `name`) pair from the macro input.
pub struct CssKwarg {
    pub name: Ident,
    /// `None` when the user has typed an ident but not yet a `: value`.
    /// Either the `:` was missing, or the value expression failed to
    /// parse (e.g. cursor sentinel).
    pub value: Option<Expr>,
}

/// A parsed `css!` body — a flat list of kwargs.
pub struct CssInput {
    pub kwargs: Vec<CssKwarg>,
}

impl Parse for CssInput {
    fn parse(input: ParseStream) -> SynResult<Self> {
        let mut kwargs = Vec::new();
        while !input.is_empty() {
            // RA injects sentinels that aren't idents; stopping here
            // still leaves the collected kwargs to expand.
            if !input.peek(Ident) {
                break;
            }
            let name: Ident = input.parse()?;

            let value = if input.peek(Token![:]) {
                let _: Token![:] = input.parse()?;
                // A failed parse (mid-typing) falls through as `None`;
                // the emitter uses `()` so the method-call shape
                // survives for RA's completion.
                input.parse::<Expr>().ok()
            } else {
                None
            };

            kwargs.push(CssKwarg { name, value });

            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            } else {
                // Drain any trailing tokens (an unfinished partial
                // after the last kwarg) so the macro doesn't error.
                while !input.is_empty() {
                    let _: proc_macro2::TokenTree = input.parse()?;
                }
                break;
            }
        }
        Ok(CssInput { kwargs })
    }
}

/// Convenience: parse a `css!` body from a token stream.
pub fn parse_input(tokens: TokenStream2) -> SynResult<CssInput> {
    syn::parse2(tokens)
}
