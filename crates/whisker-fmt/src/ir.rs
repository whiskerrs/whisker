//! Internal, format-only normalization of `render!`'s and `routes!`'s
//! parse trees ([`whisker_macro_syntax::render`] /
//! [`whisker_macro_syntax::routes`]) into ONE shared tree shape the
//! printer walks with a single recursive function.
//!
//! This is deliberately a `whisker-fmt`-internal type, NOT something
//! added to `whisker-macro-syntax`. `whisker-macro-syntax::render`'s
//! `Node`/`ElementNode`/`UserComponentNode`/`Kwarg` and
//! `whisker-macro-syntax::css`'s `CssInput`/`CssKwarg` are consumed
//! directly by the real `render!`/`css!` codegen in `whisker-macros` (see
//! that crate's doc comment) — changing their shape would change what
//! real apps compile to. `routes!`'s mirror type has no such consumer
//! (the real `routes!` proc-macro in `whisker-router-macros` has its own,
//! separate parser), but is adapted here too for consistency and because
//! its printer is what this refactor is actually simplifying.
//!
//! `css!`'s body doesn't fit this shape at all (it's a flat kwarg list
//! with no tag and no children — see `Printer::css`), so it is NOT
//! adapted here and keeps its own small printer.

use proc_macro2::Span;
use syn::Expr;

/// One node in the normalized tree.
pub(crate) enum IrNode {
    /// A tag with optional kwargs and optional children — covers
    /// render!'s `Element`/`UserComponent` (indistinguishable to the
    /// printer, which never treated them differently) and routes!'s
    /// `Switch`/`Stack`/`Route`/unrecognized-ident nodes.
    Tag(IrTag),
    /// render!'s `children()` slot — always prints literally as
    /// `children()`, never subject to the "omit `()` with no kwargs"
    /// rule other tags get.
    ChildrenSlot(Span),
    /// routes!'s `..expr` spread — doesn't fit the tag shape at all.
    Spread(Expr),
}

pub(crate) struct IrTag {
    /// Text to print for this tag. For render! this is ALREADY the
    /// classified/derived name (`ElementNode.tag` or
    /// `UserComponentNode.alias_ident`, post `snake_to_pascal`) — this
    /// module does no classification of its own, it just carries
    /// whatever `whisker-macro-syntax::render` already decided. For
    /// routes! this is the literal `Switch`/`Stack`/`Route`/unknown-ident
    /// keyword.
    pub tag: String,
    /// Span of the source ident this tag was built from, used to locate
    /// the node's `(kwargs)? {children}?` extent (for comment
    /// placement). Reused even for a render! `UserComponent`'s derived
    /// `alias_ident` — that ident's span still points at the ORIGINAL
    /// tag text in source (`Ident::new` there just swaps the text, not
    /// the span).
    pub tag_span: Option<Span>,
    pub kwargs: Vec<IrKwarg>,
    pub children: Vec<IrNode>,
    /// `true` only for routes!'s `Switch`/`Stack`, whose `{ … }` is
    /// mandatory in the grammar even when empty (`braced!` requires the
    /// braces to be present at parse time) — never omit the block for
    /// these, even with zero children. Every other tag omits an empty,
    /// comment-free block, per the shared "optional block" rule.
    pub always_block: bool,
}

pub(crate) struct IrKwarg {
    pub name: String,
    /// `None` = partial (mid-typing: no `:` yet, or the value failed to
    /// parse) — printed as the bare name, no value.
    pub value: Option<IrValue>,
}

pub(crate) enum IrValue {
    /// A real Rust expression — printed through
    /// [`crate::printer::Printer::expr_src`] (ExprMap / verbatim /
    /// nested-macro recursion). Boxed: `Expr` is far larger than
    /// [`IrValue::Literal`]'s `String`, and this enum lives inside every
    /// [`IrKwarg`] in a tree.
    Expr(Box<Expr>),
    /// Pre-rendered text, printed as-is with no `expr_src` machinery —
    /// used for routes!'s `Route(path: "…", component: Foo)`, whose
    /// `path`/`component` aren't full exprs in
    /// `whisker-macro-syntax::routes` (`LitStr`/`Ident` respectively) and
    /// are reconstructed the same way `Printer::routes_route` always
    /// has: a debug-quoted string for `path`, a bare ident for
    /// `component`.
    Literal(String),
}

// ---- adapters -------------------------------------------------------------

/// Adapt a `render!` body's single root into an [`IrNode`].
pub(crate) fn adapt_render_root(root: &whisker_macro_syntax::render::Root) -> IrNode {
    adapt_render_node(&root.node)
}

