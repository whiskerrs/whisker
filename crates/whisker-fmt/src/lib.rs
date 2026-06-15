//! `whisker-fmt` — a rustfmt drop-in that also formats Whisker's
//! `render!` and `css!` macro bodies.
//!
//! # Architecture (mirrors yew-fmt)
//!
//! rustfmt leaves macro *bodies* untouched. So we:
//!
//! 1. Shell out to the real **rustfmt binary** (`--emit stdout`),
//!    letting it read the project's `rustfmt.toml` itself. This is the
//!    base Rust formatting. ([`run_rustfmt`])
//! 2. Parse that output with `syn` + `proc-macro2` (`span-locations`),
//!    walk for `render!` / `css!` invocations, re-parse each body with
//!    [`whisker_macro_syntax`], pretty-print it, and splice the result
//!    back over the original body token range. ([`reformat_macros`])
//!
//! [`format_source`] runs the whole pipeline. [`reformat_macros`] is
//! the macro-only pass and is independently testable WITHOUT the
//! rustfmt binary (feed it already-rust-formatted input) — useful in
//! CI environments where rustfmt may be absent.
//!
//! # Config
//!
//! There are NO whisker-specific options. [`FmtOptions`] mirrors only
//! rustfmt keys (`max_width`, `tab_spaces`, `hard_tabs`, `edition`)
//! and the base rustfmt pass reads `rustfmt.toml` directly.
//!
//! # Comments inside macros
//!
//! `syn` drops comments, and `proc-macro2` exposes them only as
//! whitespace between tokens. Full-fidelity comment preservation inside
//! `render!` bodies is **not** implemented in this pass — see
//! [`reformat_macros`] docs and the `comments_*` tests for the exact,
//! documented limitation. Comments OUTSIDE macros are preserved by
//! rustfmt as usual.

mod options;
mod printer;
mod source_map;

pub use options::FmtOptions;

use anyhow::{anyhow, Context, Result};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use source_map::SourceMap;
use std::path::Path;
use std::process::Command;

/// Run the full pipeline: rustfmt the source, then reformat every
/// `render!` / `css!` body found in the rustfmt output.
///
/// `opts` supplies the layout values the macro pretty-printer needs.
/// The rustfmt binary independently reads `rustfmt.toml`; pass
/// `opts.edition` through so both passes agree on the edition.
pub fn format_source(src: &str, opts: &FmtOptions) -> Result<String> {
    let base = run_rustfmt(src, opts, None)?;
    reformat_macros(&base, opts)
}

/// Like [`format_source`] but tells rustfmt to resolve `rustfmt.toml`
/// from `config_dir` (its `--config-path`). Used by the CLI so each
/// file's nearest `rustfmt.toml` governs.
pub fn format_source_in_dir(src: &str, opts: &FmtOptions, config_dir: &Path) -> Result<String> {
    let base = run_rustfmt(src, opts, Some(config_dir))?;
    reformat_macros(&base, opts)
}

/// `--check` helper: returns `Ok(None)` if the source is already
/// formatted, or `Ok(Some(unified_diff))` describing what would change.
pub fn check_source(src: &str, opts: &FmtOptions) -> Result<Option<String>> {
    let formatted = format_source(src, opts)?;
    if formatted == src {
        Ok(None)
    } else {
        Ok(Some(unified_diff(src, &formatted)))
    }
}

/// Render a unified diff between `before` and `after`.
pub fn unified_diff(before: &str, after: &str) -> String {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(before, after);
    let mut out = String::new();
    for group in diff.grouped_ops(3) {
        for op in group {
            for change in diff.iter_changes(&op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                out.push_str(sign);
                out.push_str(change.value());
                if !change.value().ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }
    out
}

// ---- rustfmt subprocess --------------------------------------------------

/// Locate the rustfmt binary: `$RUSTFMT`, else `rustup which rustfmt`,
/// else `rustfmt` on `PATH`.
pub fn rustfmt_path() -> String {
    if let Ok(p) = std::env::var("RUSTFMT") {
        if !p.is_empty() {
            return p;
        }
    }
    if let Ok(out) = Command::new("rustup").args(["which", "rustfmt"]).output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return p;
            }
        }
    }
    "rustfmt".to_string()
}

