//! The `routes!` macro — lowers a declarative route tree into a
//! `RouteSet` (a compiled tree + its id → component registry).
//!
//! Grammar (this phase):
//!
//! ```ignore
//! routes! {
//!     Switch {
//!         Stack { Route("", Home)  Route("detail/:id", Detail) }
//!         Stack { Route("list", List)  Route("detail/:id", Detail) }
//!     }
//! }
//! ```
//!
//! - `Route("path", Component)` — a screen. Its **id is the component name
//!   in snake_case** (`Detail` → `"detail"`); the same component routed in
//!   several stacks shares that id (a shared route → one registry entry).
//!   The component reads its own `:param`s via `use_param`.
//! - `Stack { … }` / `Switch { … }` — the two containers.
//!
//! Later phases add `Layout(…)`, per-route transitions, `..spread`, typed
//! nav targets, and branch enums.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token, braced, parenthesized};

/// One node in the route-tree DSL.
enum Node {
    Switch(Vec<Node>),
    Stack(Vec<Node>),
    Route { path: LitStr, component: Ident },
}

impl Parse for Node {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let kw: Ident = input.parse()?;
        match kw.to_string().as_str() {
            "Switch" => {
                let content;
                braced!(content in input);
                Ok(Node::Switch(parse_nodes(&content)?))
            }
            "Stack" => {
                let content;
                braced!(content in input);
                Ok(Node::Stack(parse_nodes(&content)?))
            }
            "Route" => {
                let content;
                parenthesized!(content in input);
                let path: LitStr = content.parse()?;
                content.parse::<Token![,]>()?;
                let component: Ident = content.parse()?;
                Ok(Node::Route { path, component })
            }
            other => Err(syn::Error::new(
                kw.span(),
                format!("expected `Switch`, `Stack`, or `Route`, found `{other}`"),
            )),
        }
    }
}

fn parse_nodes(input: ParseStream) -> syn::Result<Vec<Node>> {
    let mut nodes = Vec::new();
    while !input.is_empty() {
        nodes.push(input.parse()?);
    }
    Ok(nodes)
}

/// The whole `routes! { … }` input.
pub struct Routes {
    roots: Vec<Node>,
}

impl Parse for Routes {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Routes {
            roots: parse_nodes(input)?,
        })
    }
}

/// snake_case a PascalCase component name (`ListScreen` → `list_screen`).
fn snake_case(ident: &Ident) -> String {
    let s = ident.to_string();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn expand(routes: Routes) -> TokenStream {
    // This phase requires exactly one rooted node (a `Stack`/`Switch` tree).
    // Multi-node fragments are for the future `..spread` feature.
    if routes.roots.len() != 1 {
        return syn::Error::new(
            Span::call_site(),
            "routes! currently requires a single root `Stack { … }` or `Switch { … }`",
        )
        .to_compile_error();
    }

    // Collect (id → component) registry entries, deduping shared routes and
    // erroring if one id maps to two different components.
    let mut reg: Vec<(String, Ident)> = Vec::new();
    let mut err: Option<syn::Error> = None;
    collect(&routes.roots, &mut reg, &mut err);
    if let Some(e) = err {
        return e.to_compile_error();
    }

    let reg_inserts = reg.iter().map(|(id, comp)| {
        quote! {
            .route(
                #id,
                |_: &::whisker_router::core::RouteInstance| ::whisker::render! { #comp {} },
            )
        }
    });

    let mut switch_n = 0usize;
    let root_tree = node_to_tree(&routes.roots[0], &mut switch_n);

    quote! {{
        let __registry = ::whisker_router::render::RouteRegistry::new() #(#reg_inserts)*;
        let __tree = ::whisker_router::core::CompiledTree::new(#root_tree);
        ::whisker_router::render::RouteSet::from_parts(__tree, __registry)
    }}
}

fn collect(nodes: &[Node], reg: &mut Vec<(String, Ident)>, err: &mut Option<syn::Error>) {
    for node in nodes {
        match node {
            Node::Switch(children) | Node::Stack(children) => collect(children, reg, err),
            Node::Route { component, .. } => {
                let id = snake_case(component);
                // Clone the existing ident so the immutable borrow ends
                // before the `reg.push` below.
                let existing = reg.iter().find(|(i, _)| *i == id).map(|(_, c)| c.clone());
                match existing {
                    Some(prev) if &prev != component => {
                        let e = syn::Error::new(
                            component.span(),
                            format!(
                                "route id `{id}` maps to both `{prev}` and `{component}`; \
                                 routes sharing an id must use the same component (a shared route)"
                            ),
                        );
                        match err {
                            Some(p) => p.combine(e),
                            None => *err = Some(e),
                        }
                    }
                    Some(_) => { /* shared route — already registered once */ }
                    None => reg.push((id, component.clone())),
                }
            }
        }
    }
}

fn node_to_tree(node: &Node, switch_n: &mut usize) -> TokenStream {
    match node {
        Node::Route { path, component } => {
            let id = snake_case(component);
            quote! { ::whisker_router::core::RouteTree::route(#path, #id) }
        }
        Node::Stack(children) => {
            let kids = children.iter().map(|c| node_to_tree(c, switch_n));
            quote! { ::whisker_router::core::RouteTree::stack(::std::vec![ #(#kids),* ]) }
        }
        Node::Switch(children) => {
            let id = format!("switch_{}", *switch_n);
            *switch_n += 1;
            let kids = children.iter().map(|c| node_to_tree(c, switch_n));
            quote! {
                ::whisker_router::core::RouteTree::switch(
                    ::whisker_router::core::SwitchDef::new(#id, 0usize),
                    ::std::vec![ #(#kids),* ],
                )
            }
        }
    }
}
