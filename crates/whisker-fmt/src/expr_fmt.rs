//! Real-rustfmt formatting for the embedded Rust expressions inside
//! `render!` / `css!` bodies (kwarg values, event-handler closures).
//!
//! # Why
//!
//! The pretty-printer ([`crate::printer`]) lays out the macro *grammar*
//! (tags, kwargs, children) but treats each embedded expression as an
//! opaque slice. By default it keeps that slice verbatim. This module
//! upgrades the embedded exprs to *real* rustfmt formatting so e.g.
//! `format!("count: {}",c.get())` becomes `format!("count: {}", c.get())`
//! and long values wrap at `max_width`.
//!
//! # Approach (matches dioxus-fmt / yew-fmt)
//!
//! We format each expr's **source text** (not its AST — so comments and
//! the author's intent survive) by wrapping every expr of one macro body
//! into a single synthetic source:
//!
//! ```text
//! fn __wsk_e0() {
//! <expr0 source>
//! }
//! fn __wsk_e1() {
//! <expr1 source>
//! }
//! ```
//!
//! That source is run through the SAME rustfmt binary / `rustfmt.toml` /
//! edition as the base pass — exactly ONCE per macro body (one spawn,
//! not one per expr). We then re-parse rustfmt's output, find each
//! `__wsk_eN` fn, slice its body text, strip the `fn __wsk_eN() {` / `}`
//! wrapper lines and dedent the body by one indent level. The result is
//! the formatted expr at column 0, ready for the printer to splice in
//! and re-indent to the kwarg's column.
//!
//! # Column-shift limitation (MVP)
//!
//! rustfmt wraps each expr against the synthetic wrapper's SHALLOW
//! indent (one level), not against the expr's true, possibly-deeper
//! column in the macro. So a value that lands at, say, column 24 in the
//! final output may wrap a few columns later than a from-scratch rustfmt
//! run would. dioxus-fmt and yew-fmt have the same limitation. A future
//! refinement could compute a per-expr `max_width` adjusted by the
//! expr's target column and run a wrapper per width-bucket — deliberately
//! NOT done in this pass.
//!
//! # Fallbacks
//!
//! Every step degrades gracefully to the verbatim slice (the prior
//! behavior): no rustfmt binary, a wrapper that fails to format, or an
//! individual `__wsk_eN` body that can't be re-extracted all leave that
//! expr (or all exprs of the body) verbatim. A formatting hiccup in one
//! expr never errors the whole file.

use crate::options::FmtOptions;
use proc_macro2::{LineColumn, Span};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A map from an embedded expr's body-relative span to its
/// rustfmt-formatted text (dedented to column 0).
///
/// The key is the `(start, end)` [`LineColumn`] pair of the expr's span.
/// Because the macro body is re-parsed as its own standalone
/// `TokenStream`, these line/column positions are body-relative and
/// unique per expr within that body — a stable key the printer can look
/// up by `Span` alone.
#[derive(Default)]
pub(crate) struct ExprMap {
    inner: HashMap<SpanKey, String>,
}

impl ExprMap {
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Look up the formatted text for an expr by its span.
    pub(crate) fn get(&self, span: Span) -> Option<&str> {
        self.inner.get(&SpanKey::from(span)).map(String::as_str)
    }

    fn insert(&mut self, span: Span, formatted: String) {
        self.inner.insert(SpanKey::from(span), formatted);
    }
}

/// Hashable, comparable key derived from a span's start/end position.
#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct SpanKey {
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
}

impl From<Span> for SpanKey {
    fn from(span: Span) -> Self {
        let LineColumn {
            line: start_line,
            column: start_col,
        } = span.start();
        let LineColumn {
            line: end_line,
            column: end_col,
        } = span.end();
        SpanKey {
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }
}

/// Holds the rustfmt configuration so a batch of expr slices from one
/// macro body can be formatted with the SAME settings as the base pass.
pub(crate) struct ExprFormatter {
    opts: FmtOptions,
    config_dir: Option<PathBuf>,
}

impl ExprFormatter {
    /// Build a formatter that mirrors `format_source` (no explicit
    /// config dir — rustfmt resolves `rustfmt.toml` from cwd).
    pub(crate) fn new(opts: &FmtOptions) -> Self {
        Self {
            opts: opts.clone(),
            config_dir: None,
        }
    }

    /// Build a formatter that mirrors `format_source_in_dir`, threading
    /// the same `config_dir` through so the embedded exprs honor the
    /// same nearest `rustfmt.toml` as the base pass.
    pub(crate) fn new_in_dir(opts: &FmtOptions, config_dir: &Path) -> Self {
        Self {
            opts: opts.clone(),
            config_dir: Some(config_dir.to_path_buf()),
        }
    }