fn adapt_render_node(node: &whisker_macro_syntax::render::Node) -> IrNode {
    use whisker_macro_syntax::render::Node;
    match node {
        Node::Element(el) => IrNode::Tag(IrTag {
            tag: el.tag.to_string(),
            tag_span: Some(el.tag.span()),
            kwargs: adapt_render_kwargs(&el.kwargs),
            children: el.children.iter().map(adapt_render_node).collect(),
            always_block: false,
        }),
        Node::UserComponent(uc) => IrNode::Tag(IrTag {
            tag: uc.alias_ident.to_string(),
            tag_span: Some(uc.alias_ident.span()),
            kwargs: adapt_render_kwargs(&uc.kwargs),
            children: uc.children.iter().map(adapt_render_node).collect(),
            always_block: false,
        }),
        Node::ChildrenSlot { span } => IrNode::ChildrenSlot(*span),
    }
}

fn adapt_render_kwargs(kwargs: &[whisker_macro_syntax::render::Kwarg]) -> Vec<IrKwarg> {
    kwargs
        .iter()
        .map(|kw| IrKwarg {
            name: kw.name.to_string(),
            value: if kw.partial {
                None
            } else {
                Some(IrValue::Expr(Box::new(kw.value.clone())))
            },
        })
        .collect()
}

/// Adapt a `routes!` body's root list into a sequence of [`IrNode`]s.
pub(crate) fn adapt_routes_roots(input: &whisker_macro_syntax::routes::RoutesInput) -> Vec<IrNode> {
    input.roots.iter().map(adapt_routes_node).collect()
}

fn adapt_routes_node(node: &whisker_macro_syntax::routes::RoutesNode) -> IrNode {
    use whisker_macro_syntax::routes::RoutesNode;
    match node {
        RoutesNode::Switch { kw, children } => IrNode::Tag(IrTag {
            tag: kw.to_string(),
            tag_span: Some(kw.span()),
            kwargs: Vec::new(),
            children: children.iter().map(adapt_routes_node).collect(),
            always_block: true,
        }),
        RoutesNode::Stack { kw, children } => IrNode::Tag(IrTag {
            tag: kw.to_string(),
            tag_span: Some(kw.span()),
            kwargs: Vec::new(),
            children: children.iter().map(adapt_routes_node).collect(),
            always_block: true,
        }),
        RoutesNode::Route {
            kw,
            path,
            component,
            transition,
            children,
        } => {
            let mut kwargs = Vec::new();
            if let Some(p) = path {
                kwargs.push(IrKwarg {
                    name: "path".to_string(),
                    value: Some(IrValue::Literal(format!("{:?}", p.value()))),
                });
            }
            if let Some(c) = component {
                kwargs.push(IrKwarg {
                    name: "component".to_string(),
                    value: Some(IrValue::Literal(c.to_string())),
                });
            }
            if let Some(t) = transition {
                kwargs.push(IrKwarg {
                    name: "transition".to_string(),
                    value: Some(IrValue::Expr(Box::new(t.clone()))),
                });
            }
            IrNode::Tag(IrTag {
                tag: kw.to_string(),
                tag_span: Some(kw.span()),
                kwargs,
                children: children.iter().map(adapt_routes_node).collect(),
                always_block: false,
            })
        }
        RoutesNode::Spread(expr) => IrNode::Spread(expr.clone()),
        RoutesNode::Unknown(ident) => IrNode::Tag(IrTag {
            tag: ident.to_string(),
            tag_span: Some(ident.span()),
            kwargs: Vec::new(),
            children: Vec::new(),
            always_block: false,
        }),
    }
}

/// Walk an adapted tree collecting the span of every embedded `Expr` —
/// mirrors what `collect_render_expr_spans` / `collect_routes_expr_spans`
/// used to do separately, now over the shared shape. `IrValue::Literal`
/// values (routes!'s `path`/`component`) contribute no span — they were
/// never batch-rustfmt'd or excluded from comment recovery, matching
/// today's behavior.
pub(crate) fn collect_ir_expr_spans(node: &IrNode, out: &mut Vec<Span>) {
    use syn::spanned::Spanned;
    match node {
        IrNode::Tag(tag) => {
            for kw in &tag.kwargs {
                if let Some(IrValue::Expr(e)) = &kw.value {
                    out.push(e.span());
                }
            }
            for child in &tag.children {
                collect_ir_expr_spans(child, out);
            }
        }
        IrNode::ChildrenSlot(_) => {}
        IrNode::Spread(expr) => out.push(expr.span()),
    }
}
