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
use quote::ToTokens;
use syn::{Expr, LitStr};

/// One node in the normalized tree.
pub(crate) enum IrNode {
    /// A tag with optional kwargs and optional children — covers
    /// render!'s `Element`/`UserComponent` (indistinguishable to the
    /// printer) and routes!'s `Switch`/`Stack`/`Route`/unknown-ident
    /// nodes.
    Tag(IrTag),
    /// A static render child string.
    Text(LitStr),
    /// A dynamic `{expr}` render child.
    Expression(Expr),
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
    /// nested-macro recursion). Boxed to keep the tree compact.
    Expr(Box<Expr>),
}

/// Adapt the shared builder-composition AST used by `compose!`, `render!`,
/// and `routes!` into the formatter's layout tree.
pub(crate) fn adapt_compose_input(
    input: &whisker_macro_syntax::compose::ComposeInput,
) -> Vec<IrNode> {
    input.nodes.iter().map(adapt_compose_node).collect()
}

pub(crate) fn adapt_compose_node(node: &whisker_macro_syntax::compose::ComposeNode) -> IrNode {
    use syn::spanned::Spanned;
    use whisker_macro_syntax::compose::ComposeChild;

    let tag = node
        .path
        .to_token_stream()
        .to_string()
        .replace(" :: ", "::");
    IrNode::Tag(IrTag {
        tag,
        tag_span: Some(node.path.span()),
        kwargs: node
            .arguments
            .iter()
            .map(|argument| IrKwarg {
                name: argument.name.to_string(),
                name_span: Some(argument.name.span()),
                value: (!argument.partial).then(|| IrValue::Expr(Box::new(argument.value.clone()))),
                value_span: (!argument.partial).then(|| argument.value.span()),
            })
            .collect(),
        children: node
            .body
            .iter()
            .map(|child| match child {
                ComposeChild::Node(node) => adapt_compose_node(node),
                ComposeChild::Text(value) => IrNode::Text(value.clone()),
                ComposeChild::Expression(value) => IrNode::Expression(value.clone()),
                ComposeChild::Spread(value) => IrNode::Spread(value.clone()),
            })
            .collect(),
        always_block: node.has_body,
    })
}

/// Walk an adapted tree collecting the span of every embedded `Expr`.
/// Walk an adapted tree collecting every embedded Rust expression.
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
        IrNode::Text(_) => {}
        IrNode::Expression(expr) => out.push(expr.span()),
        IrNode::Spread(expr) => out.push(expr.span()),
    }
}
