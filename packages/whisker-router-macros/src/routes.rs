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
//! - `Route("path", Component, transition = <expr>)` — a screen with an
//!   explicit [`RouteTransition`]. `<expr>` is any expression yielding a
//!   `RouteTransition` (e.g. `RouteTransition::modal()`); omitted, the route
//!   uses the platform default. The transition is keyed by route id, so a
//!   shared route must declare the **same** transition (or only one site
//!   declares it) — conflicting transitions for one id are a compile error.
//! - `Stack { … }` / `Switch { … }` — the two containers.
//!
//! Later phases add per-route **directional** (4-slot) transitions,
//! `..spread`, typed nav targets, and branch enums.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Expr, Ident, LitStr, Token, braced, parenthesized};

/// One node in the route-tree DSL.
enum Node {
    Switch(Vec<Node>),
    Stack(Vec<Node>),
    Route {
        path: LitStr,
        component: Ident,
        /// Optional `transition = <expr>` — an expression yielding a
        /// `RouteTransition`. `None` → the platform default.
        transition: Option<Expr>,
    },
    /// `Layout(Comp) { <container> }` — chrome wrapping a container. NOT a
    /// nav node: the wrapped child takes the position, and `Comp` is recorded
    /// as that path's layout.
    Layout {
        component: Ident,
        child: Box<Node>,
    },
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
                // Optional trailing `, key = value` options (currently only
                // `transition`). Tolerates a trailing comma.
                let mut transition: Option<Expr> = None;
                while !content.is_empty() {
                    content.parse::<Token![,]>()?;
                    if content.is_empty() {
                        break; // trailing comma after the last arg
                    }
                    let key: Ident = content.parse()?;
                    content.parse::<Token![=]>()?;
                    let val: Expr = content.parse()?;
                    match key.to_string().as_str() {
                        "transition" => transition = Some(val),
                        other => {
                            return Err(syn::Error::new(
                                key.span(),
                                format!("unknown Route option `{other}`; expected `transition`"),
                            ));
                        }
                    }
                }
                Ok(Node::Route {
                    path,
                    component,
                    transition,
                })
            }
            "Layout" => {
                let args;
                parenthesized!(args in input);
                let component: Ident = args.parse()?;
                let content;
                braced!(content in input);
                let mut children = parse_nodes(&content)?;
                if children.len() != 1 {
                    return Err(syn::Error::new(
                        kw.span(),
                        "Layout(X) { … } must wrap exactly one container (a Stack or Switch)",
                    ));
                }
                Ok(Node::Layout {
                    component,
                    child: Box::new(children.remove(0)),
                })
            }
            other => Err(syn::Error::new(
                kw.span(),
                format!("expected `Switch`, `Stack`, `Route`, or `Layout`, found `{other}`"),
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

    // Collect (id → component, transition) registry entries, deduping shared
    // routes and erroring if one id maps to two different components or two
    // different transitions.
    let mut reg: Vec<RegEntry> = Vec::new();
    let mut err: Option<syn::Error> = None;
    collect(&routes.roots, &mut reg, &mut err);
    if let Some(e) = err {
        return e.to_compile_error();
    }

    let reg_inserts = reg.iter().map(|entry| {
        let RegEntry {
            id,
            component: comp,
            transition,
        } = entry;
        match transition {
            // `transition = <expr>` → register with the explicit transition.
            Some(t) => quote! {
                .route_with(
                    #id,
                    #t,
                    |_: &::whisker_router::core::RouteInstance| ::whisker::render! { #comp {} },
                )
            },
            // No transition → platform default.
            None => quote! {
                .route(
                    #id,
                    |_: &::whisker_router::core::RouteInstance| ::whisker::render! { #comp {} },
                )
            },
        }
    });

    let mut switch_n = 0usize;
    let mut layouts: Vec<(Vec<usize>, Ident)> = Vec::new();
    let root_tree = node_to_tree(&routes.roots[0], &[], &mut switch_n, &mut layouts);

    let layout_inserts = layouts.iter().map(|(path, comp)| {
        let idxs = path.iter();
        quote! {
            .with(
                ::whisker_router::core::NodePath(::std::vec![ #(#idxs),* ]),
                ::whisker_router::render::LayoutFn::new(|| ::whisker::render! { #comp {} }),
            )
        }
    });

    quote! {{
        let __registry = ::whisker_router::render::RouteRegistry::new() #(#reg_inserts)*;
        let __layouts = ::whisker_router::render::LayoutRegistry::new() #(#layout_inserts)*;
        let __tree = ::whisker_router::core::CompiledTree::new(#root_tree);
        ::whisker_router::render::RouteSet::from_parts_with_layouts(__tree, __registry, __layouts)
    }}
}

/// One collected registry entry: a route id, its component, and its
/// optional `transition` expression.
struct RegEntry {
    id: String,
    component: Ident,
    transition: Option<Expr>,
}

fn collect(nodes: &[Node], reg: &mut Vec<RegEntry>, err: &mut Option<syn::Error>) {
    fn push_err(err: &mut Option<syn::Error>, e: syn::Error) {
        match err {
            Some(p) => p.combine(e),
            None => *err = Some(e),
        }
    }
    for node in nodes {
        match node {
            Node::Switch(children) | Node::Stack(children) => collect(children, reg, err),
            Node::Layout { child, .. } => collect(std::slice::from_ref(child.as_ref()), reg, err),
            Node::Route {
                component,
                transition,
                ..
            } => {
                let id = snake_case(component);
                match reg.iter_mut().find(|e| e.id == id) {
                    Some(existing) => {
                        // Shared route: must agree on the component…
                        if &existing.component != component {
                            push_err(
                                err,
                                syn::Error::new(
                                    component.span(),
                                    format!(
                                        "route id `{id}` maps to both `{}` and `{component}`; \
                                         routes sharing an id must use the same component \
                                         (a shared route)",
                                        existing.component
                                    ),
                                ),
                            );
                        }
                        // …and on the transition. The transition is keyed by
                        // id, so two sites can't give it different ones. If
                        // only one site declares it, that wins; if both do,
                        // they must be token-identical.
                        match (&existing.transition, transition) {
                            (Some(a), Some(b))
                                if quote!(#a).to_string() != quote!(#b).to_string() =>
                            {
                                push_err(
                                    err,
                                    syn::Error::new(
                                        b.span(),
                                        format!(
                                            "route id `{id}` declares two different transitions; \
                                             a shared route's transition must match at every site \
                                             (per-site transitions need the 4-slot form, not yet \
                                             implemented)"
                                        ),
                                    ),
                                );
                            }
                            (None, Some(b)) => existing.transition = Some(b.clone()),
                            _ => { /* same transition, or none new — keep existing */ }
                        }
                    }
                    None => reg.push(RegEntry {
                        id,
                        component: component.clone(),
                        transition: transition.clone(),
                    }),
                }
            }
        }
    }
}

/// Emit the `RouteTree` for `node`, threading its compile-time `path` so
/// `Layout(X)` wrappers can register their chrome against the wrapped
/// container's path. `layouts` accumulates `(path, layout component)`.
fn node_to_tree(
    node: &Node,
    path: &[usize],
    switch_n: &mut usize,
    layouts: &mut Vec<(Vec<usize>, Ident)>,
) -> TokenStream {
    match node {
        Node::Route {
            path: seg,
            component,
            ..
        } => {
            let id = snake_case(component);
            quote! { ::whisker_router::core::RouteTree::route(#seg, #id) }
        }
        Node::Stack(children) => {
            let kids = children.iter().enumerate().map(|(i, c)| {
                let mut child = path.to_vec();
                child.push(i);
                node_to_tree(c, &child, switch_n, layouts)
            });
            quote! { ::whisker_router::core::RouteTree::stack(::std::vec![ #(#kids),* ]) }
        }
        Node::Switch(children) => {
            let id = format!("switch_{}", *switch_n);
            *switch_n += 1;
            let kids = children.iter().enumerate().map(|(i, c)| {
                let mut child = path.to_vec();
                child.push(i);
                node_to_tree(c, &child, switch_n, layouts)
            });
            quote! {
                ::whisker_router::core::RouteTree::switch(
                    ::whisker_router::core::SwitchDef::new(#id, 0usize),
                    ::std::vec![ #(#kids),* ],
                )
            }
        }
        Node::Layout { component, child } => {
            // Layout is transparent in the nav tree: the wrapped child takes
            // this position. Record the chrome at this path, then emit the
            // child here.
            layouts.push((path.to_vec(), component.clone()));
            node_to_tree(child, path, switch_n, layouts)
        }
    }
}