    /// Format every expr (given as `(span, source_slice)` pairs) of ONE
    /// macro body with a single rustfmt spawn, returning a map from span
    /// to formatted text. Exprs that fail to format are simply absent
    /// from the map (the printer then falls back to verbatim).
    ///
    /// `exprs` carries the body-relative span of each expr (for keying)
    /// alongside its verbatim source slice (the text we format).
    pub(crate) fn format_body(&self, exprs: &[(Span, String)]) -> ExprMap {
        let mut map = ExprMap::default();
        if exprs.is_empty() {
            return map;
        }

        // ---- build the batched synthetic source ----
        let mut synthetic = String::new();
        for (i, (_, src)) in exprs.iter().enumerate() {
            synthetic.push_str(&format!("fn __wsk_e{i}() {{\n"));
            synthetic.push_str(src);
            // Guarantee the body ends on its own line so the closing
            // brace is on a fresh line (slicing relies on this).
            if !src.ends_with('\n') {
                synthetic.push('\n');
            }
            synthetic.push_str("}\n");
        }

        // ---- one rustfmt spawn for the whole batch ----
        let formatted_file =
            match crate::run_rustfmt(&synthetic, &self.opts, self.config_dir.as_deref()) {
                Ok(s) => s,
                // No rustfmt, or the batch failed to parse/format as a whole
                // → leave every expr verbatim.
                Err(_) => return map,
            };

        // ---- extract each __wsk_eN body ----
        let bodies = match extract_bodies(&formatted_file, exprs.len()) {
            Some(b) => b,
            None => return map,
        };

        let unit = self.opts.indent_unit();
        for (i, (span, _)) in exprs.iter().enumerate() {
            if let Some(body) = bodies.get(i).and_then(|b| b.as_deref()) {
                let dedented = dedent_one_level(body, &unit);
                map.insert(*span, dedented);
            }
        }
        map
    }
}

/// Re-parse the rustfmt output, locate every `__wsk_eN` fn (in order),
/// and return its raw body text (between the `{` and `}` braces, newline
/// boundaries trimmed). Returns `None` if the output can't be parsed or
/// any wrapper fn is missing — so the caller falls back wholesale.
fn extract_bodies(formatted_file: &str, count: usize) -> Option<Vec<Option<String>>> {
    let parsed: syn::File = syn::parse_file(formatted_file).ok()?;
    let map = crate::source_map::SourceMap::new(formatted_file);

    // index -> body text
    let mut found: HashMap<usize, String> = HashMap::new();
    for item in &parsed.items {
        let syn::Item::Fn(f) = item else { continue };
        let name = f.sig.ident.to_string();
        let Some(idx) = name
            .strip_prefix("__wsk_e")
            .and_then(|n| n.parse::<usize>().ok())
        else {
            continue;
        };
        // The block's brace span covers `{ … }`. Slice the inside.
        let brace = f.block.brace_token.span;
        // `Span::join` of the open/close delimiters gives the full
        // `{ … }` range; use the group span directly.
        let span: Span = brace.join();
        let Some((open, close)) = map.byte_range(span) else {
            continue;
        };
        // Strip the outer braces: byte `open` is `{`, `close-1` is `}`.
        if close <= open + 1 {
            // Empty body — keep as empty string.
            found.insert(idx, String::new());
            continue;
        }
        let inner = &formatted_file[open + 1..close - 1];
        // Trim a single leading / trailing newline introduced by the
        // braces being on their own lines; keep internal indentation so
        // `dedent_one_level` can strip exactly one level.
        let inner = inner.strip_prefix('\n').unwrap_or(inner);
        let inner = inner.strip_suffix('\n').unwrap_or(inner);
        found.insert(idx, inner.to_string());
    }

    // Require every expected wrapper to be present.
    let mut bodies = Vec::with_capacity(count);
    for i in 0..count {
        match found.remove(&i) {
            Some(b) => bodies.push(Some(b)),
            None => return None,
        }
    }
    Some(bodies)
}

/// Strip exactly one indentation level (`unit`) from the front of every
/// non-blank line, leaving the expr at column 0. Lines that don't start
/// with the full unit (shouldn't happen for rustfmt output, but be
/// defensive) are left as-is. Blank lines stay blank.
fn dedent_one_level(body: &str, unit: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    for line in body.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        if line.is_empty() {
            continue;
        }
        match line.strip_prefix(unit) {
            Some(rest) => out.push_str(rest),
            None => out.push_str(line),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> FmtOptions {
        FmtOptions {
            max_width: 100,
            tab_spaces: 4,
            hard_tabs: false,
            edition: Some("2021".to_string()),
        }
    }

    #[test]
    fn dedent_strips_one_level() {
        let body = "    foo(a, b)\n    + bar";
        assert_eq!(dedent_one_level(body, "    "), "foo(a, b)\n+ bar");
    }

    #[test]
    fn dedent_keeps_blank_lines() {
        let body = "    a\n\n    b";
        assert_eq!(dedent_one_level(body, "    "), "a\n\nb");
    }

    #[test]
    fn extract_single_body() {
        let file = "fn __wsk_e0() {\n    foo(a, b)\n}\n";
        let bodies = extract_bodies(file, 1).unwrap();
        assert_eq!(bodies[0].as_deref(), Some("    foo(a, b)"));
    }

    #[test]
    fn extract_requires_all() {
        let file = "fn __wsk_e0() {\n    a\n}\n";
        // Asking for 2 but only 1 present → None.
        assert!(extract_bodies(file, 2).is_none());
    }

    #[test]
    fn format_body_empty_is_empty_map() {
        let f = ExprFormatter::new(&opts());
        let m = f.format_body(&[]);
        assert!(m.is_empty());
    }
}
