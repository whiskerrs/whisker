//! The `routes!` macro — lowers a declarative route tree into a
//! `RouteSet` (a compiled tree + its id → component registry).
//!
//! Grammar:
//!
//! ```ignore
//! routes! {
//!     Switch {
//!         Route(path: "(home)", component: TabLayout) {
//!             Stack {
//!                 Route(path: "", component: Home)
//!                 Route(path: "detail/:id", component: Detail)
//!             }
//!         }
//!         Route(path: "(search)", component: TabLayout) {
//!             Stack {
//!                 Route(path: "search", component: Search)
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! - `Route(path: "segment", component: Comp) { children }` — a named route
//!   with a component and child routes. The component renders with an `Outlet`
//!   for the active child (expo-router's `_layout.tsx` model).
//! - `Route(path: "segment", component: Comp)` — a leaf route (no children).
//! - `Route(path: "segment") { children }` — a structural route with no
//!   component (grouping only, expo-router's `(group)` folder).
//! - `Route(component: Comp) { children }` — a pathless route with a layout
//!   component.
//! - `Stack { … }` / `Switch { … }` — the two containers.
//! - `..frag` — **spread** a reusable [`RouteFragment`].
//!
//! Route IDs are derived from the component name in snake_case. Routes without
//! a component get their ID from the path segment (or a generated ID for
//! pathless/group routes).

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Expr, Ident, LitStr, Token, braced, parenthesized};

/// One node in the route-tree DSL.
///
/// Container nodes keep their `kw` (the `Stack` / `Switch` / `Route`
/// keyword `Ident`, with its source span) so the expansion can emit a
/// span-carrying `whisker_router::__kw` reference — that's what gives the
/// keyword rust-analyzer completion AND go-to-definition / hover.
enum Node {
    Switch {
        kw: Ident,
        children: Vec<Node>,
    },
    Stack {
        kw: Ident,
        children: Vec<Node>,
    },
    Route {
        kw: Ident,
        path: Option<LitStr>,
        component: Option<Ident>,
        transition: Option<Expr>,
        children: Vec<Node>,
    },
    /// `..frag` — splice a [`RouteFragment`] value's routes in at this
    /// position.
    Spread(Expr),
    /// An **unknown / half-typed keyword** (e.g. `Sta|` mid-edit). Kept so
    /// the macro still expands and emits a span-carrying probe into
    /// `whisker_router::__kw`, letting rust-analyzer complete the keyword.
    Unknown(Ident),
}

impl Parse for Node {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // `..frag` — a spread of a `RouteFragment` value.
        if input.peek(Token![..]) {
            input.parse::<Token![..]>()?;
            let expr: Expr = input.parse()?;
            return Ok(Node::Spread(expr));
        }
        let kw: Ident = input.parse()?;
        match kw.to_string().as_str() {
            "Switch" => {
                let content;
                braced!(content in input);
                let children = parse_nodes(&content)?;
                if children.is_empty() {
                    return Err(syn::Error::new(
                        kw.span(),
                        "`Switch { }` needs at least one branch",
                    ));
                }
                Ok(Node::Switch { kw, children })
            }
            "Stack" => {
                let content;
                braced!(content in input);
                let children = parse_nodes(&content)?;
                if children.is_empty() {
                    return Err(syn::Error::new(
                        kw.span(),
                        "`Stack { }` needs at least one route or container",
                    ));
                }
                Ok(Node::Stack { kw, children })
            }
            "Route" => parse_route(input, kw),
            _ => {
                // Unknown keyword — possibly half-typed. Absorb an optional
                // braced or parenthesised body so the rest of the stream
                // stays parseable, then emit an `Unknown` node (the RA probe).
                if input.peek(syn::token::Paren) {
                    let _content;
                    parenthesized!(_content in input);
                }
                if input.peek(syn::token::Brace) {
                    let _content;
                    braced!(_content in input);
                }
                Ok(Node::Unknown(kw))
            }
        }
    }
}

/// Parse a `Route(...)` node with named keyword arguments.
///
/// Supported kwargs: `path`, `component`, `transition`.
fn parse_route(input: ParseStream, kw: Ident) -> syn::Result<Node> {
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
                "path" => {
                    path = Some(content.parse()?);
                }
                "component" => {
                    component = Some(content.parse()?);
                }
                "transition" => {
                    transition = Some(content.parse()?);
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "unknown Route option `{other}`; expected `path`, `component`, \
                             or `transition`"
                        ),
                    ));
                }
            }
            // Eat optional trailing comma.
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }
    }

    // Parse optional braced children.
    let children = if input.peek(syn::token::Brace) {
        let content;
        braced!(content in input);
        parse_nodes(&content)?
    } else {
        Vec::new()
    };

    // A Route must have at least a path or a component.
    if path.is_none() && component.is_none() {
        return Err(syn::Error::new(
            kw.span(),
            "`Route` must have at least a `path` or a `component`",
        ));
    }

    Ok(Node::Route {
        kw,
        path,
        component,
        transition,
        children,
    })
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

