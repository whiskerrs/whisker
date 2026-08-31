//! Shared syntax tree for Whisker's builder-composition macros.
//!
//! A node is deliberately unaware of whether it denotes an Element, a Rust
//! component, or a router node. Meaning comes exclusively from the public
//! builder resolved by its Rust path.

use proc_macro2::TokenStream as TokenStream2;
use syn::{
    Expr, Ident, LitStr, Path, Token, braced,
    ext::IdentExt,
    parenthesized,
    parse::{Parse, ParseStream},
    token,
};

pub struct ComposeInput {
    pub nodes: Vec<ComposeNode>,
}

pub struct ComposeNode {
    pub path: Path,
    pub arguments: Vec<ComposeArgument>,
    pub body: Vec<ComposeChild>,
    pub has_body: bool,
}

pub struct ComposeArgument {
    pub name: Ident,
    pub value: Expr,
    pub partial: bool,
}

pub struct ComposeArguments {
    pub arguments: Vec<ComposeArgument>,
}

pub enum ComposeChild {
    Node(ComposeNode),
    Text(LitStr),
    Expression(Expr),
    Spread(Expr),
}

impl Parse for ComposeInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut nodes = Vec::new();
        while !input.is_empty() {
            nodes.push(input.parse()?);
        }
        Ok(Self { nodes })
    }
}

impl Parse for ComposeArguments {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            arguments: parse_arguments(input)?,
        })
    }
}

impl Parse for ComposeNode {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: Path = input.parse()?;
        let arguments = if input.peek(token::Paren) {
            let content;
            parenthesized!(content in input);
            parse_arguments(&content)?
        } else {
            Vec::new()
        };
        let has_body = input.peek(token::Brace);
        let body = if has_body {
            let content;
            braced!(content in input);
            parse_children(&content)?
        } else {
            Vec::new()
        };
        Ok(Self {
            path,
            arguments,
            body,
            has_body,
        })
    }
}

fn parse_arguments(input: ParseStream) -> syn::Result<Vec<ComposeArgument>> {
    let mut arguments = Vec::new();
    while !input.is_empty() {
        if !input.peek(Ident::peek_any) {
            return Err(input.error("arguments must use `name: expression` syntax"));
        }
        let name = input.call(Ident::parse_any)?;
        let (value, partial) = if input.peek(Token![:]) {
            input.parse::<Token![:]>()?;
            match input.parse::<Expr>() {
                Ok(value) => (value, false),
                Err(_) => (syn::parse_quote_spanned!(name.span()=> ()), true),
            }
        } else {
            (syn::parse_quote_spanned!(name.span()=> ()), true)
        };
        arguments.push(ComposeArgument {
            name,
            value,
            partial,
        });
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        } else if !input.is_empty() {
            return Err(input.error("expected `,` between named arguments"));
        }
    }
    Ok(arguments)
}

fn parse_children(input: ParseStream) -> syn::Result<Vec<ComposeChild>> {
    let mut children = Vec::new();
    while !input.is_empty() {
        if input.peek(Token![..]) {
            input.parse::<Token![..]>()?;
            children.push(ComposeChild::Spread(input.parse()?));
        } else if input.peek(LitStr) {
            children.push(ComposeChild::Text(input.parse()?));
        } else if input.peek(token::Brace) {
            let expression;
            braced!(expression in input);
            let value = expression.parse()?;
            if !expression.is_empty() {
                return Err(expression.error("a dynamic body item must contain one expression"));
            }
            children.push(ComposeChild::Expression(value));
        } else {
            children.push(ComposeChild::Node(input.parse()?));
        }
    }
    Ok(children)
}

pub fn parse_input(tokens: TokenStream2) -> syn::Result<ComposeInput> {
    syn::parse2(tokens)
}

pub fn parse_root(tokens: TokenStream2) -> syn::Result<ComposeNode> {
    let mut input = parse_input(tokens)?;
    if input.nodes.len() != 1 {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "expected exactly one root builder",
        ));
    }
    Ok(input.nodes.remove(0))
}
