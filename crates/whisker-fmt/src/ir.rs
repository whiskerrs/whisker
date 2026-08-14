//! Internal, format-only normalization of `render!`'s and `routes!`'s
//! parse trees ([`whisker_macro_syntax::render`] /
//! [`whisker_macro_syntax::routes`]) into ONE shared tree shape the
//! printer walks with a single recursive function.
//!
//! This is deliberately a `whisker-fmt`-internal type, NOT something
//! added to `whisker-macro-syntax`: that crate's `render` and `css`
//! parse types are consumed directly by the real codegen in
//! `whisker-macros`, so changing their shape would change what real apps
//! compile to.
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
    /// printer) and routes!'s `Switch`/`Stack`/`Route`/unknown-ident
    /// nodes.
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
    /// `UserComponentNode.alias_ident`, post `snake_to_pascal`); this
    /// module classifies nothing of its own. For routes! it is the
    /// literal `Switch`/`Stack`/`Route`/unknown-ident keyword.
    pub tag: String,
    /// Span of the source ident this tag was built from, used to locate
    /// the node's `(kwargs)? {children}?` extent for comment placement.
    /// Safe even for a `UserComponent`'s derived `alias_ident`: `Ident::new`
    /// swaps the text but keeps the original source span.
    pub tag_span: Option<Span>,
    pub kwargs: Vec<IrKwarg>,
    pub children: Vec<IrNode>,
    /// `true` only for routes!'s `Switch`/`Stack`, whose `{ … }` is
    /// mandatory in the grammar even when empty. Every other tag omits
    /// an empty, comment-free block.
    pub always_block: bool,
}

pub(crate) struct IrKwarg {
    pub name: String,
    /// Source span of the kwarg's name (routes!'s synthesized kwargs use
    /// the value token's span), locating the kwarg for comment placement.
    pub name_span: Option<Span>,
    /// `None` = partial (mid-typing: no `:` yet, or the value failed to
    /// parse) — printed as the bare name, no value.
    pub value: Option<IrValue>,
    /// Source span of the value, bounding the kwarg's line for trailing-
    /// comment attachment.
    pub value_span: Option<Span>,
}

pub(crate) enum IrValue {
    /// A real Rust expression — printed through
    /// [`crate::printer::Printer::expr_src`] (ExprMap / verbatim /
    /// nested-macro recursion). Boxed: `Expr` is far larger than
    /// [`IrValue::Literal`]'s `String`, and this enum lives inside every
    /// [`IrKwarg`] in a tree.
    Expr(Box<Expr>),
    /// Pre-rendered text, printed as-is with no `expr_src` machinery.
    /// Used for routes!'s `Route(path: "…", component: Foo)`, whose
    /// `path`/`component` are a `LitStr`/`Ident` rather than full exprs:
    /// a debug-quoted string and a bare ident respectively.
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
    use syn::spanned::Spanned;
    kwargs
        .iter()
        .map(|kw| IrKwarg {
            name: kw.name.to_string(),
            name_span: Some(kw.name.span()),
            value: if kw.partial {
                None
            } else {
                Some(IrValue::Expr(Box::new(kw.value.clone())))
            },
            value_span: (!kw.partial).then(|| kw.value.span()),
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
            use syn::spanned::Spanned;
            let mut kwargs = Vec::new();
            if let Some(p) = path {
                kwargs.push(IrKwarg {
                    name: "path".to_string(),
                    name_span: Some(p.span()),
                    value: Some(IrValue::Literal(format!("{:?}", p.value()))),
                    value_span: Some(p.span()),
                });
            }
            if let Some(c) = component {
                kwargs.push(IrKwarg {
                    name: "component".to_string(),
                    name_span: Some(c.span()),
                    value: Some(IrValue::Literal(c.to_string())),
                    value_span: Some(c.span()),
                });
            }
            if let Some(t) = transition {
                kwargs.push(IrKwarg {
                    name: "transition".to_string(),
                    name_span: Some(t.span()),
                    value: Some(IrValue::Expr(Box::new(t.clone()))),
                    value_span: Some(t.span()),
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

/// Walk an adapted tree collecting the span of every embedded `Expr`.
/// `IrValue::Literal` values (routes!'s `path`/`component`) contribute
/// no span: they are neither batch-rustfmt'd nor excluded from comment
/// recovery.
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
