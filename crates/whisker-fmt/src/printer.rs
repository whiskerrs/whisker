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
use crate::expr_fmt::{ExprFormatter, ExprMap};
use crate::ir::{IrKwarg, IrNode, IrTag, IrValue};
use crate::options::FmtOptions;
use crate::source_map::SourceMap;
use proc_macro2::{Span, TokenStream, TokenTree};
use quote::ToTokens;
use std::cell::Cell;
use syn::Expr;
use whisker_macro_syntax::compose::{ComposeArgument, ComposeArguments};

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
    exprfmt: Option<&ExprFormatter>,
    comments: &[GrammarComment],
    body_len: usize,
) -> String {
    let p = Printer {
        map,
        opts,
        expr_map,
        exprfmt,
        comments,
        next: Cell::new(0),
        prev_end: Cell::new(None),
    };
    let mut out = String::new();
    if let Some(start) = p.ir_node_start_byte(root) {
        p.flush(start, base_indent + 1, &mut out);
    }
    p.ir_node(root, base_indent + 1, &mut out);
    // A trailing comment on the root node's own last line attaches inline.
    if let Some((_, after)) = p.ir_node_extent(root)
        && let Some(idx) = p.pending_trailing_on_line(after)
    {
        let before = p.comments[idx].start + 1;
        p.flush(before, base_indent + 1, &mut out);
    }
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
///
/// `inline_budget` is the char count the joined field list may have and
/// still fit the macro's ORIGINAL line (prefix, delimiters and trailing
/// text included) — the caller inlines a single-line result there. Over
/// budget the body goes one field per line: there is no middle form
/// with broken delimiters around one joined line.
#[allow(clippy::too_many_arguments)]
pub(crate) fn print_css(
    input: &ComposeArguments,
    map: &SourceMap,
    opts: &FmtOptions,
    base_indent: usize,
    expr_map: &ExprMap,
    exprfmt: Option<&ExprFormatter>,
    comments: &[GrammarComment],
    body_len: usize,
    inline_budget: usize,
) -> String {
    let p = Printer {
        map,
        opts,
        expr_map,
        exprfmt,
        comments,
        next: Cell::new(0),
        prev_end: Cell::new(None),
    };
    p.css(input, base_indent + 1, body_len, inline_budget)
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
    exprfmt: Option<&ExprFormatter>,
    comments: &[GrammarComment],
    body_len: usize,
) -> String {
    let p = Printer {
        map,
        opts,
        expr_map,
        exprfmt,
        comments,
        next: Cell::new(0),
        prev_end: Cell::new(None),
    };
    let mut out = String::new();
    let level = base_indent + 1;
    for (i, node) in roots.iter().enumerate() {
        if let Some(start) = p.ir_node_start_byte(node) {
            p.flush(start, level, &mut out);
            p.maybe_blank_line(start, &mut out);
        }
        p.ir_node(node, level, &mut out);
        if let Some((_, end)) = p.ir_node_extent(node) {
            p.prev_end.set(Some(end));
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
    /// Threaded through so a re-entrant macro pass over an expr fragment
    /// ([`Printer::fragment_macro_src`]) formats ITS embedded exprs with
    /// the same rustfmt settings as the enclosing body.
    exprfmt: Option<&'a ExprFormatter>,
    comments: &'a [GrammarComment],
    /// Index of the next unconsumed comment.
    next: Cell<usize>,
    /// Source byte just past the previously emitted item (sibling or
    /// comment), for carrying authored blank lines between items.
    /// `None` suppresses the blank-line check (block start).
    prev_end: Cell<Option<usize>>,
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
        let mut fragment = if let Some(formatted) = self.expr_map.get(span) {
            formatted.to_string()
        } else if let Some(s) = self.map.slice(span) {
            // Continuation lines must be dedented to column 0 (the
            // [`ExprMap`] contract) or the later [`reindent`] compounds
            // the indentation on every pass.
            dedent_continuation(s.trim())
        } else {
            return expr.to_token_stream().to_string();
        };
        if contains_target_macro(expr.to_token_stream()) {
            // Unblock BEFORE the re-entrant pass so the macro body is
            // measured at the depth it will actually print at.
            if let Some(s) = unblock_macro_closure(&fragment, &self.opts.indent_unit()) {
                fragment = s;
            }
            if let Some(s) = self.fragment_macro_src(&fragment, level) {
                fragment = s;
            }
        }
        fragment
    }

    /// Re-enter the whole macro pass over an expr fragment that contains
    /// a `render!`/`css!`/`routes!` call somewhere INSIDE it (nested in a
    /// closure, a method chain, …) where [`Printer::nested_macro_src`]'s
    /// direct-`Expr::Macro` fast path can't reach. The fragment is
    /// wrapped in a synthetic fn at `level`'s indent — so nested bodies
    /// measure width at their real depth — reformatted by
    /// [`crate::reformat_macros_pass`] (comment fail-safes included),
    /// then unwrapped back to a column-0-anchored fragment.
    fn fragment_macro_src(&self, fragment: &str, level: usize) -> Option<String> {
        const HEAD: &str = "fn __wsk_frag() {\nlet _x =\n";
        const TAIL: &str = ";\n}\n";
        let indent = self.indent(level);
        let mut synthetic = String::from(HEAD);
        for (i, line) in fragment.split('\n').enumerate() {
            if i > 0 {
                synthetic.push('\n');
            }
            if !line.is_empty() {
                synthetic.push_str(&indent);
                synthetic.push_str(line);
            }
        }
        synthetic.push_str(TAIL);
        let out = crate::reformat_macros_pass(&synthetic, self.opts, self.exprfmt, true).ok()?;
        let inner = out.strip_prefix(HEAD)?.strip_suffix(TAIL)?;
        let mut result = String::new();
        for (i, line) in inner.split('\n').enumerate() {
            if i > 0 {
                result.push('\n');
            }
            result.push_str(line.strip_prefix(indent.as_str()).unwrap_or(line));
        }
        Some(result)
    }

    /// If `expr` is a `css!( … )` / `render!{ … }` / `routes!{ … }` macro
    /// call, recursively format its body with the grammar-aware printer
    /// instead of treating it as an opaque expression.
    ///
    /// `level` is the caller's best estimate of the indent level the
    /// nested macro's line will sit at once the OUTER node's
    /// inline-vs-wrap decision is made (callers pass their own `level`,
    /// +1 when a wrap would push the value one level deeper). It is used
    /// ONLY as the width reference for the nested call's own fit check
    /// ([`Printer::delimited_list`]), so a deeply-nested value doesn't
    /// measure itself against a shallow assumed depth. It cannot be
    /// exact: the true depth depends on the outer wrap decision, which
    /// depends circularly on this one.
    ///
    /// Returns `None` — falling back to [`Printer::fragment_macro_src`]'s
    /// re-entrant pass in [`Printer::expr_src`] — when `expr` isn't one
    /// of those macros, its body doesn't parse or is empty, or its source
    /// carries a comment (this fast path has no comment reattachment; the
    /// fragment pass does).
    fn nested_macro_src(&self, expr: &Expr, level: usize) -> Option<String> {
        let Expr::Macro(em) = expr else {
            return None;
        };
        let name = em.mac.path.get_ident()?.to_string();
        if name != "compose" && name != "css" && name != "render" && name != "routes" {
            return None;
        }
        // The delimiter span, not `mac.tokens`'s: the latter excludes a
        // comment sitting right inside `(`/`{` or right before `)`/`}`,
        // which the fail-safe below must see.
        let (open, close, delim_span) = match &em.mac.delimiter {
            syn::MacroDelimiter::Paren(p) => ('(', ')', p.span),
            syn::MacroDelimiter::Brace(b) => ('{', '}', b.span),
            syn::MacroDelimiter::Bracket(bk) => ('[', ']', bk.span),
        };
        let full_src = self.map.slice(delim_span.join())?;
        // String-aware comment scan: a `//` inside a string literal
        // (`"https://…"`) must not force the fallback.
        let full_map = SourceMap::new(full_src);
        if !crate::comments::collect_grammar_comments(full_src, &[], &full_map).is_empty() {
            return None;
        }
        // rustfmt spaces a brace-delimited macro's `{` but not `(`/`[`.
        let bang = if open == '{' { "! " } else { "!" };
        match name.as_str() {
            "css" => {
                let input = syn::parse2::<ComposeArguments>(em.mac.tokens.clone()).ok()?;
                if input.arguments.is_empty() {
                    return None;
                }
                let parts: Vec<String> = input
                    .arguments
                    .iter()
                    .map(|kw| self.css_kwarg(kw, level))
                    .collect();
                let force_wrap = full_map.trailing_comma_in(1, full_src.len().saturating_sub(1));
                // `output_level = 0`: a relative, column-0-anchored
                // fragment — see `delimited_list`'s doc.
                let list = self.delimited_list(
                    level,
                    0,
                    name.len() + bang.len(),
                    &parts,
                    open,
                    close,
                    0,
                    force_wrap,
                );
                Some(format!("{name}{bang}{list}"))
            }
            "compose" | "render" => {
                let input =
                    whisker_macro_syntax::compose::parse_input(em.mac.tokens.clone()).ok()?;
                let mut roots = crate::ir::adapt_compose_input(&input);
                if roots.len() != 1 {
                    return None;
                }
                let ir_root = roots.remove(0);
                let body = print_render(
                    &ir_root,
                    self.map,
                    self.opts,
                    0,
                    self.expr_map,
                    self.exprfmt,
                    &[],
                    0,
                );
                Some(nested_wrap(&name, bang, open, close, &body))
            }
            "routes" => {
                let input =
                    whisker_macro_syntax::compose::parse_input(em.mac.tokens.clone()).ok()?;
                if input.nodes.is_empty() {
                    return None;
                }
                let roots = crate::ir::adapt_compose_input(&input);
                let body = print_routes(
                    &roots,
                    self.map,
                    self.opts,
                    0,
                    self.expr_map,
                    self.exprfmt,
                    &[],
                    0,
                );
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
    /// printed, used ONLY for the width decision; see
    /// [`Printer::nested_macro_src`]. `output_level` is the level the
    /// WRAPPED form is indented to: the same value as `check_level` for
    /// output written straight into the current line, or `0` for a
    /// column-0-anchored fragment the caller [`reindent`]s itself (per
    /// the [`ExprMap`] contract). They differ because the real ambient
    /// depth is often not yet decided at output time.
    ///
    /// `prefix_width`/`suffix_width` account for text sharing the
    /// group's own line that isn't one of `parts` (e.g. a tag name
    /// before the opening delimiter, or a trailing ` {` before a child
    /// block).
    ///
    /// `force_wrap` is the author's keep-vertical hint (a trailing comma
    /// in the source list): the wrapped form is used even when the
    /// inline form would fit.
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
        force_wrap: bool,
    ) -> String {
        let inline = parts.join(", ");
        let delimited = format!("{open}{inline}{close}");
        let fits = !force_wrap
            && !inline.contains('\n')
            && self.opts.indent_width(check_level) + prefix_width + delimited.len() + suffix_width
                <= self.opts.max_width;
        if fits {
            return delimited;
        }
        // A multi-line argument makes the containing argument list
        // vertical as well. Keeping the opening delimiter beside the
        // first line of a nested macro produces an asymmetric shape such
        // as `View(style: css!(\n...))`; propagating the break outward
        // matches rustfmt's ordinary nested-call layout.
        let body = self.wrap_one_per_line(output_level + 1, parts);
        format!("{open}\n{body}\n{}{close}", self.indent(output_level))
    }

    fn indent(&self, level: usize) -> String {
        self.opts.indent_prefix(level)
    }

    /// Emit ONE blank line when the source had one or more between the
    /// previously emitted item ([`Printer::prev_end`]) and the item
    /// starting at `next_start`.
    fn maybe_blank_line(&self, next_start: usize, out: &mut String) {
        if let Some(prev) = self.prev_end.get()
            && self.map.has_blank_line_between(prev, next_start)
        {
            out.push('\n');
        }
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
                self.maybe_blank_line(c.start, out);
                out.push_str(&indent);
                out.push_str(&reindent(&c.text, &indent));
                out.push('\n');
            } else {
                // Strip a trailing newline the caller may already have
                // pushed, append ` text`, then restore it.
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
            self.prev_end.set(Some(c.end));
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

    /// First source byte of an [`IrNode`] (its tag path or expression).
    /// both reduce to the same tag/kwargs/children shape.
    fn ir_node_start_byte(&self, node: &IrNode) -> Option<usize> {
        let span = match node {
            IrNode::Tag(tag) => tag.tag_span?,
            IrNode::Text(value) => value.span(),
            IrNode::Expression(expr) => span_of(expr),
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
            IrNode::Text(value) => {
                out.push_str(&self.indent(level));
                out.push_str(&format!("{:?}", value.value()));
            }
            IrNode::Expression(expr) => {
                let indent = self.indent(level);
                let src = self.expr_src(span_of(expr), expr, level);
                out.push_str(&indent);
                out.push('{');
                out.push_str(&src);
                out.push('}');
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

        let paren_range = tag
            .tag_span
            .and_then(|s| self.map.byte_range(s))
            .and_then(|(start, _)| self.map.kwarg_paren_range(start));
        // A pending comment inside the parens anchors to its kwarg, so
        // the list must print one kwarg per line with interleaved
        // flushes instead of going through `delimited_list`.
        let kwarg_comments = paren_range
            .map(|(open, close)| {
                self.comments
                    .get(self.next.get())
                    .map(|c| c.start > open && c.start < close)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if !tag.kwargs.is_empty() && kwarg_comments {
            self.kwargs_with_comments(tag, level, paren_range, out);
        } else if !tag.kwargs.is_empty() {
            let parts: Vec<String> = tag
                .kwargs
                .iter()
                .map(|kw| self.ir_kwarg(kw, level))
                .collect();
            let suffix = ir_brace_width(&tag.children, tag.always_block);
            let force_wrap = paren_range
                .map(|(open, close)| self.map.trailing_comma_in(open + 1, close))
                .unwrap_or(false);
            out.push_str(&self.delimited_list(
                level,
                level,
                tag.tag.len(),
                &parts,
                '(',
                ')',
                suffix,
                force_wrap,
            ));
        } else if paren_range.is_some() {
            // The author's empty `()` is preserved, not stripped.
            out.push_str("()");
        }

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
            // Blank lines at the block's edges are dropped: the check is
            // disarmed until the first child/comment sets a new anchor.
            self.prev_end.set(None);
            for child in &tag.children {
                if let Some(cs) = self.ir_node_start_byte(child) {
                    self.flush(cs, child_level, out);
                    self.maybe_blank_line(cs, out);
                }
                self.ir_node(child, child_level, out);
                if let Some((_, child_end)) = self.ir_node_extent(child) {
                    self.prev_end.set(Some(child_end));
                    if let Some(idx) = self.pending_trailing_on_line(child_end) {
                        let before = self.comments[idx].start + 1;
                        self.flush(before, child_level, out);
                    }
                }
                out.push('\n');
            }
            if let Some(close) = inner_close {
                self.flush(close, child_level, out);
            }
            out.push_str(&indent);
            out.push('}');
            if let Some(close) = inner_close {
                self.prev_end.set(Some(close + 1));
            }
        }
    }

    /// One kwarg per line with comments flushed in place: own-line
    /// comments at the kwarg indent, trailing comments after the comma
    /// of the kwarg whose line they share.
    fn kwargs_with_comments(
        &self,
        tag: &IrTag,
        level: usize,
        paren_range: Option<(usize, usize)>,
        out: &mut String,
    ) {
        out.push_str("(\n");
        let kw_level = level + 1;
        let indent = self.indent(kw_level);
        self.prev_end.set(None);
        for kw in &tag.kwargs {
            if let Some((s, _)) = kw.name_span.and_then(|sp| self.map.byte_range(sp)) {
                self.flush(s, kw_level, out);
                self.maybe_blank_line(s, out);
            }
            out.push_str(&indent);
            out.push_str(&reindent(&self.ir_kwarg(kw, level), &indent));
            out.push(',');
            let end = kw
                .value_span
                .or(kw.name_span)
                .and_then(|sp| self.map.byte_range(sp))
                .map(|(_, e)| e);
            if let Some(end) = end {
                self.prev_end.set(Some(end));
                if let Some(idx) = self.pending_trailing_on_line(end) {
                    let before = self.comments[idx].start + 1;
                    self.flush(before, kw_level, out);
                }
            }
            out.push('\n');
        }
        if let Some((_, close)) = paren_range {
            self.flush(close, kw_level, out);
        }
        out.push_str(&self.indent(level));
        out.push(')');
        if let Some((_, close)) = paren_range {
            self.prev_end.set(Some(close + 1));
        }
    }

    /// `level` is the tag's own indent level; a value that needs wrapping
    /// lands at `level + 1`, so that is what reaches
    /// [`Printer::expr_src`] as the nested macro's width reference.
    fn ir_kwarg(&self, kw: &IrKwarg, level: usize) -> String {
        match &kw.value {
            // Partial kwarg: just the name (mid-typing). Preserve the
            // author's `name` with no value.
            None => kw.name.clone(),
            Some(IrValue::Expr(e)) => {
                let value = self.expr_src(span_of(e), e, level + 1);
                format!("{}: {value}", kw.name)
            }
        }
    }

    // ---- css! ----------------------------------------------------------

    fn css(
        &self,
        input: &ComposeArguments,
        level: usize,
        body_len: usize,
        inline_budget: usize,
    ) -> String {
        if input.arguments.is_empty() {
            return String::new();
        }

        let has_comments = !self.comments.is_empty();

        // Inline form only when there are NO comments to place (comments
        // imply line breaks), the author left no keep-vertical hint (a
        // trailing comma after the last field), and the joined list fits
        // the macro's original line (`inline_budget`).
        if !has_comments {
            let parts: Vec<String> = input
                .arguments
                .iter()
                .map(|kw| self.css_kwarg(kw, level))
                .collect();
            let force_wrap = self.map.trailing_comma_in(0, body_len);
            let inline = parts.join(", ");
            if !force_wrap && !inline.contains('\n') && inline.chars().count() <= inline_budget {
                let indent = self.indent(level);
                return format!("{indent}{inline}");
            }
            let indent = self.indent(level);
            let mut out = String::new();
            for (kw, part) in input.arguments.iter().zip(&parts) {
                if let Some((s, _)) = self.map.byte_range(kw.name.span()) {
                    self.maybe_blank_line(s, &mut out);
                }
                out.push_str(&indent);
                out.push_str(&reindent(part, &indent));
                out.push_str(",\n");
                self.prev_end.set(Some(css_field_end(self.map, kw)));
            }
            out.pop();
            return out;
        }

        // Comment-bearing css! body: one field per line, flushing
        // comments before each field and after the last.
        let indent = self.indent(level);
        let mut out = String::new();
        for kw in &input.arguments {
            let start = self.map.byte_range(kw.name.span()).map(|(s, _)| s);
            if let Some(s) = start {
                self.flush(s, level, &mut out);
                self.maybe_blank_line(s, &mut out);
            }
            out.push_str(&indent);
            out.push_str(&reindent(&self.css_kwarg(kw, level), &indent));
            out.push(',');
            let line_end = css_field_end(self.map, kw);
            self.prev_end.set(Some(line_end));
            if let Some(idx) = self.pending_trailing_on_line(line_end) {
                let before = self.comments[idx].start + 1;
                self.flush(before, level, &mut out);
            }
            out.push('\n');
        }
        self.flush(body_len, level, &mut out);
        // strip trailing newline (caller adds delimiters)
        while out.ends_with('\n') {
            out.pop();
        }
        out
    }

    fn css_kwarg(&self, kw: &ComposeArgument, level: usize) -> String {
        let name = kw.name.to_string();
        if kw.partial {
            name
        } else {
            let v = self.expr_src(span_of(&kw.value), &kw.value, level);
            format!("{name}: {v}")
        }
    }
}

/// Byte just past a css field in the source: the value's end when
/// present, else the name's.
fn css_field_end(map: &SourceMap, kw: &ComposeArgument) -> usize {
    (!kw.partial)
        .then_some(&kw.value)
        .and_then(|e| map.byte_range(span_of(e)))
        .map(|(_, e)| e)
        .or_else(|| map.byte_range(kw.name.span()).map(|(_, e)| e))
        .unwrap_or(0)
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
/// relative to the first line (which the caller already trimmed). This is
/// what makes the later [`reindent`] idempotent.
///
/// All-whitespace lines are ignored when computing the common indent, so
/// they can't force it to zero.
fn dedent_continuation(fragment: &str) -> String {
    if !fragment.contains('\n') {
        return fragment.to_string();
    }
    let mut lines = fragment.split('\n');
    let first = lines.next().unwrap_or("");
    let rest: Vec<&str> = lines.collect();
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

/// Rewrite a closure whose block body is EXACTLY one `render!`/`css!`/
/// `routes!` call — `|args| { mac! { … } }` — to `|args| mac! { … }`.
/// rustfmt blocks such closure bodies in the embedded-expr pass, but a
/// macro-body slot is whisker-fmt's to lay out and the unblocked form
/// is the DSL idiom. Returns `None` (leave the fragment alone) unless
/// the pattern matches exactly: a comment or second statement in the
/// block keeps the block.
fn unblock_macro_closure(fragment: &str, unit: &str) -> Option<String> {
    let mut lines = fragment.lines();
    let header = lines.next()?.strip_suffix(" {")?;
    if !header.trim_end().ends_with('|') {
        return None;
    }
    let rest: Vec<&str> = lines.collect();
    let (&last, middle) = rest.split_last()?;
    if last != "}" || middle.is_empty() {
        return None;
    }
    let mut body = String::new();
    for (i, line) in middle.iter().enumerate() {
        if i > 0 {
            body.push('\n');
        }
        body.push_str(line.strip_prefix(unit).unwrap_or(line));
    }
    for name in ["compose!", "render!", "css!", "routes!"] {
        if body.trim_start().starts_with(name) {
            let ts: TokenStream = body.parse().ok()?;
            let trees: Vec<TokenTree> = ts.into_iter().collect();
            let sole_macro = matches!(
                trees.as_slice(),
                [TokenTree::Ident(_), TokenTree::Punct(p), TokenTree::Group(_)]
                    if p.as_char() == '!'
            );
            if !sole_macro {
                return None;
            }
            let candidate = format!("{header} {body}");
            return syn::parse_str::<Expr>(&candidate).ok().map(|_| candidate);
        }
    }
    None
}

/// `true` if the token stream contains a `render!`/`css!`/`routes!`
/// invocation (`IDENT ! GROUP`) at any nesting depth.
fn contains_target_macro(tokens: TokenStream) -> bool {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (i, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Ident(ident)
                if matches!(
                    ident.to_string().as_str(),
                    "compose" | "render" | "css" | "routes"
                ) && matches!(trees.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == '!')
                    && matches!(trees.get(i + 2), Some(TokenTree::Group(_))) =>
            {
                return true;
            }
            TokenTree::Group(group) if contains_target_macro(group.stream()) => {
                return true;
            }
            _ => {}
        }
    }
    false
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
        let pad = if open == '{' { " " } else { "" };
        format!("{name}{bang}{open}{pad}{}{pad}{close}", body.trim_start())
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
