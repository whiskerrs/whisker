//! Width-aware pretty-printer for `render!` and `css!` bodies.
//!
//! ## Embedded Rust expressions
//!
//! Kwarg values, event-handler closures and any other embedded Rust
//! are rendered through [`Printer::expr_src`], which resolves in this
//! order:
//!
//! 1. A rustfmt-formatted entry in the [`ExprMap`] built by the full
//!    pipeline (see `expr_fmt.rs`). The text is stored dedented to
//!    column 0; the surrounding [`reindent`] calls push its continuation
//!    lines under the kwarg column.
//! 2. The verbatim source slice from [`SourceMap`] (see
//!    `source_map.rs`). This is the rustfmt-free path the unit tests
//!    use, and the fallback when rustfmt produced no entry for an expr.
//! 3. `proc_macro2` token printing when the span has no source position
//!    (a rare, best-effort path).
//!
//! ## Comments
//!
//! `syn` drops comments, so they are recovered from the body source as
//! [`GrammarComment`]s (see `comments.rs`) and reattached here. A cursor
//! ([`Printer::next`]) tracks the next unconsumed comment; [`flush`] emits
//! every pending comment whose `start` precedes a given byte bound, at the
//! right indent. Own-line comments go on their own line; trailing comments
//! are appended to the end of the preceding line.
//!
//! ## Layout
//!
//! A simple indent-and-wrap scheme (not full Wadler/Prettier): each
//! node renders inline if it fits within `max_width` at its current
//! indent, otherwise the kwargs and/or children break onto their own
//! indented lines. This matches the shallow, regular `render!` grammar
//! and is easy to keep idempotent.

use crate::comments::GrammarComment;
use crate::expr_fmt::ExprMap;
use crate::options::FmtOptions;
use crate::source_map::SourceMap;
use proc_macro2::Span;
use quote::ToTokens;
use std::cell::Cell;
use syn::{Expr, Ident, LitStr};
use whisker_macro_syntax::{
    CssInput, ElementNode, Kwarg, Node, Root, RoutesInput, RoutesNode, UserComponentNode,
};

/// Pretty-print a parsed `render!` body.
///
/// `base_indent` is the indent level (in tab-units) at which the macro
/// invocation sits in the rustfmt output; the body is indented one
/// level deeper.
///
/// `expr_map` supplies rustfmt-formatted text for the embedded
/// expressions, keyed by each expr's body-relative span. An EMPTY map
/// means "render every expr verbatim" — that is the rustfmt-free path
/// the [`crate::reformat_macros`] unit tests use.
///
/// `comments` are the grammar comments recovered from the body source,
/// reattached during printing. `body_len` is the byte length of the body
/// source (the top-level block's upper bound).
#[allow(clippy::too_many_arguments)]
pub(crate) fn print_render(
    root: &Root,
    map: &SourceMap,
    opts: &FmtOptions,
    base_indent: usize,
    expr_map: &ExprMap,
    comments: &[GrammarComment],
    body_len: usize,
) -> String {
    let p = Printer {
        map,
        opts,
        expr_map,
        comments,
        next: Cell::new(0),
    };
    let mut out = String::new();
    // Leading comments before the root node.
    if let Some(start) = p.node_start_byte(&root.node) {
        p.flush(start, base_indent + 1, &mut out);
    }
    p.node(&root.node, base_indent + 1, &mut out);
    // A trailing comment on the root node's own last line attaches inline.
    if let Some(start) = p.node_start_byte(&root.node) {
        let (_, after) = p.map.node_extent(start);
        if let Some(idx) = p.pending_trailing_on_line(after) {
            let before = p.comments[idx].start + 1;
            p.flush(before, base_indent + 1, &mut out);
        }
    }
    // Any remaining (own-line) comments after the root node, on their own
    // lines at the body indent.
    let idx = p.next.get();
    if idx < comments.len() {
        let mut tail = String::new();
        p.flush(body_len, base_indent + 1, &mut tail);
        if !tail.is_empty() {
            out.push('\n');
            out.push_str(tail.trim_end_matches('\n'));
        }
    }
    out
}

