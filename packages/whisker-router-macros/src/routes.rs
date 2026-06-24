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
//! - `..frag` — **spread** a reusable [`RouteFragment`] (an un-rooted
//!   `routes! { Route(..) Route(..) }` value bound to a variable) into a
//!   `Stack` / `Switch`, splicing its routes in at that position:
//!
//!   ```ignore
//!   let content = routes! { Route("post/:id", Post)  Route("u/:id", Profile) };
//!   routes! {
//!       Switch {
//!           Stack { Route("", Timeline)  ..content }   // + /post/:id, /u/:id
//!           Stack { Route("me", MyPage)  ..content }   // same routes, own stack
//!       }
//!   }
//!   ```
//!
//!   A body whose top level is a single container evaluates to a `RouteSet`
//!   (→ a `RouterHandle`); a body of bare `Route`s / spreads evaluates to a
//!   spreadable `RouteFragment`. Spreading a fragment into N stacks creates N
//!   tree instances that dedupe to one nav target by id (the registry holds
//!   one entry per id; resolution picks the instance relative to the current
//!   position).
//!
//! ## Structure checks
//!
//! The macro enforces the tree's parent/child rules at compile time, with a
//! span on the offending node:
//!
//! - a `Switch` branch must be its own container (`Stack` / `Switch` /
//!   `Layout`) — a bare `Route` (no history) or a `..spread` (yields routes)
//!   is rejected;
//! - `Layout(X) { … }` must wrap exactly one `Stack` or `Switch`;
//! - `Stack { }` / `Switch { }` must be non-empty.
//!
//! Directional (enter/exit/pop-enter/pop-exit) animation is handled by the
//! `Transition` itself — its `pose` sees a `Direction`, so a single
//! `transition = <expr>` can be fully asymmetric without extra macro syntax.
//! Later phases add typed nav targets and branch enums.

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
    /// `..frag` — splice a [`RouteFragment`] value's routes in at this
    /// position. The expression must evaluate to a `RouteFragment`.
    Spread(Expr),
    /// An **unknown / half-typed keyword** (e.g. `Sta|` mid-edit). Rather than
    /// hard-erroring, we keep it so the macro still expands and emits a span-
    /// carrying probe into `whisker_router::__kw`, letting rust-analyzer
    /// complete the keyword name — the same trick `render!` uses for tag names.
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
                Ok(Node::Switch(children))
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
                Ok(Node::Stack(children))
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
                // The wrapped child must be a container — a `Layout` adds chrome
                // *around* a `Stack`/`Switch`; it can't wrap a bare `Route`,
                // a `..spread`, or another `Layout`. `Unknown` (a half-typed
                // keyword like `Swi|`) is allowed so completion survives the
                // edit — it would otherwise error and wipe the expansion.
                if !matches!(
                    children[0],
                    Node::Stack(_) | Node::Switch(_) | Node::Unknown(_)
                ) {
                    return Err(syn::Error::new(
                        kw.span(),
                        "Layout(X) { … } must wrap a `Stack` or `Switch` (not a bare `Route`, \
                         a `..spread`, or another `Layout`)",
                    ));
                }
                Ok(Node::Layout {
                    component,
                    child: Box::new(children.remove(0)),
                })
            }
            _ => {
                // Unknown / half-typed keyword. Don't hard-error (that would
                // wipe the whole expansion and kill completion). Consume any
                // following `(...)` / `{...}` groups so the rest still parses,
                // and keep the ident for the completion probe.
                if input.peek(syn::token::Paren) {
                    let _discard;
                    parenthesized!(_discard in input);
                }
                if input.peek(syn::token::Brace) {
                    let _discard;
                    braced!(_discard in input);
                }
                Ok(Node::Unknown(kw))
            }
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
    if routes.roots.is_empty() {
        return syn::Error::new(
            Span::call_site(),
            "routes! { … } must contain at least one `Route` or container",
        )
        .to_compile_error();
    }

    // A single container at the top → a rooted `RouteSet` (→ a
    // `RouterHandle`). Anything else (bare `Route`s and/or `..spread`s) → a
    // spreadable `RouteFragment` value.
    let is_rooted = routes.roots.len() == 1
        && matches!(
            routes.roots[0],
            Node::Stack(_) | Node::Switch(_) | Node::Layout { .. }
        );

    // Structure checks (parent/child constraints) + collect (id → component,
    // transition) registry entries (deduping shared routes / erroring on
    // conflicts) and the `..spread` expressions whose registries must be merged
    // in. Both accumulate into one `err` so every problem surfaces at once.
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
        // Emit the **desugared** component instantiation (`Comp(Comp::builder()
        // .build())`) that `render! { Comp {} }` lowers to, rather than nesting
        // `render!` itself. Two reasons: (1) `#comp` appears as a direct path
        // reference, so rust-analyzer can resolve + complete the component name
        // inside `routes!` (a nested proc-macro breaks RA's span mapping); (2)
        // one less expansion layer. The result is identical to the `render!`
        // form for a prop-less component.
        match transition {
            // `transition = <expr>` → register with the explicit transition.
            Some(t) => quote! {
                .route_with(
                    #id,
                    #t,
                    |_: &::whisker_router::core::RouteInstance| #comp(#comp::builder().build()),
                )
            },
            // No transition → platform default.
            None => quote! {
                .route(
                    #id,
                    |_: &::whisker_router::core::RouteInstance| #comp(#comp::builder().build()),
                )
            },
        }
    });

    // Fold each distinct spread fragment's id → render/transition entries in
    // (id-keyed, first declaration wins).
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
        // A spreadable fragment: a flat list of route roots (+ spreads). A
        // `Layout(X)` can't keep a stable compile-time path once the fragment
        // is spliced at an arbitrary position, so it isn't allowed here.
        let roots = children_vec_tokens(&routes.roots, &[], &mut switch_n, &mut layouts);
        if !layouts.is_empty() {
            return syn::Error::new(
                Span::call_site(),
                "a spreadable `routes!` fragment cannot contain a `Layout(X)`; \
                 declare layouts in the rooted `routes!` that consumes the fragment",
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

/// Emit the `LayoutRegistry` `.with(path, layout)` inserts for the collected
/// `(path, component)` layouts.
fn layout_inserts(layouts: &[(Vec<usize>, Ident)]) -> Vec<TokenStream> {
    layouts
        .iter()
        .map(|(path, comp)| {
            let idxs = path.iter();
            quote! {
                .with(
                    ::whisker_router::core::NodePath(::std::vec![ #(#idxs),* ]),
                    // Desugared form (see `expand`) so RA can complete the
                    // layout component name; equivalent to `render! { #comp {} }`.
                    ::whisker_router::render::LayoutFn::new(|| #comp(#comp::builder().build())),
                )
            }
        })
        .collect()
}

/// Distinct spread expressions (by token text), preserving first-seen order,
/// so spreading one fragment into several containers merges its registry once.
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

/// Accumulate `e` into `err`, combining with any error already collected so
/// every problem surfaces in one compile pass (not just the first).
fn push_err(err: &mut Option<syn::Error>, e: syn::Error) {
    match err {
        Some(p) => p.combine(e),
        None => *err = Some(e),
    }
}

/// Enforce the route-tree's parent/child structure rules. Empty containers and
/// `Layout`'s child kind are checked at parse time (where the keyword span is
/// in hand); this pass covers the rules that need a node's *parent* context —
/// a `Switch` branch must be its own container, never a bare `Route` (it would
/// have no history) or a `..spread` (which yields routes).
fn validate(nodes: &[Node], err: &mut Option<syn::Error>) {
    for node in nodes {
        match node {
            Node::Switch(children) => {
                for c in children {
                    match c {
                        // `Unknown` is a half-typed branch — allow it (no error)
                        // so completion survives mid-edit.
                        Node::Stack(_)
                        | Node::Switch(_)
                        | Node::Layout { .. }
                        | Node::Unknown(_) => {}
                        Node::Route { component, .. } => push_err(
                            err,
                            syn::Error::new(
                                component.span(),
                                "a `Switch` branch must be its own container; wrap this in \
                                 `Stack { … }` (a bare `Route` can't be a tab — it has no history)",
                            ),
                        ),
                        Node::Spread(expr) => push_err(
                            err,
                            syn::Error::new(
                                expr.span(),
                                "a `Switch` branch must be a `Stack` / `Switch`; `..spread` yields \
                                 routes — put the spread inside a branch's `Stack { … }`",
                            ),
                        ),
                    }
                }
                validate(children, err);
            }
            Node::Stack(children) => validate(children, err),
            Node::Layout { child, .. } => validate(std::slice::from_ref(child.as_ref()), err),
            Node::Route { .. } | Node::Spread(_) | Node::Unknown(_) => {}
        }
    }
}

/// One collected registry entry: a route id, its component, and its
/// optional `transition` expression.
struct RegEntry {
    id: String,
    component: Ident,
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
            Node::Switch(children) | Node::Stack(children) => collect(children, reg, spreads, err),
            Node::Layout { child, .. } => {
                collect(std::slice::from_ref(child.as_ref()), reg, spreads, err)
            }
            Node::Spread(expr) => spreads.push(expr.clone()),
            // A half-typed keyword contributes nothing to the registry; it must
            // NOT add an error, or `expand` would replace the whole expansion
            // (probe included) with the error and break completion.
            Node::Unknown(_) => {}
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
            let kids = children_vec_tokens(children, path, switch_n, layouts);
            quote! { ::whisker_router::core::RouteTree::stack(#kids) }
        }
        Node::Switch(children) => {
            let id = format!("switch_{}", *switch_n);
            *switch_n += 1;
            let kids = children_vec_tokens(children, path, switch_n, layouts);
            quote! {
                ::whisker_router::core::RouteTree::switch(
                    ::whisker_router::core::SwitchDef::new(#id, 0usize),
                    #kids,
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
        Node::Spread(_) => {
            // Spreads are spliced by `children_vec_tokens` as a *list* of
            // children; one can't stand alone as a single tree node. Reaching
            // here means a spread in a non-list position, e.g.
            // `Layout(X) { ..frag }`.
            syn::Error::new(
                Span::call_site(),
                "`..spread` must be a direct child of a `Stack` or `Switch`",
            )
            .to_compile_error()
        }
        Node::Unknown(kw) => {
            // A half-typed keyword. Emit a **completion probe**: a path into
            // `whisker_router::__kw` carrying the keyword's span, so
            // rust-analyzer completes `Stack` / `Switch` / `Route` / `Layout`
            // here (same mechanism `render!` uses for tag names). It has no
            // runtime effect; the dummy `RouteTree::route` keeps the expansion
            // well-typed so RA can descend into it. A real typo becomes an
            // "unresolved item in `__kw`" error pointing at the bad keyword.
            quote! {{
                #[allow(unused)]
                let _ = ::whisker_router::__kw::#kw;
                ::whisker_router::core::RouteTree::route("", "")
            }}
        }
    }
}

/// Emit a `Vec<RouteTree>` expression for a container's `children`, handling
/// `..spread` items.
///
/// With no spread it is a plain `::std::vec![ … ]`. With a spread it becomes a
/// block that pushes the literal children and `extend`s each spread fragment's
/// roots — so the splice happens at runtime. Literal children keep their
/// compile-time indices (used only for `Layout` paths); because a spread
/// shifts the *runtime* indices of later siblings, a positioned `Layout` must
/// not follow a spread in the same container (rare — spreads are trailing leaf
/// routes in practice).
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
