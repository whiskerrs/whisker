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
//! a component get their ID from the path Segment (or a generated ID for
//! pathless/group routes).

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::ParseStream;
use syn::spanned::Spanned;
use syn::{Expr, ExprLit, ExprPath, Ident, Lit, LitStr, Path};
use whisker_macro_syntax::compose::{ComposeArgument, ComposeChild, ComposeInput, ComposeNode};

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
        component: Option<Path>,
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

fn route_from_compose(node: ComposeNode) -> syn::Result<Node> {
    let kw = node
        .path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new(node.path.span(), "expected a route builder name"))?
        .ident
        .clone();
    let children = compose_children(node.body)?;
    match kw.to_string().as_str() {
        "Switch" => {
            reject_arguments(&kw, &node.arguments)?;
            if children.is_empty() {
                return Err(syn::Error::new(
                    kw.span(),
                    "`Switch { }` needs at least one branch",
                ));
            }
            Ok(Node::Switch { kw, children })
        }
        "Stack" => {
            reject_arguments(&kw, &node.arguments)?;
            if children.is_empty() {
                return Err(syn::Error::new(
                    kw.span(),
                    "`Stack { }` needs at least one route or container",
                ));
            }
            Ok(Node::Stack { kw, children })
        }
        "Route" => route_from_arguments(kw, node.arguments, children),
        _ => Ok(Node::Unknown(kw)),
    }
}

fn route_from_arguments(
    kw: Ident,
    arguments: Vec<ComposeArgument>,
    children: Vec<Node>,
) -> syn::Result<Node> {
    let mut path: Option<LitStr> = None;
    let mut component: Option<Path> = None;
    let mut transition: Option<Expr> = None;
    for argument in arguments {
        match argument.name.to_string().as_str() {
            "path" => match argument.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) => path = Some(value),
                value => {
                    return Err(syn::Error::new(
                        value.span(),
                        "`path` must be a string literal",
                    ));
                }
            },
            "component" => match argument.value {
                Expr::Path(ExprPath { path: value, .. }) => component = Some(value),
                value => {
                    return Err(syn::Error::new(
                        value.span(),
                        "`component` must be a component path",
                    ));
                }
            },
            "transition" => transition = Some(argument.value),
            other => {
                return Err(syn::Error::new(
                    argument.name.span(),
                    format!(
                        "unknown Route option `{other}`; expected `path`, `component`, or `transition`"
                    ),
                ));
            }
        }
    }

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

fn compose_children(children: Vec<ComposeChild>) -> syn::Result<Vec<Node>> {
    children
        .into_iter()
        .map(|child| match child {
            ComposeChild::Node(node) => route_from_compose(node),
            ComposeChild::Spread(expr) => Ok(Node::Spread(expr)),
            ComposeChild::Text(value) => Err(syn::Error::new(
                value.span(),
                "route bodies cannot contain text",
            )),
            ComposeChild::Expression(value) => Err(syn::Error::new(
                value.span(),
                "use `..fragment` to splice route values",
            )),
        })
        .collect()
}

fn reject_arguments(kw: &Ident, arguments: &[ComposeArgument]) -> syn::Result<()> {
    if let Some(argument) = arguments.first() {
        Err(syn::Error::new(
            argument.name.span(),
            format!("`{kw}` does not accept arguments"),
        ))
    } else {
        Ok(())
    }
}

/// The whole `routes! { … }` input.
pub struct Routes {
    roots: Vec<Node>,
}

impl syn::parse::Parse for Routes {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let input: ComposeInput = input.parse()?;
        Ok(Routes {
            roots: input
                .nodes
                .into_iter()
                .map(route_from_compose)
                .collect::<syn::Result<_>>()?,
        })
    }
}

/// snake_case a PascalCase component name (`ListScreen` → `list_screen`).
fn snake_case(path: &Path) -> String {
    let s = path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default();
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
fn route_id(component: &Option<Path>, path: &Option<LitStr>) -> String {
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
                    |_: &::whisker_router::core::RouteInstance| #comp::builder().build(),
                )
            },
            None => quote! {
                .route(
                    #id,
                    |_: &::whisker_router::core::RouteInstance| #comp::builder().build(),
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
    let mut layouts: Vec<(Vec<usize>, Path)> = Vec::new();

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
fn layout_inserts(layouts: &[(Vec<usize>, Path)]) -> Vec<TokenStream> {
    layouts
        .iter()
        .map(|(path, comp)| {
            let idxs = path.iter();
            quote! {
                .with(
                    ::whisker_router::core::NodePath(::std::vec![ #(#idxs),* ]),
                    ::whisker_router::render::LayoutFn::new(|| #comp::builder().build()),
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
    component: Option<Path>,
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
                if let Some(comp) = component {
                    let id = route_id(&Some(comp.clone()), path);
                    match reg.iter_mut().find(|e| e.id == id) {
                        Some(existing) => {
                            if existing.component.as_ref().map(path_key) != Some(path_key(comp)) {
                                push_err(
                                    err,
                                    syn::Error::new(
                                        comp.span(),
                                        format!(
                                            "route id `{id}` maps to both `{}` and `{}`; \
                                             routes sharing an id must use the same component",
                                            existing
                                                .component
                                                .as_ref()
                                                .map(path_key)
                                                .unwrap_or_default(),
                                            path_key(comp),
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
                collect(children, reg, spreads, err);
            }
        }
    }
}

fn path_key(path: &Path) -> String {
    quote!(#path).to_string().replace(" :: ", "::")
}

/// Emit the `RouteTree` for `node`.
fn node_to_tree(
    node: &Node,
    path: &[usize],
    switch_n: &mut usize,
    layouts: &mut Vec<(Vec<usize>, Path)>,
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

            if component.is_some() && !children.is_empty() {
                layouts.push((path.to_vec(), component.as_ref().unwrap().clone()));
            }

            let kids = if children.is_empty() {
                quote! { ::std::vec::Vec::new() }
            } else {
                children_vec_tokens(children, path, switch_n, layouts)
            };

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
            quote! {{ #anchor ::whisker_router::core::RouteTree::Stack(#kids) }}
        }
        Node::Switch { kw, children } => {
            let id = format!("switch_{}", *switch_n);
            *switch_n += 1;
            let kids = children_vec_tokens(children, path, switch_n, layouts);
            let anchor = kw_anchor(kw);
            quote! {{
                #anchor
                ::whisker_router::core::RouteTree::Switch(
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
    layouts: &mut Vec<(Vec<usize>, Path)>,
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