/// Derive a route ID from a `Route` node. Component name wins (snake_case);
/// if no component, use the path segment as-is; if neither, fall back to
/// `"route"`.
fn route_id(component: &Option<Ident>, path: &Option<LitStr>) -> String {
    if let Some(comp) = component {
        return snake_case(comp);
    }
    if let Some(p) = path {
        let seg = p.value();
        if seg.is_empty() {
            return "index".to_string();
        }
        seg
    } else {
        "route".to_string()
    }
}

/// Detect whether a path literal is a group segment: `(name)`.
fn is_group_path(path: &LitStr) -> bool {
    let v = path.value();
    v.starts_with('(') && v.ends_with(')')
}

pub fn expand(routes: Routes) -> TokenStream {
    if routes.roots.is_empty() {
        return syn::Error::new(
            Span::call_site(),
            "routes! { … } must contain at least one `Route` or container",
        )
        .to_compile_error();
    }

    // A single container at the top → a rooted `RouteSet`.
    // Anything else → a spreadable `RouteFragment`.
    let is_rooted = routes.roots.len() == 1
        && matches!(
            routes.roots[0],
            Node::Stack { .. } | Node::Switch { .. } | Node::Route { .. }
        );

    let mut err: Option<syn::Error> = None;
    validate(&routes.roots, &mut err);
    let mut reg: Vec<RegEntry> = Vec::new();
    let mut spreads: Vec<Expr> = Vec::new();
    collect(&routes.roots, &mut reg, &mut spreads, &mut err);
    if let Some(e) = err {
        return e.to_compile_error();
    }

    let reg_inserts = reg.iter().map(|entry| {
        let RegEntry {
            id,
            component: comp,
            transition,
        } = entry;
        let comp = comp
            .as_ref()
            .expect("registry entries always have a component");
        match transition {
            Some(t) => quote! {
                .route_with(
                    #id,
                    #t,
                    |_: &::whisker_router::core::RouteInstance| #comp(#comp::builder().build()),
                )
            },
            None => quote! {
                .route(
                    #id,
                    |_: &::whisker_router::core::RouteInstance| #comp(#comp::builder().build()),
                )
            },
        }
    });

    let spread_merges = dedup_exprs(&spreads).into_iter().map(|e| {
        quote! {
            .merge(::whisker_router::render::RouteFragment::registry(&(#e)))
        }
    });

    let registry_expr = quote! {
        ::whisker_router::render::RouteRegistry::new()
            #(#reg_inserts)*
            #(#spread_merges)*
    };

    let mut switch_n = 0usize;
    let mut layouts: Vec<(Vec<usize>, Ident)> = Vec::new();

    if is_rooted {
        let root_tree = node_to_tree(&routes.roots[0], &[], &mut switch_n, &mut layouts);
        let layout_inserts = layout_inserts(&layouts);
        quote! {{
            let __registry = #registry_expr;
            let __layouts = ::whisker_router::render::LayoutRegistry::new() #(#layout_inserts)*;
            let __tree = ::whisker_router::core::CompiledTree::new(#root_tree);
            ::whisker_router::render::RouteSet::from_parts_with_layouts(
                __tree, __registry, __layouts,
            )
        }}
    } else {
        let roots = children_vec_tokens(&routes.roots, &[], &mut switch_n, &mut layouts);
        if !layouts.is_empty() {
            return syn::Error::new(
                Span::call_site(),
                "a spreadable `routes!` fragment cannot contain layout routes; \
                 declare layout routes in the rooted `routes!` that consumes the fragment",
            )
            .to_compile_error();
        }
        quote! {{
            let __registry = #registry_expr;
            let __roots = #roots;
            ::whisker_router::render::RouteFragment::new(__roots, __registry)
        }}
    }
}

/// Emit the `LayoutRegistry` `.with(path, layout)` inserts.
fn layout_inserts(layouts: &[(Vec<usize>, Ident)]) -> Vec<TokenStream> {
    layouts
        .iter()
        .map(|(path, comp)| {
            let idxs = path.iter();
            quote! {
                .with(
                    ::whisker_router::core::NodePath(::std::vec![ #(#idxs),* ]),
                    ::whisker_router::render::LayoutFn::new(|| #comp(#comp::builder().build())),
                )
            }
        })
        .collect()
}

/// Distinct spread expressions (by token text), preserving first-seen order.
fn dedup_exprs(exprs: &[Expr]) -> Vec<Expr> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for e in exprs {
        if seen.insert(quote!(#e).to_string()) {
            out.push(e.clone());
        }
    }
    out
}

fn push_err(err: &mut Option<syn::Error>, e: syn::Error) {
    match err {
        Some(p) => p.combine(e),
        None => *err = Some(e),
    }
}

/// Enforce the route-tree's parent/child structure rules.
fn validate(nodes: &[Node], err: &mut Option<syn::Error>) {
    for node in nodes {
        match node {
            Node::Switch { children, .. } => {
                for c in children {
                    match c {
                        // A Switch branch must be a Route-with-children, Stack,
                        // Switch, or Unknown (half-typed).
                        Node::Stack { .. } | Node::Switch { .. } | Node::Unknown(_) => {}
                        Node::Route { children, .. } if !children.is_empty() => {}
                        Node::Route { kw, .. } => push_err(
                            err,
                            syn::Error::new(
                                kw.span(),
                                "a `Switch` branch must be a container (Route with children, \
                                 Stack, or Switch); a leaf `Route` can't be a tab — \
                                 wrap it in `Stack { … }` or give it children",
                            ),
                        ),
                        Node::Spread(expr) => push_err(
                            err,
                            syn::Error::new(
                                expr.span(),
                                "a `Switch` branch must be a container; \
                                 `..spread` yields routes — put the spread inside \
                                 a branch's `Stack { … }`",
                            ),
                        ),
                    }
                }
                validate(children, err);
            }
            Node::Stack { children, .. } => validate(children, err),
            Node::Route { children, .. } => validate(children, err),
            Node::Spread(_) | Node::Unknown(_) => {}
        }
    }
}

/// One collected registry entry.
struct RegEntry {
    id: String,
    component: Option<Ident>,
    transition: Option<Expr>,
}

fn collect(
    nodes: &[Node],
    reg: &mut Vec<RegEntry>,
    spreads: &mut Vec<Expr>,
    err: &mut Option<syn::Error>,
) {
    for node in nodes {
        match node {
            Node::Switch { children, .. } | Node::Stack { children, .. } => {
                collect(children, reg, spreads, err)
            }
            Node::Spread(expr) => spreads.push(expr.clone()),
            Node::Unknown(_) => {}
            Node::Route {
                path,
                component,
                transition,
                children,
                ..
            } => {
                // Only register routes that have a component.
                if let Some(comp) = component {
                    let id = route_id(&Some(comp.clone()), path);
                    match reg.iter_mut().find(|e| e.id == id) {
                        Some(existing) => {
                            if existing.component.as_ref() != Some(comp) {
                                push_err(
                                    err,
                                    syn::Error::new(
                                        comp.span(),
                                        format!(
                                            "route id `{id}` maps to both `{}` and `{comp}`; \
                                             routes sharing an id must use the same component",
                                            existing
                                                .component
                                                .as_ref()
                                                .map(|c| c.to_string())
                                                .unwrap_or_default()
                                        ),
                                    ),
                                );
                            }
                            match (&existing.transition, transition) {
                                (Some(a), Some(b))
                                    if quote!(#a).to_string() != quote!(#b).to_string() =>
                                {
                                    push_err(
                                        err,
                                        syn::Error::new(
                                            b.span(),
                                            format!(
                                                "route id `{id}` declares two different transitions"
                                            ),
                                        ),
                                    );
                                }
                                (None, Some(b)) => existing.transition = Some(b.clone()),
                                _ => {}
                            }
                        }
                        None => reg.push(RegEntry {
                            id,
                            component: Some(comp.clone()),
                            transition: transition.clone(),
                        }),
                    }
                }
                // Recurse into children.
                collect(children, reg, spreads, err);
            }
        }
    }
}

/// Emit the `RouteTree` for `node`.
fn node_to_tree(
    node: &Node,
    path: &[usize],
    switch_n: &mut usize,
    layouts: &mut Vec<(Vec<usize>, Ident)>,
) -> TokenStream {
    match node {
        Node::Route {
            kw,
            path: seg,
            component,
            children,
            ..
        } => {
            let id = route_id(component, seg);
            let anchor = kw_anchor(kw);

            // Build the RouteDef.
            let segment_expr = match seg {
                Some(s) => quote! { ::std::option::Option::Some(::std::string::String::from(#s)) },
                None => quote! { ::std::option::Option::None },
            };
            let component_expr = match component {
                Some(_) => {
                    let id_str = &id;
                    quote! { ::std::option::Option::Some(::std::string::String::from(#id_str)) }
                }
                None => quote! { ::std::option::Option::None },
            };
            let is_group = seg.as_ref().map(is_group_path).unwrap_or(false);

            // If this Route has a component and children, it's a layout route:
            // register it in the layout registry.
            if component.is_some() && !children.is_empty() {
                layouts.push((path.to_vec(), component.as_ref().unwrap().clone()));
            }

            let kids = if children.is_empty() {
                quote! { ::std::vec::Vec::new() }
            } else {
                children_vec_tokens(children, path, switch_n, layouts)
            };

            // Extract params from the path segment.
            let params: Vec<String> = seg
                .as_ref()
                .map(|s| {
                    s.value()
                        .split('/')
                        .filter(|s| s.starts_with(':'))
                        .map(|s| s[1..].to_string())
                        .collect()
                })
                .unwrap_or_default();
            let params_expr = params.iter();

            quote! {{
                #anchor
                ::whisker_router::core::RouteTree::route_with(
                    ::whisker_router::core::RouteDef {
                        segment: #segment_expr,
                        id: ::std::string::String::from(#id),
                        params: ::std::vec![ #(::std::string::String::from(#params_expr)),* ],
                        component: #component_expr,
                        is_group: #is_group,
                    },
                    #kids,
                )
            }}
        }
        Node::Stack { kw, children } => {
            let kids = children_vec_tokens(children, path, switch_n, layouts);
            let anchor = kw_anchor(kw);
            quote! {{ #anchor ::whisker_router::core::RouteTree::stack(#kids) }}
        }
        Node::Switch { kw, children } => {
            let id = format!("switch_{}", *switch_n);
            *switch_n += 1;
            let kids = children_vec_tokens(children, path, switch_n, layouts);
            let anchor = kw_anchor(kw);
            quote! {{
                #anchor
                ::whisker_router::core::RouteTree::switch(
                    ::whisker_router::core::SwitchDef::new(#id, 0usize),
                    #kids,
                )
            }}
        }
        Node::Spread(_) => syn::Error::new(
            Span::call_site(),
            "`..spread` must be a direct child of a `Stack` or `Switch`",
        )
        .to_compile_error(),
        Node::Unknown(kw) => {
            let anchor = kw_anchor(kw);
            quote! {{
                #anchor
                ::whisker_router::core::RouteTree::route_with(
                    ::whisker_router::core::RouteDef {
                        segment: ::std::option::Option::Some(::std::string::String::from("")),
                        id: ::std::string::String::from(""),
                        params: ::std::vec::Vec::new(),
                        component: ::std::option::Option::None,
                        is_group: false,
                    },
                    ::std::vec::Vec::new(),
                )
            }}
        }
    }
}

/// A span-carrying reference into `whisker_router::__kw` for the keyword `kw`.
fn kw_anchor(kw: &Ident) -> TokenStream {
    quote! { #[allow(unused, clippy::let_unit_value)] let _ = ::whisker_router::__kw::#kw; }
}

/// Emit a `Vec<RouteTree>` expression for a container's `children`.
fn children_vec_tokens(
    children: &[Node],
    path: &[usize],
    switch_n: &mut usize,
    layouts: &mut Vec<(Vec<usize>, Ident)>,
) -> TokenStream {
    let has_spread = children.iter().any(|c| matches!(c, Node::Spread(_)));
    if !has_spread {
        let kids = children.iter().enumerate().map(|(i, c)| {
            let mut child = path.to_vec();
            child.push(i);
            node_to_tree(c, &child, switch_n, layouts)
        });
        return quote! { ::std::vec![ #(#kids),* ] };
    }
    let mut stmts: Vec<TokenStream> = Vec::new();
    let mut lit_index = 0usize;
    for c in children {
        match c {
            Node::Spread(expr) => stmts.push(quote! {
                __kids.extend(
                    ::whisker_router::render::RouteFragment::roots(&(#expr))
                        .iter()
                        .cloned(),
                );
            }),
            other => {
                let mut child = path.to_vec();
                child.push(lit_index);
                lit_index += 1;
                let t = node_to_tree(other, &child, switch_n, layouts);
                stmts.push(quote! { __kids.push(#t); });
            }
        }
    }
    quote! {{
        let mut __kids: ::std::vec::Vec<::whisker_router::core::RouteTree> =
            ::std::vec::Vec::new();
        #(#stmts)*
        __kids
    }}
}
