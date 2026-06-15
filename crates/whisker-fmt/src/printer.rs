//! Width-aware pretty-printer for `render!` and `css!` bodies.
//!
//! ## Embedded Rust expressions
//!
//! Kwarg values, event-handler closures and any other embedded Rust
//! are NOT re-printed from tokens. They are sliced verbatim out of the
//! original macro-body source via [`SourceMap`] (see `source_map.rs`),
//! so the user's own expression formatting is preserved exactly. We
//! only fall back to `proc_macro2` token printing when the slice fails
//! (e.g. a synthesized span with no source position) — a rare,
//! best-effort path. This choice keeps the formatter from fighting
//! rustfmt over expression internals.
//!
//! ## Layout
//!
//! A simple indent-and-wrap scheme (not full Wadler/Prettier): each
//! node renders inline if it fits within `max_width` at its current
//! indent, otherwise the kwargs and/or children break onto their own
//! indented lines. This matches the shallow, regular `render!` grammar
//! and is easy to keep idempotent.

use crate::options::FmtOptions;
use crate::source_map::SourceMap;
use proc_macro2::Span;
use quote::ToTokens;
use whisker_macro_syntax::{CssInput, ElementNode, Kwarg, Node, Root, UserComponentNode};

/// Pretty-print a parsed `render!` body.
///
/// `base_indent` is the indent level (in tab-units) at which the macro
/// invocation sits in the rustfmt output; the body is indented one
/// level deeper.
pub(crate) fn print_render(
    root: &Root,
    map: &SourceMap,
    opts: &FmtOptions,
    base_indent: usize,
) -> String {
    let p = Printer { map, opts };
    let mut out = String::new();
    p.node(&root.node, base_indent + 1, &mut out);
    out
}

/// Pretty-print a parsed `css!` body.
pub(crate) fn print_css(
    input: &CssInput,
    map: &SourceMap,
    opts: &FmtOptions,
    base_indent: usize,
) -> String {
    let p = Printer { map, opts };
    p.css(input, base_indent + 1)
}

struct Printer<'a> {
    map: &'a SourceMap<'a>,
    opts: &'a FmtOptions,
}

impl Printer<'_> {
    /// Slice an embedded Rust expr verbatim from the source, falling
    /// back to token printing if the span has no source position.
    fn expr_src(&self, span: Span, tokens: &dyn ToTokens) -> String {
        if let Some(s) = self.map.slice(span) {
            // Trim surrounding whitespace; internal formatting is kept.
            s.trim().to_string()
        } else {
            tokens.to_token_stream().to_string()
        }
    }

    fn indent(&self, level: usize) -> String {
        self.opts.indent_prefix(level)
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
        self.tag_node(&el.tag.to_string(), &el.kwargs, &el.children, level, out);
    }

    fn user_component(&self, uc: &UserComponentNode, level: usize, out: &mut String) {
        self.tag_node(
            &uc.alias_ident.to_string(),
            &uc.kwargs,
            &uc.children,
            level,
            out,
        );
    }

    /// The shared `tag(kwargs) { children }` rendering used by both
    /// element and user-component nodes.
    fn tag_node(
        &self,
        tag: &str,
        kwargs: &[Kwarg],
        children: &[Node],
        level: usize,
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
        if !children.is_empty() {
            out.push_str(" {\n");
            for child in children {
                self.node(child, level + 1, out);
                out.push('\n');
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

    fn css(&self, input: &CssInput, level: usize) -> String {
        if input.kwargs.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = input
            .kwargs
            .iter()
            .map(|kw| {
                let name = kw.name.to_string();
                match &kw.value {
                    Some(expr) => {
                        let v = self.expr_src(span_of(expr), expr);
                        format!("{name}: {v}")
                    }
                    None => name,
                }
            })
            .collect();

        // Try a single inline line first.
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
        // strip trailing newline (caller adds delimiters)
        out.pop();
        out
    }
}

/// Width contribution of a ` { … }` children block when deciding
/// whether a node's kwargs fit inline. A non-empty children block
/// always forces a multi-line body, so we only need the ` {` opener's
/// width to be honest about the first line; the closing brace and the
/// children sit on later lines.
fn brace_width(children: &[Node]) -> usize {
    if children.is_empty() {
        0
    } else {
        " {".len()
    }
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
