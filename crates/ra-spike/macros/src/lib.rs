//! Minimal proc-macros for the rust-analyzer completion spike.
//!
//! Each macro takes `tag(name: value, …)` Compose-style input and
//! emits a different "lowered" shape. The aim is to find out which
//! lowered shape, if any, lets RA do method-name completion on the
//! kwarg slot when the user is mid-typing
//! (`compose_a!(view(sty|))`, etc.).
//!
//! All macros use the same parser; only the codegen differs.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    braced, parenthesized,
    parse::{Parse, ParseStream, Result},
    parse_macro_input, token, Expr, Ident, Token,
};

struct Input {
    tag: Ident,
    attrs: Vec<(Ident, Option<Expr>)>,
}

impl Parse for Input {
    fn parse(input: ParseStream) -> Result<Self> {
        let tag: Ident = input.parse()?;
        let mut attrs = Vec::new();
        if input.peek(token::Paren) {
            let body;
            parenthesized!(body in input);
            while !body.is_empty() {
                if !body.peek(Ident) {
                    return Err(body.error("expected `name: value` kwarg"));
                }
                let name: Ident = body.parse()?;
                let value = if body.peek(Token![:]) {
                    body.parse::<Token![:]>()?;
                    Some(body.parse::<Expr>()?)
                } else {
                    None
                };
                attrs.push((name, value));
                if body.peek(Token![,]) {
                    body.parse::<Token![,]>()?;
                }
            }
        }
        Ok(Self { tag, attrs })
    }
}

/// Element with optional kwargs and an optional child block.
/// Children are themselves Elements (no text / `{expr}` interp yet —
/// adding those later if completion needs proven on them).
struct Element {
    tag: Ident,
    attrs: Vec<(Ident, Option<Expr>)>,
    children: Vec<Element>,
}

impl Parse for Element {
    fn parse(input: ParseStream) -> Result<Self> {
        let tag: Ident = input.parse()?;
        let mut attrs = Vec::new();
        if input.peek(token::Paren) {
            let body;
            parenthesized!(body in input);
            while !body.is_empty() {
                if !body.peek(Ident) {
                    return Err(body.error("expected `name: value` kwarg"));
                }
                let name: Ident = body.parse()?;
                let value = if body.peek(Token![:]) {
                    body.parse::<Token![:]>()?;
                    Some(body.parse::<Expr>()?)
                } else {
                    None
                };
                attrs.push((name, value));
                if body.peek(Token![,]) {
                    body.parse::<Token![,]>()?;
                }
            }
        }
        let mut children = Vec::new();
        if input.peek(token::Brace) {
            let body;
            braced!(body in input);
            while !body.is_empty() {
                children.push(body.parse::<Element>()?);
            }
        }
        Ok(Self {
            tag,
            attrs,
            children,
        })
    }
}

fn emit_element(e: &Element) -> TokenStream2 {
    let tag_span = e.tag.span();
    let ctor = format_ident!("__{}_ctor", e.tag, span = tag_span);
    let attr_calls: Vec<TokenStream2> = e
        .attrs
        .iter()
        .map(|(name, value)| {
            let span = name.span();
            let value_tokens = match value {
                Some(v) => quote!(#v),
                None => quote!(()),
            };
            quote_spanned! {span=> .#name(#value_tokens) }
        })
        .collect();
    let child_calls: Vec<TokenStream2> = e
        .children
        .iter()
        .map(|c| {
            let inner = emit_element(c);
            quote! { .child(#inner) }
        })
        .collect();
    quote! {
        {
            ::ra_spike::__tags::#ctor()
                #(#attr_calls)*
                #(#child_calls)*
                .__h()
        }
    }
}

/// Variant A: emit an inline method chain on a constructor named
/// `__<tag>_ctor`. This is what whisker's current built-in path
/// uses.
#[proc_macro]
pub fn compose_a(input: TokenStream) -> TokenStream {
    let Input { tag, attrs } = parse_macro_input!(input);
    let tag_span = tag.span();
    let ctor = format_ident!("__{}_ctor", tag, span = tag_span);
    let calls: Vec<TokenStream2> = attrs
        .iter()
        .map(|(name, value)| {
            let span = name.span();
            let value_tokens = match value {
                Some(v) => quote!(#v),
                None => quote!(()),
            };
            quote_spanned! {span=> .#name(#value_tokens) }
        })
        .collect();
    quote! {
        {
            ::ra_spike::__tags::#ctor() #(#calls)* .__h()
        }
    }
    .into()
}

/// Variant B: emit through a typed local binding. Mirrors the
/// earlier whisker emission shape with `let __b: view = … ;`.
#[proc_macro]
pub fn compose_b(input: TokenStream) -> TokenStream {
    let Input { tag, attrs } = parse_macro_input!(input);
    let tag_span = tag.span();
    let ctor = format_ident!("__{}_ctor", tag, span = tag_span);
    let ty = quote_spanned!(tag_span=> ::ra_spike::__tags::#tag);
    let calls: Vec<TokenStream2> = attrs
        .iter()
        .map(|(name, value)| {
            let span = name.span();
            let value_tokens = match value {
                Some(v) => quote!(#v),
                None => quote!(()),
            };
            quote_spanned! {span=> .#name(#value_tokens) }
        })
        .collect();
    quote! {
        {
            let __b: #ty = ::ra_spike::__tags::#ctor();
            let __h = __b #(#calls)* .__h();
            __h
        }
    }
    .into()
}

/// Variant C: emit through a `XxxProps::builder()` typed-builder
/// chain. Mirrors whisker's user-component emission shape (the
/// one that *does* work for completion today).
#[proc_macro]
pub fn compose_c(input: TokenStream) -> TokenStream {
    let Input { tag, attrs } = parse_macro_input!(input);
    let tag_span = tag.span();
    let props_ident = {
        let s = tag.to_string();
        let mut out = String::new();
        let mut upper_next = true;
        for c in s.chars() {
            if c == '_' {
                upper_next = true;
                continue;
            }
            if upper_next {
                out.extend(c.to_uppercase());
                upper_next = false;
            } else {
                out.push(c);
            }
        }
        out.push_str("Props");
        Ident::new(&out, tag_span)
    };
    let calls: Vec<TokenStream2> = attrs
        .iter()
        .map(|(name, value)| {
            let span = name.span();
            let value_tokens = match value {
                Some(v) => quote!(#v),
                None => quote!(()),
            };
            quote_spanned! {span=> .#name(#value_tokens) }
        })
        .collect();
    quote! {
        {
            ::ra_spike::#tag(
                ::ra_spike::#props_ident::builder() #(#calls)* .build()
            )
        }
    }
    .into()
}

/// Variant D: full compose `tag(props) { children }`. Same inline
/// chain shape as A, with `.child({ inner_chain })` appended for
/// each nested element. No intermediate `let __h = …; __h` binding
/// at any level — that's the form that broke completion when tested
/// against the let-binding variant earlier.
#[proc_macro]
pub fn render(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as Element);
    emit_element(&root).into()
}
