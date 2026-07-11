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
use crate::ir::{IrKwarg, IrNode, IrTag, IrValue};
use crate::options::FmtOptions;
use crate::source_map::SourceMap;
use proc_macro2::Span;
use quote::ToTokens;
use std::cell::Cell;
use syn::Expr;
use whisker_macro_syntax::CssInput;

/// Pretty-print an adapted `render!` root ([`crate::ir::adapt_render_root`]).
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
    root: &IrNode,
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
    if let Some(start) = p.ir_node_start_byte(root) {
        p.flush(start, base_indent + 1, &mut out);
    }
    p.ir_node(root, base_indent + 1, &mut out);
    // A trailing comment on the root node's own last line attaches inline.
    if let Some((_, after)) = p.ir_node_extent(root) {
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

/// Pretty-print an adapted `routes!` root list
/// ([`crate::ir::adapt_routes_roots`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn print_routes(
    roots: &[IrNode],
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
    for (i, node) in roots.iter().enumerate() {
        if let Some(start) = p.ir_node_start_byte(node) {
            p.flush(start, level, &mut out);
        }
        p.ir_node(node, level, &mut out);
        if let Some((_, end)) = p.ir_node_extent(node) {
            if let Some(idx) = p.pending_trailing_on_line(end) {
                let before = p.comments[idx].start + 1;
                p.flush(before, level, &mut out);
            }
        }
        if i + 1 < roots.len() || p.next.get() < comments.len() {
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
    /// Render an embedded Rust expr. `level` is the indent level of the
    /// line the expr's value sits on — its ONLY use is as the width
    /// reference for a nested `css!`/`routes!` macro call's own
    /// inline-vs-wrap decision (see [`Printer::nested_macro_src`]); it
    /// does not affect the returned fragment's own indentation (that is
    /// always column-0-anchored, per the [`ExprMap`] contract below).
    ///
    /// Resolution order:
    ///
    /// 1. A nested `css!( … )` / `routes!{ … }` macro call: recursively
    ///    printed with the grammar-aware printer instead of treated as an
    ///    opaque expr.
    /// 2. A rustfmt-formatted entry in [`ExprMap`] (keyed by the expr's
    ///    body-relative span), stored dedented to column 0. Continuation
    ///    lines are re-indented to the kwarg column by the [`reindent`]
    ///    calls at the call sites, so nothing extra is needed here.
    /// 3. The verbatim source slice (rustfmt-free core / fallback).
    /// 4. `proc_macro2` token printing when the span has no source
    ///    position.
    fn expr_src(&self, span: Span, expr: &Expr, level: usize) -> String {
        if let Some(nested) = self.nested_macro_src(expr, level) {
            return nested;
        }
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
            expr.to_token_stream().to_string()
        }
    }

    /// If `expr` is a `css!( … )` / `render!{ … }` / `routes!{ … }` macro
    /// call, recursively format its body with the grammar-aware printer
    /// instead of treating it as an opaque expression — this is what
    /// makes `render! { view(style: css!(…)) }` reformat the nested
    /// `css!`/`routes!` call instead of passing it through verbatim.
    ///
    /// `level` is the caller's best estimate of the indent level the
    /// nested macro's own line will actually sit at once the OUTER node's
    /// inline-vs-wrap decision has been made — callers pass their own
    /// `level` (+1 when the value would land one level deeper if the
    /// surrounding kwargs end up wrapped, which is the common case for
    /// anything wide enough to need this decision at all). It is used
    /// ONLY as the width reference for the nested call's own
    /// inline-vs-wrap fit check ([`Printer::delimited_list`]) so a
    /// deeply-nested `css!`/`routes!` doesn't wrongly collapse onto one,
    /// far-too-long line by measuring itself against a shallow assumed
    /// depth. This is a best-effort estimate, not an exact final column
    /// (the true depth isn't known until the OUTER node's own wrap
    /// decision, which depends circularly on this one) — the same
    /// approximation [`crate::expr_fmt`] already accepts for
    /// rustfmt-formatted embedded exprs.
    ///
    /// Returns `None` (falling back to the normal [`ExprMap`] / verbatim
    /// path in [`Printer::expr_src`]) when `expr` isn't a
    /// `css!`/`render!`/`routes!` call, its body doesn't parse as that
    /// grammar, is empty, or — the comment fail-safe — its source
    /// contains `//` or `/*` anywhere. Comments inside a nested macro
    /// aren't threaded through this recursive call (the grammar-comment
    /// recovery pass that collects them runs once, over the OUTER body,
    /// before this nested printer exists), so rather than risk dropping
    /// one we leave the whole nested call untouched, matching the
    /// fail-safe used everywhere else in this crate.
    fn nested_macro_src(&self, expr: &Expr, level: usize) -> Option<String> {
        let Expr::Macro(em) = expr else {
            return None;
        };
        let name = em.mac.path.get_ident()?.to_string();
        if name != "css" && name != "render" && name != "routes" {
            return None;
        }
        // The delimiter token's own span covers everything BETWEEN AND
        // INCLUDING the open/close delimiters — unlike `mac.tokens`'s
        // (joined-from-real-tokens) span, which excludes any comment
        // sitting in the gap right after `(`/`{` or right before `)`/`}`.
        // We need the delimiter span, not the tokens' span, so the
        // comment fail-safe below can't miss a leading/trailing comment.
        let (open, close, delim_span) = match &em.mac.delimiter {
            syn::MacroDelimiter::Paren(p) => ('(', ')', p.span),
            syn::MacroDelimiter::Brace(b) => ('{', '}', b.span),
            syn::MacroDelimiter::Bracket(bk) => ('[', ']', bk.span),
        };
        let full_src = self.map.slice(delim_span.join())?;
        if full_src.contains("//") || full_src.contains("/*") {
            return None;
        }
        // Rust/rustfmt convention: a space before a brace-delimited
        // macro's `{` (`render!`/`routes! { … }`), none before `(`/`[`
        // (`css!(…)`) — matches how the base rustfmt pass spaces the
        // user's own top-level invocations.
        let bang = if open == '{' { "! " } else { "!" };
        match name.as_str() {
            "css" => {
                let input = whisker_macro_syntax::css::parse_input(em.mac.tokens.clone()).ok()?;
                if input.kwargs.is_empty() {
                    return None;
                }
                let parts: Vec<String> = input
                    .kwargs
                    .iter()
                    .map(|kw| self.css_kwarg(kw, level))
                    .collect();
                // `output_level = 0`: a relative, column-0-anchored
                // fragment — see `delimited_list`'s doc.
                let list =
                    self.delimited_list(level, 0, name.len() + bang.len(), &parts, open, close, 0);
                Some(format!("{name}{bang}{list}"))
            }
            "render" => {
                let root = whisker_macro_syntax::render::parse_root(em.mac.tokens.clone()).ok()?;
                let ir_root = crate::ir::adapt_render_root(&root);
                let body = print_render(&ir_root, self.map, self.opts, 0, self.expr_map, &[], 0);
                Some(nested_wrap(&name, bang, open, close, &body))
            }
            "routes" => {
                let input =
                    whisker_macro_syntax::routes::parse_input(em.mac.tokens.clone()).ok()?;
                if input.roots.is_empty() {
                    return None;
                }
                let roots = crate::ir::adapt_routes_roots(&input);
                let body = print_routes(&roots, self.map, self.opts, 0, self.expr_map, &[], 0);
                Some(nested_wrap(&name, bang, open, close, &body))
            }
            _ => None,
        }
    }

    /// Break `parts` one per line at `level`, each with a trailing
    /// comma — the WRAP half of the width-aware "join or wrap" list
    /// layout shared by [`Printer::delimited_list`] and
    /// [`Printer::css`]'s own (delimiter-less) body. Multi-line parts are
    /// [`reindent`]ed under `level`'s column.
    fn wrap_one_per_line(&self, level: usize, parts: &[String]) -> String {
        let indent = self.indent(level);
        let mut out = String::new();
        for part in parts {
            out.push_str(&indent);
            out.push_str(&reindent(part, &indent));
            out.push_str(",\n");
        }
        out.pop();
        out
    }

    /// The width-aware "join with `, ` if it fits `max_width`, else one
    /// item per line with a trailing comma" layout used for every
    /// delimited kwarg/arg list in this printer: tag/component `(...)`
    /// kwargs, `Route(...)` kwargs, and a nested `css!`/`routes!` call.
    /// Returns just the delimited chunk (`(a, b)` or
    /// `(\n    a,\n    b,\n)`) — callers prepend their own tag/keyword
    /// name (already written to `out`, or folded into `prefix_width`).
    ///
    /// `check_level` is the level the group ACTUALLY sits at once
    /// printed — used ONLY for the width decision. This is what makes a
    /// deeply-nested value wrap instead of measuring itself against a
    /// shallow assumed depth; see [`Printer::nested_macro_src`].
    /// `output_level` is the level the WRAPPED form is indented to in the
    /// returned string: pass the same value as `check_level` for output
    /// written directly into the current line (tag/component kwargs,
    /// `Route(...)` kwargs), or `0` for a relative, column-0-anchored
    /// fragment the caller will [`reindent`] itself (a nested macro's own
    /// kwarg list, per the [`ExprMap`] contract). The two differ because
    /// the real ambient depth used for the width check is often not yet
    /// decided at output time — see [`Printer::kwarg`].
    ///
    /// `prefix_width`/`suffix_width` account for text sharing the
    /// group's own line that isn't one of `parts` (e.g. a tag name
    /// before the opening delimiter, or a trailing ` {` before a child
    /// block).
    #[allow(clippy::too_many_arguments)]
    fn delimited_list(
        &self,
        check_level: usize,
        output_level: usize,
        prefix_width: usize,
        parts: &[String],
        open: char,
        close: char,
        suffix_width: usize,
    ) -> String {
        let inline = parts.join(", ");
        let delimited = format!("{open}{inline}{close}");
        let fits = !inline.contains('\n')
            && self.opts.indent_width(check_level) + prefix_width + delimited.len() + suffix_width
                <= self.opts.max_width;
        if fits {
            return delimited;
        }
        let body = self.wrap_one_per_line(output_level + 1, parts);
        format!("{open}\n{body}\n{}{close}", self.indent(output_level))
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

    /// First source byte of an [`IrNode`] (its tag ident / spread expr /
    /// `children()` ident). Shared by `render!` and `routes!` printing —
    /// both reduce to the same tag/kwargs/children shape.
    fn ir_node_start_byte(&self, node: &IrNode) -> Option<usize> {
        let span = match node {
            IrNode::Tag(tag) => tag.tag_span?,
            IrNode::ChildrenSlot(span) => *span,
            IrNode::Spread(expr) => span_of(expr),
        };
        self.map.byte_range(span).map(|(s, _)| s)
    }

    /// Byte extent `(start, end)` of a node via [`Printer::ir_node_start_byte`]
    /// + [`SourceMap::node_extent`]. `end` is the byte just past the node.
    fn ir_node_extent(&self, node: &IrNode) -> Option<(usize, usize)> {
        let start = self.ir_node_start_byte(node)?;
        let (_, after) = self.map.node_extent(start);
        Some((start, after))
    }

    // ---- render! / routes! (shared tag/kwargs/children shape) ---------

    fn ir_node(&self, node: &IrNode, level: usize, out: &mut String) {
        match node {
            IrNode::Tag(tag) => self.ir_tag(tag, level, out),
            IrNode::ChildrenSlot(_) => {
                out.push_str(&self.indent(level));
                out.push_str("children()");
            }
            IrNode::Spread(expr) => {
                let indent = self.indent(level);
                let src = self.expr_src(span_of(expr), expr, level);
                out.push_str(&indent);
                out.push_str("..");
                out.push_str(&src);
            }
        }
    }

    /// The shared `tag(kwargs) { children }` rendering — covers a
    /// render! element/user-component (`always_block: false`, so an
    /// empty comment-free block is omitted) and a routes! `Switch`/
    /// `Stack`/`Route`/unrecognized-ident node (`Switch`/`Stack` set
    /// `always_block: true` since their `{ … }` is mandatory even when
    /// empty).
    fn ir_tag(&self, tag: &IrTag, level: usize, out: &mut String) {
        let indent = self.indent(level);
        out.push_str(&indent);
        out.push_str(&tag.tag);

        // ---- kwargs ----
        if !tag.kwargs.is_empty() {
            let parts: Vec<String> = tag
                .kwargs
                .iter()
                .map(|kw| self.ir_kwarg(kw, level))
                .collect();
            let suffix = ir_brace_width(&tag.children, tag.always_block);
            out.push_str(&self.delimited_list(
                level,
                level,
                tag.tag.len(),
                &parts,
                '(',
                ')',
                suffix,
            ));
        }

        // ---- children ----
        // Resolve this node's block byte bounds so comments are placed
        // relative to its `{ … }`.
        let inner_close = tag
            .tag_span
            .and_then(|s| self.map.byte_range(s))
            .and_then(|(s, _)| self.map.node_extent(s).0);

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

        if !tag.children.is_empty() || tag.always_block || has_block_comments {
            out.push_str(" {\n");
            let child_level = level + 1;
            for child in &tag.children {
                // Leading own-line comments before this child.
                if let Some(cs) = self.ir_node_start_byte(child) {
                    self.flush(cs, child_level, out);
                }
                self.ir_node(child, child_level, out);
                // Trailing same-line comment on the child.
                if let Some((_, child_end)) = self.ir_node_extent(child) {
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

    /// `level` is the tag's own indent level — if the value turns out to
    /// need wrapping (e.g. a nested `css!` that doesn't fit), it lands
    /// one level deeper (`level + 1`) once the kwargs break onto their
    /// own lines, so that is what gets passed to [`Printer::expr_src`]
    /// as the nested macro's width reference (`IrValue::Literal` values —
    /// routes!'s `path`/`component` — never go through `expr_src` at all,
    /// matching how they were always hand-formatted rather than treated
    /// as embedded exprs). See [`Printer::nested_macro_src`] for why this
    /// is a same-turn estimate rather than the exact final depth.
    fn ir_kwarg(&self, kw: &IrKwarg, level: usize) -> String {
        match &kw.value {
            // Partial kwarg: just the name (mid-typing). Preserve the
            // author's `name` with no value.
            None => kw.name.clone(),
            Some(IrValue::Literal(s)) => format!("{}: {s}", kw.name),
            Some(IrValue::Expr(e)) => {
                let value = self.expr_src(span_of(e), e, level + 1);
                format!("{}: {value}", kw.name)
            }
        }
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
            let parts: Vec<String> = input
                .kwargs
                .iter()
                .map(|kw| self.css_kwarg(kw, level))
                .collect();
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
            out.push_str(&reindent(&self.css_kwarg(kw, level), &indent));
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

    fn css_kwarg(&self, kw: &whisker_macro_syntax::CssKwarg, level: usize) -> String {
        let name = kw.name.to_string();
        match &kw.value {
            Some(expr) => {
                let v = self.expr_src(span_of(expr), expr, level);
                format!("{name}: {v}")
            }
            None => name,
        }
    }
}

/// Width contribution of a ` { … }` children block when deciding
/// whether a node's kwargs fit inline. A non-empty children block, or a
/// tag whose block is mandatory even when empty (`always_block` — routes!
/// `Switch`/`Stack`), always forces a multi-line body, so we only need
/// the ` {` opener's width to be honest about the first line; the
/// closing brace and the children sit on later lines.
fn ir_brace_width(children: &[IrNode], always_block: bool) -> usize {
    if children.is_empty() && !always_block {
        0
    } else {
        " {".len()
    }
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

/// Wrap a nested `render!`/`routes!` macro's already-printed `body` with
/// its `name!` prefix and delimiters: `name!open\nbody\nclose` if `body`
/// is multi-line, else the fully collapsed `name!openbodyclose` inline
/// form (stripping the leading indent `print_render`/`print_routes`
/// bakes into a single-line body's first line, since a nested value
/// isn't on its own line).
fn nested_wrap(name: &str, bang: &str, open: char, close: char, body: &str) -> String {
    if body.contains('\n') {
        format!("{name}{bang}{open}\n{body}\n{close}")
    } else {
        format!("{name}{bang}{open}{}{close}", body.trim_start())
    }
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
