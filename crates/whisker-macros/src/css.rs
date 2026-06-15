//! `css!` macro codegen — builds a [`Css`] value from `name: value`
//! kwargs.
//!
//! The PARSE side (`CssInput` / `CssKwarg` + their `Parse` impl) lives
//! in `whisker-macro-syntax`; this module holds only the lowering.
//!
//! Emitted as a proc macro (not `macro_rules!`) so partial input
//! produced by rust-analyzer's completion engine still lowers to a
//! well-formed method-call chain. Every kwarg becomes `.<name>(<value>)`
//! in the expansion, and when the value is missing (cursor sitting
//! after `name` but before `:`), we emit `.<name>(())` so RA's
//! method-name completion fires on the `.<name>` part.
//!
//! [`Css`]: whisker_css::Css

use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use whisker_macro_syntax::css::CssInput;

/// Expand `css!(name: value, …)` into `Css::new().name(value).…`.
///
/// Paths resolve against the call site. `Css` itself is taken
/// straight from the call site's scope — `use whisker::prelude::*`
/// brings it in. Falling through unqualified keeps the macro usable
/// from both `whisker` (umbrella) and `whisker-css` standalone
/// without runtime-aware path detection.
pub fn expand(input: TokenStream2) -> TokenStream2 {
    let parsed: CssInput = match syn::parse2(input) {
        Ok(p) => p,
        // On total parse failure, still emit the root `Css::new()`
        // so the user sees a real type at the cursor instead of a
        // raw macro error.
        Err(_) => return quote! { Css::new() },
    };

    let mut chain = quote! { Css::new() };
    for kw in &parsed.kwargs {
        let name = &kw.name;
        let value: TokenStream2 = match &kw.value {
            Some(expr) => quote! { #expr },
            None => quote! { () },
        };
        // Keep the method-call's identifier span attached to the
        // user's source span so RA's jump-to-definition / hover
        // resolve to the right method on `Css`.
        chain = quote_spanned! {name.span()=> #chain.#name(#value) };
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strip whitespace so token-stream layout differences don't
    /// trip the assertion.
    fn norm(t: TokenStream2) -> String {
        t.to_string().split_whitespace().collect::<String>()
    }

    #[test]
    fn empty_input_yields_bare_new() {
        let out = expand(quote! {});
        assert_eq!(norm(out), "Css::new()");
    }

    #[test]
    fn single_complete_kwarg_emits_method_call() {
        let out = expand(quote! { color: red });
        assert_eq!(norm(out), "Css::new().color(red)");
    }

    #[test]
    fn multiple_kwargs_chain() {
        let out = expand(quote! { color: red, padding: px(8) });
        assert_eq!(norm(out), "Css::new().color(red).padding(px(8))");
    }

    #[test]
    fn trailing_comma_accepted() {
        let out = expand(quote! { color: red, });
        assert_eq!(norm(out), "Css::new().color(red)");
    }

    #[test]
    fn partial_ident_only_emits_unit_arg() {
        // Cursor sits right after `back` — no `:` yet. The expansion
        // still surfaces `.back(())` so RA's method-name completion
        // fires on the `.back` part.
        let out = expand(quote! { back });
        assert_eq!(norm(out), "Css::new().back(())");
    }

    #[test]
    fn partial_ident_with_colon_no_value_emits_unit_arg() {
        // `color:` with no expression after — common when the user
        // is about to start typing the value. We still emit a
        // method call so RA can complete the value position.
        let out = expand(quote! { color: });
        assert_eq!(norm(out), "Css::new().color(())");
    }

    #[test]
    fn partial_kwarg_after_complete_ones_keeps_both() {
        // Earlier complete kwargs survive; the trailing partial
        // contributes a `.<name>(())` so the user still gets
        // method-name completion at the cursor.
        let out = expand(quote! { color: red, back });
        assert_eq!(norm(out), "Css::new().color(red).back(())");
    }

    #[test]
    fn complete_value_with_tuple_passes_through() {
        let out = expand(quote! { padding: (px(8), px(16)) });
        // Whitespace-normalised; the `:` etc. is fine inside the call.
        assert_eq!(norm(out), "Css::new().padding((px(8),px(16)))");
    }

    #[test]
    fn unparseable_value_falls_back_to_unit() {
        // `color: !` is not a valid expression. The parser bails
        // on the value and we emit `.color(())` so the call shape
        // survives for RA.
        let out = expand(quote! { color: ! });
        assert_eq!(norm(out), "Css::new().color(())");
    }
}
