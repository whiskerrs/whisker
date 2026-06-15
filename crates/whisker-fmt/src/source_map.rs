//! Slice the original source text out of a `proc_macro2::Span`.
//!
//! When `proc-macro2`'s `span-locations` feature is on, `Span::start()`
//! / `Span::end()` return `LineColumn` (1-based line, 0-based column)
//! *relative to the token stream the span came from*. We re-parse each
//! macro body as a standalone `TokenStream`, so the line/column are
//! relative to that body's own text — which is exactly the substring we
//! pass in here. That lets us recover the user's original expression
//! text verbatim (preserving their internal formatting) rather than
//! re-printing it from tokens (which would mangle spacing).

use proc_macro2::Span;

/// Maps `(line, column)` positions within a source blob to byte
/// offsets, so a [`Span`] can be turned back into the exact source
/// substring it covers.
pub(crate) struct SourceMap<'a> {
    src: &'a str,
    /// Byte offset of the start of each line (1-based line → index
    /// `line - 1`).
    line_starts: Vec<usize>,
}

impl<'a> SourceMap<'a> {
    pub(crate) fn new(src: &'a str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        SourceMap { src, line_starts }
    }

    /// Byte offset of a `(line, column)` location. `line` is 1-based,
    /// `column` is a 0-based *char* offset within the line (matching
    /// `proc_macro2::LineColumn`).
    fn offset(&self, line: usize, column: usize) -> Option<usize> {
        let line_start = *self.line_starts.get(line.checked_sub(1)?)?;
        // `column` counts chars; walk that many chars from line_start.
        let rest = &self.src[line_start..];
        let mut byte = line_start;
        let mut chars = column;
        for ch in rest.chars() {
            if chars == 0 {
                break;
            }
            byte += ch.len_utf8();
            chars -= 1;
        }
        Some(byte)
    }

    /// Byte range `(start, end)` covered by `span`, or `None` if the
    /// span has no resolvable source location.
    pub(crate) fn byte_range(&self, span: Span) -> Option<(usize, usize)> {
        let start = span.start();
        let end = span.end();
        if start.line == 0 {
            return None;
        }
        let s = self.offset(start.line, start.column)?;
        let e = self.offset(end.line, end.column)?;
        if e < s || e > self.src.len() {
            return None;
        }
        Some((s, e))
    }

    /// The exact source substring covered by `span`, or `None` if the
    /// span's locations don't resolve (e.g. a synthesized span with no
    /// real source position).
    pub(crate) fn slice(&self, span: Span) -> Option<&'a str> {
        let start = span.start();
        let end = span.end();
        // A synthesized/`call_site` span reports (1,0)..(1,0); guard
        // against that producing an empty (or bogus) slice.
        if start.line == 0 {
            return None;
        }
        let s = self.offset(start.line, start.column)?;
        let e = self.offset(end.line, end.column)?;
        if e < s || e > self.src.len() {
            return None;
        }
        Some(&self.src[s..e])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::TokenStream;
    use syn::spanned::Spanned;
    use syn::Expr;

    #[test]
    fn slices_expr_from_its_own_token_stream() {
        // Span locations are relative to the TokenStream the span came
        // from, so re-parse the exact substring we map against.
        let src = "a + b * c";
        let ts: TokenStream = src.parse().unwrap();
        let expr: Expr = syn::parse2(ts).unwrap();
        let map = SourceMap::new(src);
        assert_eq!(map.slice(expr.span()).unwrap(), "a + b * c");
    }

    #[test]
    fn slices_nested_subexpr() {
        let src = "foo(bar + 1)";
        let ts: TokenStream = src.parse().unwrap();
        let call: syn::ExprCall = syn::parse2(ts).unwrap();
        let map = SourceMap::new(src);
        // first (and only) argument is `bar + 1`
        let arg = call.args.first().unwrap();
        assert_eq!(map.slice(arg.span()).unwrap(), "bar + 1");
    }

    #[test]
    fn offset_math_multiline() {
        let src = "line one\nx = a +\n    b";
        let map = SourceMap::new(src);
        assert_eq!(map.offset(1, 0), Some(0));
        assert_eq!(map.offset(2, 0), Some(9));
        assert_eq!(map.offset(2, 4), Some(13));
    }

    #[test]
    fn slices_multiline_expr_verbatim() {
        // A user expression that spans lines should come back exactly
        // as written (this is what preserves the user's formatting).
        let src = "a\n    + b\n    + c";
        let ts: TokenStream = src.parse().unwrap();
        let expr: Expr = syn::parse2(ts).unwrap();
        let map = SourceMap::new(src);
        assert_eq!(map.slice(expr.span()).unwrap(), src);
    }
}