/// Pretty-print a parsed `css!` body.
pub(crate) fn print_css(
    input: &CssInput,
    map: &SourceMap,
    opts: &FmtOptions,
    base_indent: usize,
    expr_map: &ExprMap,
    comments: &[GrammarComment],
    body_len: usize,
) -> String {
    let p = Printer {
        map,
        opts,
        expr_map,
        comments,
        next: Cell::new(0),
    };
    p.css(input, base_indent + 1, body_len)
}

/// Pretty-print a parsed `routes!` body.
#[allow(clippy::too_many_arguments)]
pub(crate) fn print_routes(
    input: &RoutesInput,
    map: &SourceMap,
    opts: &FmtOptions,
    base_indent: usize,
    expr_map: &ExprMap,
    comments: &[GrammarComment],
    body_len: usize,
) -> String {
    let p = Printer {
        map,
        opts,
        expr_map,
        comments,
        next: Cell::new(0),
    };
    let mut out = String::new();
    let level = base_indent + 1;
    for (i, node) in input.roots.iter().enumerate() {
        if let Some(start) = routes_node_start_byte(&p, node) {
            p.flush(start, level, &mut out);
        }
        p.routes_node(node, level, &mut out);
        if let Some(end) = routes_node_end_byte(&p, node) {
            if let Some(idx) = p.pending_trailing_on_line(end) {
                let before = p.comments[idx].start + 1;
                p.flush(before, level, &mut out);
            }
        }
        if i + 1 < input.roots.len() || p.next.get() < comments.len() {
            out.push('\n');
        }
    }
    let idx = p.next.get();
    if idx < comments.len() {
        let mut tail = String::new();
        p.flush(body_len, level, &mut tail);
        if !tail.is_empty() {
            out.push_str(tail.trim_end_matches('\n'));
            out.push('\n');
        }
    }
    // Strip one trailing newline — the caller wraps with \n...\n.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

struct Printer<'a> {
    map: &'a SourceMap<'a>,
    opts: &'a FmtOptions,
    expr_map: &'a ExprMap,
    comments: &'a [GrammarComment],
    /// Index of the next unconsumed comment.
    next: Cell<usize>,
}

