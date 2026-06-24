//! Parse-only AST for the `routes!` macro.
//!
//! This mirrors the grammar in `whisker-router-macros/src/routes.rs` but
//! retains only the parse side (no codegen). It is consumed by
//! `whisker-fmt` to reformat `routes!` bodies.

use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, LitStr, Token, braced, parenthesized};

/// One node in the route-tree DSL.
pub enum RoutesNode {
    Switch {
        kw: Ident,
        children: Vec<RoutesNode>,
    },
    Stack {
        kw: Ident,
        children: Vec<RoutesNode>,
    },
    Route {
        kw: Ident,
        path: Option<LitStr>,
        component: Option<Ident>,
        transition: Option<Expr>,
        children: Vec<RoutesNode>,
    },
    Spread(Expr),
    Unknown(Ident),
}

impl RoutesNode {
    pub fn kw_span(&self) -> Option<Span> {
        match self {
            RoutesNode::Switch { kw, .. }
            | RoutesNode::Stack { kw, .. }
            | RoutesNode::Route { kw, .. }
            | RoutesNode::Unknown(kw) => Some(kw.span()),
            RoutesNode::Spread(expr) => {
                use syn::spanned::Spanned;
                Some(expr.span())
            }
        }
    }

    pub fn children(&self) -> &[RoutesNode] {
        match self {
            RoutesNode::Switch { children, .. }
            | RoutesNode::Stack { children, .. }
            | RoutesNode::Route { children, .. } => children,
            RoutesNode::Spread(_) | RoutesNode::Unknown(_) => &[],
        }
    }
}

impl Parse for RoutesNode {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(Token![..]) {
            input.parse::<Token![..]>()?;
            let expr: Expr = input.parse()?;
            return Ok(RoutesNode::Spread(expr));
        }
        let kw: Ident = input.parse()?;
        match kw.to_string().as_str() {
            "Switch" => {
                let content;
                braced!(content in input);
                let children = parse_nodes(&content)?;
                Ok(RoutesNode::Switch { kw, children })
            }
            "Stack" => {
                let content;
                braced!(content in input);
                let children = parse_nodes(&content)?;
                Ok(RoutesNode::Stack { kw, children })
            }
            "Route" => parse_route(input, kw),
            _ => {
                if input.peek(syn::token::Paren) {
                    let _content;
                    parenthesized!(_content in input);
                }
                if input.peek(syn::token::Brace) {
                    let _content;
                    braced!(_content in input);
                }
                Ok(RoutesNode::Unknown(kw))
            }
        }
    }
}

fn parse_route(input: ParseStream, kw: Ident) -> syn::Result<RoutesNode> {
    let mut path: Option<LitStr> = None;
    let mut component: Option<Ident> = None;
    let mut transition: Option<Expr> = None;

    if input.peek(syn::token::Paren) {
        let content;
        parenthesized!(content in input);
        while !content.is_empty() {
            let key: Ident = content.parse()?;
            content.parse::<Token![:]>()?;
            match key.to_string().as_str() {
                "path" => path = Some(content.parse()?),
                "component" => component = Some(content.parse()?),
                "transition" => transition = Some(content.parse()?),
                _ => {
                    let _: Expr = content.parse()?;
                }
            }
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }
    }

    let children = if input.peek(syn::token::Brace) {
        let content;
        braced!(content in input);
        parse_nodes(&content)?
    } else {
        Vec::new()
    };

    Ok(RoutesNode::Route {
        kw,
        path,
        component,
        transition,
        children,
    })
}

fn parse_nodes(input: ParseStream) -> syn::Result<Vec<RoutesNode>> {
    let mut nodes = Vec::new();
    while !input.is_empty() {
        nodes.push(input.parse()?);
    }
    Ok(nodes)
}

/// The whole `routes! { … }` input.
pub struct RoutesInput {
    pub roots: Vec<RoutesNode>,
}

impl Parse for RoutesInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(RoutesInput {
            roots: parse_nodes(input)?,
        })
    }
}

/// Parse a `routes!` body token stream.
pub fn parse_input(ts: proc_macro2::TokenStream) -> syn::Result<RoutesInput> {
    syn::parse2(ts)
}
