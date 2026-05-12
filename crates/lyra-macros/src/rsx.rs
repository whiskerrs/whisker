//! `rsx!` macro implementation.
//!
//! Grammar (loosely):
//!
//! ```text
//! rsx_root  := node_or_text+
//! node      := IDENT "{" attr_list child_list "}"
//! attr_list := (IDENT ":" expr ",")*
//! child_list := (node_or_text)*
//! node_or_text := node | LIT_STR | "{" expr "}"
//! ```
//!
//! Reserved attribute names:
//!   - `style`         → `.style(<expr>)`
//!   - `key`           → `.key(<expr>)`
//!   - `on<Event>`     → `.on("<event>", <expr>)`
//!   - everything else → `.attr("<name>", <expr>)`
//!
//! Text children:
//!   - String literals  → `.child(::lyra_runtime::build::raw_text(<lit>))`
//!   - `{expr}` blocks  → `.child(::lyra_runtime::build::raw_text(<expr>.to_string()))`

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, ToTokens};
use syn::{
    braced,
    parse::{Parse, ParseStream, Result},
    parse_macro_input,
    punctuated::Punctuated,
    token, Expr, Ident, LitStr, Token,
};

pub fn expand(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as RsxRoot);
    let expanded = root.to_tokens_stream();
    expanded.into()
}

// ---- AST -----------------------------------------------------------------

struct RsxRoot {
    node: Node,
}

impl Parse for RsxRoot {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Self {
            node: input.parse()?,
        })
    }
}

enum Node {
    Element(ElementNode),
    Text(LitStr),
    Expr(Expr),
}

impl Parse for Node {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.peek(LitStr) {
            return Ok(Node::Text(input.parse()?));
        }
        if input.peek(token::Brace) {
            // {expr} interpolation
            let content;
            braced!(content in input);
            let expr: Expr = content.parse()?;
            return Ok(Node::Expr(expr));
        }
        Ok(Node::Element(input.parse()?))
    }
}

struct ElementNode {
    tag: Ident,
    attrs: Vec<AttrEntry>,
    children: Vec<Node>,
}

struct AttrEntry {
    name: Ident,
    value: Expr,
}

impl Parse for ElementNode {
    fn parse(input: ParseStream) -> Result<Self> {
        let tag: Ident = input.parse()?;
        let body;
        braced!(body in input);

        let mut attrs = Vec::new();
        let mut children = Vec::new();

        // Attributes: while we see `IDENT :`, parse an attribute. Once we
        // see something else, switch to children.
        while body.peek(Ident) && body.peek2(Token![:]) {
            let name: Ident = body.parse()?;
            body.parse::<Token![:]>()?;
            let value: Expr = body.parse()?;
            attrs.push(AttrEntry { name, value });
            // Optional trailing comma.
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        // Children until we hit closing brace (consumed by braced! above).
        while !body.is_empty() {
            children.push(body.parse()?);
            // Optional comma between children.
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        Ok(Self { tag, attrs, children })
    }
}

// ---- Codegen --------------------------------------------------------------

impl RsxRoot {
    fn to_tokens_stream(&self) -> TokenStream2 {
        self.node.to_tokens_stream()
    }
}

impl Node {
    fn to_tokens_stream(&self) -> TokenStream2 {
        match self {
            Node::Element(el) => el.to_tokens_stream(),
            Node::Text(lit) => quote! {
                ::lyra_runtime::build::raw_text(#lit)
            },
            Node::Expr(expr) => quote! {
                ::lyra_runtime::build::raw_text((#expr).to_string())
            },
        }
    }
}

impl ElementNode {
    fn to_tokens_stream(&self) -> TokenStream2 {
        let ctor = constructor_for(&self.tag);
        let mut chain = ctor;

        for attr in &self.attrs {
            let value = &attr.value;
            let name_str = attr.name.to_string();

            if name_str == "style" {
                chain = quote! { #chain.style(#value) };
            } else if name_str == "key" {
                chain = quote! { #chain.key(#value) };
            } else if let Some(event) = strip_on_prefix(&name_str) {
                let event_lit = LitStr::new(&event, attr.name.span());
                chain = quote! { #chain.on(#event_lit, #value) };
            } else {
                let attr_name = LitStr::new(&name_str, attr.name.span());
                chain = quote! { #chain.attr(#attr_name, #value) };
            }
        }

        for child in &self.children {
            let child_tokens = child.to_tokens_stream();
            chain = quote! { #chain.child(#child_tokens) };
        }

        chain
    }
}

fn constructor_for(tag: &Ident) -> TokenStream2 {
    let name = tag.to_string();
    let fn_name = format_ident!("{}", name);
    // Whitelist the well-known constructors. Custom tags fall through to a
    // generic `Element::new(ElementTag::View)` pending custom-tag support.
    match name.as_str() {
        "page" | "view" | "text" | "raw_text" | "image" => {
            quote! { ::lyra_runtime::build::#fn_name() }
        }
        _ => {
            let span = tag.span();
            let err = LitStr::new(
                &format!("unknown rsx tag `{}`", name),
                span,
            );
            quote! { compile_error!(#err) }
        }
    }
}

fn strip_on_prefix(name: &str) -> Option<String> {
    if let Some(rest) = name.strip_prefix("on_") {
        Some(rest.to_string())
    } else if let Some(rest) = name.strip_prefix("on") {
        // onClick → click
        if let Some(first) = rest.chars().next() {
            if first.is_uppercase() {
                let mut owned = first.to_lowercase().to_string();
                owned.push_str(&rest[first.len_utf8()..]);
                return Some(owned);
            }
        }
        None
    } else {
        None
    }
}

// Keep the punctuated import live so future grammar extensions (comma-
// separated lists in attribute values) don't need a new import.
#[allow(dead_code)]
fn _doc_links<P: Parse, T: ToTokens>(_p: Punctuated<P, T>) {}