impl Printer<'_> {
    /// Render an embedded Rust expr. Resolution order:
    ///
    /// 1. A rustfmt-formatted entry in [`ExprMap`] (keyed by the expr's
    ///    body-relative span), stored dedented to column 0. Continuation
    ///    lines are re-indented to the kwarg column by the [`reindent`]
    ///    calls at the call sites, so nothing extra is needed here.
    /// 2. The verbatim source slice (rustfmt-free core / fallback).
    /// 3. `proc_macro2` token printing when the span has no source
    ///    position.
    fn expr_src(&self, span: Span, tokens: &dyn ToTokens) -> String {
        if let Some(formatted) = self.expr_map.get(span) {
            return formatted.to_string();
        }
        if let Some(s) = self.map.slice(span) {
            // Trim surrounding whitespace; internal formatting is kept.
            // For multi-line values, dedent continuation lines to column 0
            // (matching the [`ExprMap`] contract) so the surrounding
            // [`reindent`] call adds exactly the kwarg-column prefix and
            // re-formatting is a fixed point. Without this, slicing an
            // already-indented value and re-indenting it compounds the
            // indentation on every pass (non-idempotent).
            dedent_continuation(s.trim())
        } else {
            tokens.to_token_stream().to_string()
        }
    }

    fn indent(&self, level: usize) -> String {
        self.opts.indent_prefix(level)
    }

    // ---- comment reattachment ------------------------------------------

    /// Emit every not-yet-consumed comment whose `start < before`, at the
    /// given indent `level`.
    ///
    /// Own-line comments are written on their own line: `{indent}{text}\n`
    /// (possibly multi-line text is re-indented under `indent`). A
    /// non-own-line (trailing) comment is appended to the END of `out`
    /// (before the next `\n` the caller adds): ` {text}`.
    fn flush(&self, before: usize, level: usize, out: &mut String) {
        let indent = self.indent(level);
        let mut idx = self.next.get();
        while idx < self.comments.len() && self.comments[idx].start < before {
            let c = &self.comments[idx];
            if c.own_line {
                out.push_str(&indent);
                out.push_str(&reindent(&c.text, &indent));
                out.push('\n');
            } else {
                // Trailing: append to the end of the current output. Strip
                // a single trailing newline the caller may have already
                // pushed, append ` text`, then restore the newline.
                let had_nl = out.ends_with('\n');
                if had_nl {
                    out.pop();
                }
                out.push(' ');
                out.push_str(&c.text);
                if had_nl {
                    out.push('\n');
                }
            }
            idx += 1;
        }
        self.next.set(idx);
    }

    /// `true` if there is a pending trailing comment whose `start` falls
    /// on the same source line as byte `line_end` (the end of the just-
    /// emitted node). Used to attach trailing comments to a child.
    fn pending_trailing_on_line(&self, line_end: usize) -> Option<usize> {
        let idx = self.next.get();
        let c = self.comments.get(idx)?;
        if c.own_line {
            return None;
        }
        // Same source line as `line_end` if no '\n' lies between them.
        let (lo, hi) = if c.start < line_end {
            (c.start, line_end)
        } else {
            (line_end, c.start)
        };
        let between = self.map.between_has_newline(lo, hi);
        if between { None } else { Some(idx) }
    }

    /// First source byte of a node (its tag / alias ident).
    fn node_start_byte(&self, node: &Node) -> Option<usize> {
        let span = match node {
            Node::Element(el) => el.tag.span(),
            Node::UserComponent(uc) => uc.alias_ident.span(),
            Node::ChildrenSlot { span } => *span,
        };
        self.map.byte_range(span).map(|(s, _)| s)
    }

    // ---- render! -------------------------------------------------------

    fn node(&self, node: &Node, level: usize, out: &mut String) {
        match node {
            Node::Element(el) => self.element(el, level, out),
            Node::UserComponent(uc) => self.user_component(uc, level, out),
            Node::ChildrenSlot { .. } => {
                out.push_str(&self.indent(level));
                out.push_str("children()");
            }
        }
    }

    fn element(&self, el: &ElementNode, level: usize, out: &mut String) {
        let start = self.map.byte_range(el.tag.span()).map(|(s, _)| s);
        self.tag_node(
            &el.tag.to_string(),
            &el.kwargs,
            &el.children,
            level,
            start,
            out,
        );
    }

    fn user_component(&self, uc: &UserComponentNode, level: usize, out: &mut String) {
        let start = self.map.byte_range(uc.alias_ident.span()).map(|(s, _)| s);
        self.tag_node(
            &uc.alias_ident.to_string(),
            &uc.kwargs,
            &uc.children,
            level,
            start,
            out,
        );
    }

    /// The shared `tag(kwargs) { children }` rendering used by both
    /// element and user-component nodes. `node_start` is the byte offset
    /// of this node's tag in the body source (used to locate its block
    /// via [`SourceMap::node_extent`] so comments land at the right
    /// indent / side of the closing `}`).
    fn tag_node(
        &self,
        tag: &str,
        kwargs: &[Kwarg],
        children: &[Node],
        level: usize,
        node_start: Option<usize>,
        out: &mut String,
    ) {
        let indent = self.indent(level);
        out.push_str(&indent);
        out.push_str(tag);

        // ---- kwargs ----
        if !kwargs.is_empty() {
            let parts: Vec<String> = kwargs.iter().map(|kw| self.kwarg(kw)).collect();
            let inline = format!("({})", parts.join(", "));
            let inline_width =
                self.opts.indent_width(level) + tag.len() + inline.len() + brace_width(children);
            let single_line = !inline.contains('\n');
            if single_line && inline_width <= self.opts.max_width {
                out.push_str(&inline);
            } else {
                // Break each kwarg onto its own line.
                out.push_str("(\n");
                let inner = self.indent(level + 1);
                for (i, part) in parts.iter().enumerate() {
                    out.push_str(&inner);
                    // Re-indent any internal newlines of a multi-line
                    // kwarg value so it stays under the kwarg's column.
                    out.push_str(&reindent(part, &inner));
                    if i + 1 < parts.len() {
                        out.push(',');
                    } else {
                        // trailing comma on the last kwarg
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&indent);
                out.push(')');
            }
        }

        // ---- children ----
        // Resolve this node's block byte bounds so comments are placed
        // relative to its `{ … }`.
        let inner_close = node_start.and_then(|s| self.map.node_extent(s).0);

        // If there are pending comments destined for this node's block
        // (i.e. starting before its closing brace) we must render the
        // multi-line block form even when `children` is empty, so those
        // comments have somewhere to go.
        let has_block_comments = inner_close
            .map(|close| {
                let idx = self.next.get();
                self.comments
                    .get(idx)
                    .map(|c| c.start < close)
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if !children.is_empty() || has_block_comments {
            out.push_str(" {\n");
            let child_level = level + 1;
            for child in children {
                let child_start = self.node_start_byte(child);
                // Leading own-line comments before this child.
                if let Some(cs) = child_start {
                    self.flush(cs, child_level, out);
                }
                self.node(child, child_level, out);
                // Trailing same-line comment on the child.
                if let Some((_, child_end)) = child_extent(self, child) {
                    if let Some(idx) = self.pending_trailing_on_line(child_end) {
                        // Append trailing comment to the end of this line.
                        let before = self.comments[idx].start + 1;
                        self.flush(before, child_level, out);
                    }
                }
                out.push('\n');
            }
            // Comments sitting before the closing `}`.
            if let Some(close) = inner_close {
                self.flush(close, child_level, out);
            }
            out.push_str(&indent);
            out.push('}');
        }
    }

    fn kwarg(&self, kw: &Kwarg) -> String {
        let name = kw.name.to_string();
        if kw.partial {
            // Partial kwarg: just the name (mid-typing). Preserve the
            // author's `name` with no value.
            return name;
        }
        let value = self.expr_src(span_of(&kw.value), &kw.value);
        format!("{name}: {value}")
    }

    // ---- css! ----------------------------------------------------------

    fn css(&self, input: &CssInput, level: usize, body_len: usize) -> String {
        if input.kwargs.is_empty() {
            return String::new();
        }

        let has_comments = !self.comments.is_empty();

        // Inline form only when there are NO comments to place (comments
        // imply line breaks).
        if !has_comments {
            let parts: Vec<String> = input.kwargs.iter().map(|kw| self.css_kwarg(kw)).collect();
            let inline = parts.join(", ");
            let inline_width = self.opts.indent_width(level) + inline.len();
            if !inline.contains('\n') && inline_width <= self.opts.max_width {
                let indent = self.indent(level);
                return format!("{indent}{inline}");
            }
            // Otherwise one kwarg per line, trailing comma.
            let indent = self.indent(level);
            let mut out = String::new();
            for part in &parts {
                out.push_str(&indent);
                out.push_str(&reindent(part, &indent));
                out.push_str(",\n");
            }
            out.pop();
            return out;
        }

        // Comment-bearing css! body: one field per line, flushing
        // comments before each field and after the last.
        let indent = self.indent(level);
        let mut out = String::new();
        for kw in &input.kwargs {
            let start = self.map.byte_range(kw.name.span()).map(|(s, _)| s);
            if let Some(s) = start {
                self.flush(s, level, &mut out);
            }
            out.push_str(&indent);
            out.push_str(&reindent(&self.css_kwarg(kw), &indent));
            out.push(',');
            // Trailing same-line comment after this field.
            let field_end = self
                .map
                .byte_range(kw.name.span())
                .map(|(_, e)| e)
                .unwrap_or(0);
            // Use the value's end if present for a tighter line bound.
            let line_end = kw
                .value
                .as_ref()
                .and_then(|e| self.map.byte_range(span_of(e)))
                .map(|(_, e)| e)
                .unwrap_or(field_end);
            if let Some(idx) = self.pending_trailing_on_line(line_end) {
                let before = self.comments[idx].start + 1;
                self.flush(before, level, &mut out);
            }
            out.push('\n');
        }
        // Any comments after the last field, before the body end.
        self.flush(body_len, level, &mut out);
        // strip trailing newline (caller adds delimiters)
        while out.ends_with('\n') {
            out.pop();
        }
        out
    }

    fn css_kwarg(&self, kw: &whisker_macro_syntax::CssKwarg) -> String {
        let name = kw.name.to_string();
        match &kw.value {
            Some(expr) => {
                let v = self.expr_src(span_of(expr), expr);
                format!("{name}: {v}")
            }
            None => name,
        }
    }

    // ---- routes! ----------------------------------------------------------

    fn routes_node(&self, node: &RoutesNode, level: usize, out: &mut String) {
        match node {
            RoutesNode::Switch { kw, children } => {
                self.routes_container(&kw.to_string(), children, level, out);
            }
            RoutesNode::Stack { kw, children } => {
                self.routes_container(&kw.to_string(), children, level, out);
            }
            RoutesNode::Route {
                kw,
                path,
                component,
                transition,
                children,
            } => {
                self.routes_route(
                    &kw.to_string(),
                    path.as_ref(),
                    component.as_ref(),
                    transition.as_ref(),
                    children,
                    level,
                    out,
                );
            }
            RoutesNode::Spread(expr) => {
                let indent = self.indent(level);
                let src = self.expr_src(span_of(expr), expr);
                out.push_str(&indent);
                out.push_str("..");
                out.push_str(&src);
            }
            RoutesNode::Unknown(ident) => {
                let indent = self.indent(level);
                out.push_str(&indent);
                out.push_str(&ident.to_string());
            }
        }
    }

    fn routes_container(
        &self,
        keyword: &str,
        children: &[RoutesNode],
        level: usize,
        out: &mut String,
    ) {
        let indent = self.indent(level);
        out.push_str(&indent);
        out.push_str(keyword);
        out.push_str(" {\n");
        let child_level = level + 1;
        for child in children {
            if let Some(start) = routes_node_start_byte(self, child) {
                self.flush(start, child_level, out);
            }
            self.routes_node(child, child_level, out);
            if let Some(end) = routes_node_end_byte(self, child) {
                if let Some(idx) = self.pending_trailing_on_line(end) {
                    let before = self.comments[idx].start + 1;
                    self.flush(before, child_level, out);
                }
            }
            out.push('\n');
        }
        out.push_str(&indent);
        out.push('}');
    }

    #[allow(clippy::too_many_arguments)]
    fn routes_route(
        &self,
        keyword: &str,
        path: Option<&LitStr>,
        component: Option<&Ident>,
        transition: Option<&Expr>,
        children: &[RoutesNode],
        level: usize,
        out: &mut String,
    ) {
        let indent = self.indent(level);
        out.push_str(&indent);
        out.push_str(keyword);

        // Build kwargs
        let mut kwargs: Vec<String> = Vec::new();
        if let Some(p) = path {
            kwargs.push(format!("path: {:?}", p.value()));
        }
        if let Some(c) = component {
            kwargs.push(format!("component: {c}"));
        }
        if let Some(t) = transition {
            let src = self.expr_src(span_of(t), t);
            kwargs.push(format!(
                "transition: {}",
                reindent(&src, &self.indent(level + 1))
            ));
        }

        if !kwargs.is_empty() {
            let inline = format!("({})", kwargs.join(", "));
            let inline_width = self.opts.indent_width(level)
                + keyword.len()
                + inline.len()
                + if children.is_empty() { 0 } else { " {".len() };
            if !inline.contains('\n') && inline_width <= self.opts.max_width {
                out.push_str(&inline);
            } else {
                out.push_str("(\n");
                let inner = self.indent(level + 1);
                for (i, kw) in kwargs.iter().enumerate() {
                    out.push_str(&inner);
                    out.push_str(&reindent(kw, &inner));
                    out.push(',');
                    if i + 1 < kwargs.len() {
                        out.push('\n');
                    }
                }
                out.push('\n');
                out.push_str(&indent);
                out.push(')');
            }
        }

        if !children.is_empty() {
            out.push_str(" {\n");
            let child_level = level + 1;
            for child in children {
                if let Some(start) = routes_node_start_byte(self, child) {
                    self.flush(start, child_level, out);
                }
                self.routes_node(child, child_level, out);
                if let Some(end) = routes_node_end_byte(self, child) {
                    if let Some(idx) = self.pending_trailing_on_line(end) {
                        let before = self.comments[idx].start + 1;
                        self.flush(before, child_level, out);
                    }
                }
                out.push('\n');
            }
            out.push_str(&indent);
            out.push('}');
        }
    }
}

/// First source byte of a routes node (its keyword ident / spread expr).
fn routes_node_start_byte(p: &Printer, node: &RoutesNode) -> Option<usize> {
    let span = node.kw_span()?;
    p.map.byte_range(span).map(|(s, _)| s)
}

/// End byte of a routes node (past its closing brace or last token).
fn routes_node_end_byte(p: &Printer, node: &RoutesNode) -> Option<usize> {
    let span = node.kw_span()?;
    let start = p.map.byte_range(span).map(|(s, _)| s)?;
    let (_, end) = p.map.node_extent(start);
    Some(end)
}

/// Byte extent `(start, end)` of a child node via its tag span +
/// [`SourceMap::node_extent`]. `end` is the byte just past the node.
fn child_extent(p: &Printer, child: &Node) -> Option<(usize, usize)> {
    let start = p.node_start_byte(child)?;
    let (_, after) = p.map.node_extent(start);
    Some((start, after))
}

/// Width contribution of a ` { … }` children block when deciding
/// whether a node's kwargs fit inline. A non-empty children block
/// always forces a multi-line body, so we only need the ` {` opener's
/// width to be honest about the first line; the closing brace and the
/// children sit on later lines.
fn brace_width(children: &[Node]) -> usize {
    if children.is_empty() { 0 } else { " {".len() }
}

/// Dedent the continuation (2nd..=Nth) lines of a multi-line fragment by
/// their common leading-whitespace amount, so they sit at column 0
/// relative to the first line. The first line is already at column 0 (the
/// caller trimmed it). This makes the later [`reindent`] idempotent: a
/// value re-sliced from already-formatted output has its kwarg-column
/// indentation stripped back off before being re-indented.
///
/// Lines that are entirely whitespace are ignored when computing the
/// common indent (and emitted empty) so they don't force the common
/// dedent to zero.
fn dedent_continuation(fragment: &str) -> String {
    if !fragment.contains('\n') {
        return fragment.to_string();
    }
    let mut lines = fragment.split('\n');
    let first = lines.next().unwrap_or("");
    let rest: Vec<&str> = lines.collect();
    // Common leading-whitespace width across non-blank continuation lines.
    let common = rest
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out = String::from(first);
    for line in rest {
        out.push('\n');
        if line.trim().is_empty() {
            // keep blank lines blank
        } else {
            out.push_str(&line[common..]);
        }
    }
    out
}

/// Re-indent the 2nd..=Nth lines of a (possibly multi-line) fragment so
/// continuation lines sit under `prefix`. The first line is left as-is
/// (the caller already emitted `prefix` before it).
fn reindent(fragment: &str, prefix: &str) -> String {
    if !fragment.contains('\n') {
        return fragment.to_string();
    }
    let mut lines = fragment.lines();
    let mut out = String::new();
    if let Some(first) = lines.next() {
        out.push_str(first);
    }
    for line in lines {
        out.push('\n');
        out.push_str(prefix);
        out.push_str(line);
    }
    out
}

/// The `Span` covering an expression (start of first token to end of
/// last), via `syn::spanned::Spanned`.
fn span_of(expr: &syn::Expr) -> Span {
    use syn::spanned::Spanned;
    expr.span()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedent_single_line_unchanged() {
        assert_eq!(dedent_continuation("foo"), "foo");
    }

    #[test]
    fn dedent_strips_common_continuation_indent() {
        let v = "css!(\n            a: 1,\n            b: 2,\n        )";
        // First line at col 0; continuation lines share 8-space common
        // indent (the `        )` closing line) — all stripped by 8.
        let out = dedent_continuation(v);
        assert_eq!(out, "css!(\n    a: 1,\n    b: 2,\n)");
    }

    #[test]
    fn dedent_is_idempotent() {
        let v = "css!(\n            a: 1,\n        )";
        let once = dedent_continuation(v);
        let twice = dedent_continuation(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn dedent_keeps_blank_lines_blank() {
        let v = "a\n        b\n\n        c";
        let out = dedent_continuation(v);
        assert_eq!(out, "a\nb\n\nc");
    }
}
