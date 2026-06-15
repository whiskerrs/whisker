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
            // Bail out if the next token isn't an ident — RA may
            // inject other sentinels we don't want to chase. The
            // already-collected kwargs still produce a useful
            // expansion.
            if !input.peek(Ident) {
                break;
            }
            let name: Ident = input.parse()?;

            let value = if input.peek(Token![:]) {
                let _: Token![:] = input.parse()?;
                // Try to parse the value expression. If the user is
                // mid-typing and the parse fails, fall through with
                // `None`; the emitter uses `()` so the method-call
                // shape survives for RA's completion.
                input.parse::<Expr>().ok()
            } else {
                // No `:` yet — cursor sits right after the ident.
                None
            };

            kwargs.push(CssKwarg { name, value });

            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            } else {
                // No comma, but the stream might still have trailing
                // tokens (e.g. an unfinished partial after the last
                // kwarg). Drain them so the macro doesn't error out.
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