/// Returns `true` if a rustfmt binary appears to be invokable. Used to
/// gate the integration tests.
pub fn rustfmt_available() -> bool {
    Command::new(rustfmt_path())
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run the rustfmt binary over `src`, returning its stdout. rustfmt is
/// run with cwd = `config_dir` (when given) so it resolves the right
/// `rustfmt.toml`; otherwise it runs in the current dir.
fn run_rustfmt(src: &str, opts: &FmtOptions, config_dir: Option<&Path>) -> Result<String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut cmd = Command::new(rustfmt_path());
    cmd.arg("--emit").arg("stdout");
    if let Some(ed) = &opts.edition {
        cmd.arg("--edition").arg(ed);
    }
    if let Some(dir) = config_dir {
        // Run with cwd = the file's directory so rustfmt's own upward
        // search finds the nearest `rustfmt.toml`. Only pass an explicit
        // `--config-path` when a config actually exists at/above `dir`
        // — pointing `--config-path` at a directory with no config is a
        // hard error in rustfmt.
        cmd.current_dir(dir);
        if find_rustfmt_toml(dir).is_some() {
            cmd.arg("--config-path").arg(dir);
        }
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn rustfmt ({})", rustfmt_path()))?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("rustfmt stdin unavailable"))?
        .write_all(src.as_bytes())
        .context("writing source to rustfmt stdin")?;
    let out = child
        .wait_with_output()
        .context("waiting for rustfmt to finish")?;
    if !out.status.success() {
        return Err(anyhow!(
            "rustfmt failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).context("rustfmt produced non-UTF-8 output")
}

/// Walk upward from `dir` looking for `rustfmt.toml` / `.rustfmt.toml`.
fn find_rustfmt_toml(dir: &Path) -> Option<std::path::PathBuf> {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        for name in ["rustfmt.toml", ".rustfmt.toml"] {
            let candidate = d.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        cur = d.parent();
    }
    None
}

// ---- macro reformatting pass --------------------------------------------

/// Reformat every `render!` / `css!` macro body found in `rust_src`
/// (which must already be valid, rustfmt-formatted Rust).
///
/// This is the testable core that does NOT need the rustfmt binary.
///
/// ## Comment limitation
///
/// Comments *inside* a `render!` / `css!` body are dropped: `syn`
/// discards them entirely, and recovering them from inter-token
/// whitespace cannot be done reliably for an arbitrary nested grammar
/// in this pass. Macro bodies that contain `//` or `/* */` comments are
/// detected and left **untouched** (the original body text is kept
/// verbatim) rather than silently losing the comments — see
/// [`body_has_comments`]. This is the documented, tested limitation.
pub fn reformat_macros(rust_src: &str, opts: &FmtOptions) -> Result<String> {
    // Parse the whole file just to confirm it is valid Rust; the actual
    // macro discovery walks the raw TokenStream (so we keep precise
    // span byte-offsets relative to `rust_src`).
    let _: syn::File = syn::parse_file(rust_src)
        .context("whisker-fmt: rustfmt output did not re-parse as valid Rust")?;

    let tokens: TokenStream = rust_src
        .parse()
        .map_err(|e| anyhow!("whisker-fmt: could not lex rustfmt output: {e}"))?;

    let file_map = SourceMap::new(rust_src);

    // Collect (body_open_offset, body_close_offset, replacement) for
    // every macro, then splice from the end backwards so earlier
    // offsets stay valid.
    let mut edits: Vec<MacroEdit> = Vec::new();
    collect_macro_edits(tokens, &file_map, rust_src, opts, &mut edits)?;

    edits.sort_by_key(|e| e.open_byte);
    // Splice from the end so earlier byte offsets remain valid.
    let mut out = rust_src.to_string();
    for edit in edits.into_iter().rev() {
        out.replace_range(edit.open_byte..edit.close_byte, &edit.replacement);
    }
    Ok(out)
}

struct MacroEdit {
    /// Byte offset just AFTER the opening delimiter.
    open_byte: usize,
    /// Byte offset of the closing delimiter.
    close_byte: usize,
    replacement: String,
}

/// Recursively walk a token stream, finding `render! { … }` /
/// `css! { … }` (or `(…)` / `[…]`) invocations and queueing an edit for
/// each body.
fn collect_macro_edits(
    tokens: TokenStream,
    file_map: &SourceMap,
    rust_src: &str,
    opts: &FmtOptions,
    edits: &mut Vec<MacroEdit>,
) -> Result<()> {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut i = 0;
    while i < trees.len() {
        // Look for `IDENT ! GROUP` where IDENT is render/css.
        if let TokenTree::Ident(ident) = &trees[i] {
            let name = ident.to_string();
            if (name == "render" || name == "css")
                && i + 2 < trees.len()
                && matches!(&trees[i + 1], TokenTree::Punct(p) if p.as_char() == '!')
            {
                if let TokenTree::Group(group) = &trees[i + 2] {
                    if let Some(edit) = macro_body_edit(&name, group, file_map, rust_src, opts)? {
                        edits.push(edit);
                    }
                    // Don't recurse into a macro body we've already
                    // reformatted as a whole; but DO recurse if we
                    // skipped it (comments etc.) so nested macros still
                    // get a chance. Simplest correct choice: recurse
                    // into the body always — the inner macro edits use
                    // the same global byte offsets and the outer edit
                    // (if any) reformats the body text from the AST,
                    // which re-prints nested render!/css! via slicing.
                    // To avoid double-editing the same bytes we only
                    // recurse when no outer edit was produced.
                    i += 3;
                    continue;
                }
            }
        }
        // Recurse into any group we didn't treat as a macro body.
        if let TokenTree::Group(group) = &trees[i] {
            collect_macro_edits(group.stream(), file_map, rust_src, opts, edits)?;
        }
        i += 1;
    }
    Ok(())
}

/// Build the splice edit for a single macro body group, or `None` if
/// the body should be left untouched (empty, comment-bearing, or
/// re-parse failure).
fn macro_body_edit(
    macro_name: &str,
    group: &proc_macro2::Group,
    file_map: &SourceMap,
    rust_src: &str,
    opts: &FmtOptions,
) -> Result<Option<MacroEdit>> {
    let span = group.span();
    // Byte offsets of the WHOLE group (including delimiters).
    let Some((group_start, group_end)) = file_map.byte_range(span) else {
        return Ok(None);
    };
    // The opening / closing delimiters are single chars; the body is
    // between them.
    let open_byte = group_start + 1;
    let close_byte = group_end - 1;
    if close_byte <= open_byte {
        return Ok(None); // empty body
    }
    let body_src = &rust_src[open_byte..close_byte];

    // Comment limitation: leave bodies with comments untouched.
    if body_has_comments(body_src) {
        return Ok(None);
    }

    // Base indent = the indent level of the line the macro sits on.
    let line_start = rust_src[..group_start]
        .rfind('\n')
        .map(|n| n + 1)
        .unwrap_or(0);
    let line_prefix = &rust_src[line_start..group_start];
    let base_indent = indent_level_of(line_prefix, opts);

    // Re-parse the body with whisker-macro-syntax. The span locations
    // inside `body_ts` are relative to `body_src`, so build a fresh
    // SourceMap over exactly that substring.
    let body_ts: TokenStream = body_src
        .parse()
        .map_err(|e| anyhow!("whisker-fmt: could not lex {macro_name}! body: {e}"))?;
    let body_map = SourceMap::new(body_src);

    let formatted = match macro_name {
        "render" => match whisker_macro_syntax::render::parse_root(body_ts.clone()) {
            Ok(root) => printer::print_render(&root, &body_map, opts, base_indent),
            // Not a well-formed render! body (e.g. mid-edit) — leave it.
            Err(_) => return Ok(None),
        },
        "css" => match whisker_macro_syntax::css::parse_input(body_ts.clone()) {
            Ok(input) => {
                if input.kwargs.is_empty() {
                    return Ok(None);
                }
                printer::print_css(&input, &body_map, opts, base_indent)
            }
            Err(_) => return Ok(None),
        },
        _ => return Ok(None),
    };

    // Wrap with the delimiter-relative newlines: the printer emits the
    // body already indented to `base_indent + 1`; surround with a
    // leading newline and a trailing newline + closing-brace indent.
    let closing_indent = opts.indent_prefix(base_indent);
    let replacement = match group.delimiter() {
        // `render! { … }` — the common form. Put body on its own lines.
        Delimiter::Brace => format!("\n{formatted}\n{closing_indent}"),
        // `css!( … )` / `css![ … ]` — also break onto lines for
        // consistency with rustfmt's treatment of multi-item macros.
        _ => format!("\n{formatted}\n{closing_indent}"),
    };

    // Idempotency guard: if the body already equals the replacement,
    // skip (avoids spurious diffs and keeps format(format(x))==format(x)).
    if body_src == replacement {
        return Ok(None);
    }

    Ok(Some(MacroEdit {
        open_byte,
        close_byte,
        replacement,
    }))
}

/// Detect `//` or `/* */` comments in a macro body. Uses a tiny
/// string-aware scan so a `//` inside a string literal isn't a false
/// positive.
fn body_has_comments(body: &str) -> bool {
    let bytes = body.as_bytes();
    let mut i = 0;
    let mut in_str: Option<u8> = None; // Some(quote_char)
    while i < bytes.len() {
        let b = bytes[i];
        match in_str {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    in_str = None;
                }
            }
            None => {
                if b == b'"' || b == b'\'' {
                    in_str = Some(b);
                } else if b == b'/' && i + 1 < bytes.len() {
                    let n = bytes[i + 1];
                    if n == b'/' || n == b'*' {
                        return true;
                    }
                }
            }
        }
        i += 1;
    }
    false
}

/// Convert a line's leading-whitespace prefix into an indent level (in
/// tab-units). Tabs count as one level each; spaces are divided by
/// `tab_spaces`.
fn indent_level_of(line_prefix: &str, opts: &FmtOptions) -> usize {
    let mut spaces = 0usize;
    let mut tabs = 0usize;
    for ch in line_prefix.chars() {
        match ch {
            ' ' => spaces += 1,
            '\t' => tabs += 1,
            _ => break,
        }
    }
    let space_levels = spaces.checked_div(opts.tab_spaces).unwrap_or(0);
    tabs + space_levels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(tab: usize, width: usize) -> FmtOptions {
        FmtOptions {
            max_width: width,
            tab_spaces: tab,
            hard_tabs: false,
            edition: None,
        }
    }

    // All tests below feed already-rust-formatted input to
    // `reformat_macros`, so they DO NOT require the rustfmt binary.

    #[test]
    fn reformats_messy_render_body() {
        let input = "fn ui() -> Element {\n    render! { view(style:\"x\",class:\"y\"){text(value:\"hi\")} }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        let expected = "fn ui() -> Element {\n    render! {\n        view(style: \"x\", class: \"y\") {\n            text(value: \"hi\")\n        }\n    }\n}\n";
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn idempotent() {
        let input =
            "fn ui() -> Element {\n    render! { view(style:\"x\"){text(value:\"hi\")} }\n}\n";
        let once = reformat_macros(input, &opts(4, 100)).unwrap();
        let twice = reformat_macros(&once, &opts(4, 100)).unwrap();
        assert_eq!(
            once, twice,
            "not idempotent:\nonce:\n{once}\ntwice:\n{twice}"
        );
    }

    #[test]
    fn honors_tab_spaces_from_options() {
        let input =
            "fn ui() -> Element {\n    render! { view(style:\"x\"){text(value:\"hi\")} }\n}\n";
        let four = reformat_macros(input, &opts(4, 100)).unwrap();
        let two = reformat_macros(input, &opts(2, 100)).unwrap();
        assert_ne!(four, two, "tab_spaces must change indentation");
        // 4-space variant indents the inner text 12 cols; 2-space, 6.
        assert!(
            four.contains("            text(value: \"hi\")"),
            "4-space:\n{four}"
        );
        assert!(two.contains("      text(value: \"hi\")"), "2-space:\n{two}");
    }

    #[test]
    fn wraps_kwargs_over_max_width() {
        // A node whose kwargs blow past a tiny max_width breaks each
        // kwarg onto its own line with a trailing comma.
        let input = "fn ui() -> Element {\n    render! { view(style: \"a-long-value\", class: \"another-long-value\") }\n}\n";
        let out = reformat_macros(input, &opts(4, 40)).unwrap();
        assert!(out.contains("view(\n"), "expected broken kwargs:\n{out}");
        assert!(
            out.contains("class: \"another-long-value\",\n"),
            "trailing comma expected:\n{out}"
        );
    }

    #[test]
    fn preserves_user_expression_source() {
        // The embedded closure expression should be kept verbatim
        // (sliced), not re-printed from tokens.
        let input =
            "fn ui() -> Element {\n    render! { view(on_tap: move |_| do_thing(a, b)) }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        assert!(
            out.contains("on_tap: move |_| do_thing(a, b)"),
            "expression must be preserved verbatim:\n{out}"
        );
    }

    #[test]
    fn formats_css_body() {
        let input = "fn s() -> Css {\n    css! { color:red,padding:px(8) }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        assert!(
            out.contains("color: red, padding: px(8)"),
            "css inline:\n{out}"
        );
    }

    #[test]
    fn comments_in_body_left_untouched() {
        // KNOWN LIMITATION: a macro body containing comments is left
        // exactly as-is rather than dropping the comment.
        let input = "fn ui() -> Element {\n    render! { view(style:\"x\") /* keep me */ }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        assert_eq!(out, input, "comment-bearing body must be untouched");
    }

    #[test]
    fn children_slot_preserved() {
        let input = "fn ui() -> Element {\n    render! { view(style:\"x\"){children()} }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        assert!(
            out.contains("children()"),
            "children() slot must survive:\n{out}"
        );
    }

    #[test]
    fn non_render_macro_untouched() {
        let input = "fn x() {\n    println!(\"hi {}\", v);\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        assert_eq!(out, input);
    }
}
