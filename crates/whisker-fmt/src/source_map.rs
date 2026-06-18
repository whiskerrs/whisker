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

    /// `true` if a `\n` lies in the byte range `[lo, hi)` of the source.
    /// Used to decide whether a trailing comment is on the same source
    /// line as a node it should attach to.
    pub(crate) fn between_has_newline(&self, lo: usize, hi: usize) -> bool {
        let lo = lo.min(self.src.len());
        let hi = hi.min(self.src.len());
        if hi <= lo {
            return false;
        }
        self.src.as_bytes()[lo..hi].contains(&b'\n')
    }

    /// Locate a node's `tag(kwargs?) { children? }` extent starting at
    /// byte `start` (the first byte of its tag ident).
    ///
    /// Returns `(inner_close, after)` where:
    /// - `inner_close` is the byte offset of this node's closing `}` (the
    ///   `}` itself), or `None` for a childless node (no `{ … }` block).
    /// - `after` is the byte just past the whole node.
    ///
    /// The scan is balanced and string / char / comment aware so braces
    /// inside literals or comments do not miscount. It skips: the ident,
    /// an optional balanced `( … )`, then an optional balanced `{ … }`.
    pub(crate) fn node_extent(&self, start: usize) -> (Option<usize>, usize) {
        let bytes = self.src.as_bytes();
        let len = bytes.len();
        // Skip the ident: identifier chars (alnum / `_`).
        let mut i = start;
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        // Skip whitespace + comments to the next significant byte.
        i = self.skip_trivia(i);
        // Optional `( … )`.
        if i < len && bytes[i] == b'(' {
            i = self.skip_balanced(i, b'(', b')');
        }
        let after_parens = i;
        i = self.skip_trivia(i);
        // Optional `{ … }`.
        if i < len && bytes[i] == b'{' {
            let close = self.skip_balanced(i, b'{', b'}');
            // `close` is just past `}`; the `}` byte is `close - 1`.
            return (Some(close - 1), close);
        }
        (None, after_parens)
    }

    /// Advance past whitespace and comments (line + nested block) from
    /// `i`, returning the index of the next significant byte.
    fn skip_trivia(&self, mut i: usize) -> usize {
        let bytes = self.src.as_bytes();
        let len = bytes.len();
        loop {
            // whitespace
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                let mut depth = 1usize;
                i += 2;
                while i < len && depth > 0 {
                    if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            break;
        }
        i
    }

    /// Skip a balanced `open … close` region starting at byte `i` (which
    /// must be `open`), returning the index just past the matching
    /// `close`. String / char literals and comments inside are skipped so
    /// their delimiters don't affect the balance.
    fn skip_balanced(&self, mut i: usize, open: u8, close: u8) -> usize {
        let bytes = self.src.as_bytes();
        let len = bytes.len();
        let mut depth = 0usize;
        while i < len {
            let b = bytes[i];
            if b == b'"' || b == b'\'' {
                i = self.skip_string(i, b);
                continue;
            }
            if i + 1 < len && b == b'/' && bytes[i + 1] == b'/' {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if i + 1 < len && b == b'/' && bytes[i + 1] == b'*' {
                let mut d = 1usize;
                i += 2;
                while i < len && d > 0 {
                    if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        d += 1;
                        i += 2;
                    } else if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        d -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            i += 1;
        }
        i
    }

    /// Skip a string or char literal that opens with quote `q` at byte
    /// `i`, returning the index just past the closing quote. Handles
    /// backslash escapes. (Raw strings are uncommon in macro bodies; a
    /// best-effort scan is acceptable here since the result only affects
    /// comment placement, never correctness of the emitted Rust.)
    fn skip_string(&self, mut i: usize, q: u8) -> usize {
        let bytes = self.src.as_bytes();
        let len = bytes.len();
        i += 1; // past opening quote
        while i < len {
            let b = bytes[i];
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                return i + 1;
            }
            i += 1;
        }
        i
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
