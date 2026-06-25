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
//! `syn` drops comments and `proc-macro2` exposes them only as
//! whitespace between tokens, so reprinting a `render!` / `css!` body
//! from the parsed AST would lose them. To preserve them we recover the
//! comments straight from the body source text ([`comments`]) and
//! reattach them while pretty-printing ([`printer`]): own-line comments
//! go on their own line at the block's indent, trailing comments are
//! appended to the end of the preceding line. Comments INSIDE an embedded
//! expr value are excluded from this pass — they ride along with the
//! verbatim / rustfmt-formatted expr source instead.
//!
//! A **fail-safe** guards the result: after formatting, if any recovered
//! comment would be dropped, or the output is not idempotent
//! (`f(f(x)) != f(x)`), the body is left **untouched** (the original
//! verbatim text) — so a comment can never be silently lost. See
//! [`macro_body_edit`]. Comments OUTSIDE macros are preserved by rustfmt
//! as usual.

mod comments;
mod expr_fmt;
mod options;
mod printer;
mod source_map;

pub use options::FmtOptions;

use anyhow::{Context, Result, anyhow};
use expr_fmt::{ExprFormatter, ExprMap};
use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
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
    let exprfmt = ExprFormatter::new(opts);
    reformat_macros_inner(&base, opts, Some(&exprfmt))
}

/// Like [`format_source`] but tells rustfmt to resolve `rustfmt.toml`
/// from `config_dir` (its `--config-path`). Used by the CLI so each
/// file's nearest `rustfmt.toml` governs.
pub fn format_source_in_dir(src: &str, opts: &FmtOptions, config_dir: &Path) -> Result<String> {
    let base = run_rustfmt(src, opts, Some(config_dir))?;
    let exprfmt = ExprFormatter::new_in_dir(opts, config_dir);
    reformat_macros_inner(&base, opts, Some(&exprfmt))
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
        // `--config-path` when a config actually exists at/above `dir`,
        // and point it at the ACTUAL config file `find_rustfmt_toml`
        // returned — NOT at `dir`. The config may live at a PARENT (e.g.
        // the project root) while `dir` is a nested subdir with no
        // rustfmt.toml of its own; `--config-path <dir>` would then make
        // rustfmt error "unable to find a config file for the given
        // path". Passing the found file path is deterministic and always
        // resolves.
        cmd.current_dir(dir);
        if let Some(toml_path) = find_rustfmt_toml(dir) {
            cmd.arg("--config-path").arg(&toml_path);
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
pub fn find_rustfmt_toml(dir: &Path) -> Option<std::path::PathBuf> {
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

// ---- edition resolution (mirrors `cargo fmt`) ----------------------------

/// The edition assumed when neither `rustfmt.toml` nor any `Cargo.toml`
/// up the tree declares one. rustfmt's *own* default is 2015, which
/// rejects 2018+ syntax (`async move`, etc.); we pick a modern default
/// so `whisker fmt` never falls into the 2015 trap.
const DEFAULT_EDITION: &str = "2021";

/// Walk upward from `dir` looking for the nearest `Cargo.toml`.
pub fn find_cargo_toml(dir: &Path) -> Option<std::path::PathBuf> {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        let candidate = d.join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        cur = d.parent();
    }
    None
}

/// Read the edition declared by the nearest `Cargo.toml` at or above
/// `dir`. Honors `[package] edition` first, then
/// `[workspace.package] edition` (the inherited-edition form used by
/// `edition.workspace = true`). Returns `None` if no `Cargo.toml` is
/// found, it can't be read/parsed, or it declares no edition.
pub fn cargo_toml_edition(dir: &Path) -> Option<String> {
    let path = find_cargo_toml(dir)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    edition_from_cargo_value(&value)
}

/// Extract the edition string from a parsed `Cargo.toml` value, checking
/// `[package] edition` then `[workspace.package] edition`.
fn edition_from_cargo_value(value: &toml::Value) -> Option<String> {
    let as_str = |v: &toml::Value| v.as_str().map(str::to_string);
    value
        .get("package")
        .and_then(|p| p.get("edition"))
        .and_then(as_str)
        .or_else(|| {
            value
                .get("workspace")
                .and_then(|w| w.get("package"))
                .and_then(|p| p.get("edition"))
                .and_then(as_str)
        })
}

/// Resolve the full set of [`FmtOptions`] for a real directory, mirroring
/// how `cargo fmt` injects each crate's edition into rustfmt.
///
/// Edition resolution order:
/// 1. The nearest `rustfmt.toml`'s `edition` key, if present (wins).
/// 2. else the nearest `Cargo.toml`'s edition (`[package]` or
///    `[workspace.package]`), searching upward from `dir`.
/// 3. else [`DEFAULT_EDITION`] (`"2021"`).
///
/// The non-edition layout keys (`max_width`, `tab_spaces`, `hard_tabs`)
/// come from the same `rustfmt.toml`. The returned `edition` is ALWAYS
/// `Some`, so both the base rustfmt pass and the embedded-expr pass pass
/// `--edition` to rustfmt and never fall back to its 2015 default.
pub fn resolve_options(dir: &Path) -> FmtOptions {
    let mut opts = match find_rustfmt_toml(dir) {
        Some(toml_path) => std::fs::read_to_string(&toml_path)
            .map(|text| FmtOptions::from_rustfmt_config(&text))
            .unwrap_or_default(),
        None => FmtOptions::default(),
    };

    // rustfmt.toml edition wins; otherwise fall back to Cargo.toml, then
    // the modern default. Never leave `edition` as `None` (2015).
    if opts.edition.is_none() {
        opts.edition = Some(cargo_toml_edition(dir).unwrap_or_else(|| DEFAULT_EDITION.to_string()));
    }

    opts
}

// ---- macro reformatting pass --------------------------------------------

/// Reformat every `render!` / `css!` macro body found in `rust_src`
/// (which must already be valid, rustfmt-formatted Rust).
///
/// This is the testable core that does NOT need the rustfmt binary.
///
/// ## Comments
///
/// Comments inside a `render!` / `css!` body ARE preserved: they're
/// recovered from the body source ([`comments::collect_grammar_comments`])
/// and reattached during pretty-printing. A fail-safe in
/// [`macro_body_edit`] falls back to leaving the body untouched if any
/// comment would be dropped or the result is not idempotent, so comments
/// are never lost.
pub fn reformat_macros(rust_src: &str, opts: &FmtOptions) -> Result<String> {
    // Public entry point: the rustfmt-FREE core. No `ExprFormatter`, so
    // every embedded expr is rendered verbatim (the printer's empty-map
    // path). This is what keeps the 17 unit tests in this file passing
    // without a rustfmt binary.
    reformat_macros_inner(rust_src, opts, None)
}

/// The shared implementation behind [`reformat_macros`] and the full
/// pipeline. When `exprfmt` is `Some`, embedded exprs are formatted by
/// the real rustfmt (batched per macro body); when `None`, they are kept
/// verbatim.
fn reformat_macros_inner(
    rust_src: &str,
    opts: &FmtOptions,
    exprfmt: Option<&ExprFormatter>,
) -> Result<String> {
    reformat_macros_pass(rust_src, opts, exprfmt, true)
}

/// One macro-reformatting pass. `verify` enables the comment-preservation
/// fail-safe (present-check + idempotency). The idempotency check re-runs
/// this pass with `verify = false` so the guard does NOT recurse into
/// itself (which would be unbounded for nested / large bodies).
fn reformat_macros_pass(
    rust_src: &str,
    opts: &FmtOptions,
    exprfmt: Option<&ExprFormatter>,
    verify: bool,
) -> Result<String> {
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
    collect_macro_edits(
        tokens, &file_map, rust_src, opts, exprfmt, verify, &mut edits,
    )?;

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
#[allow(clippy::too_many_arguments)]
fn collect_macro_edits(
    tokens: TokenStream,
    file_map: &SourceMap,
    rust_src: &str,
    opts: &FmtOptions,
    exprfmt: Option<&ExprFormatter>,
    verify: bool,
    edits: &mut Vec<MacroEdit>,
) -> Result<()> {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut i = 0;
    while i < trees.len() {
        // Look for `IDENT ! GROUP` where IDENT is render/css.
        if let TokenTree::Ident(ident) = &trees[i] {
            let name = ident.to_string();
            if (name == "render" || name == "css" || name == "routes")
                && i + 2 < trees.len()
                && matches!(&trees[i + 1], TokenTree::Punct(p) if p.as_char() == '!')
            {
                if let TokenTree::Group(group) = &trees[i + 2] {
                    if let Some(edit) =
                        macro_body_edit(&name, group, file_map, rust_src, opts, exprfmt, verify)?
                    {
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
            collect_macro_edits(
                group.stream(),
                file_map,
                rust_src,
                opts,
                exprfmt,
                verify,
                edits,
            )?;
        }
        i += 1;
    }
    Ok(())
}

/// Build the splice edit for a single macro body group, or `None` if
/// the body should be left untouched (empty, comment-bearing, or
/// re-parse failure).
#[allow(clippy::too_many_arguments)]
fn macro_body_edit(
    macro_name: &str,
    group: &proc_macro2::Group,
    file_map: &SourceMap,
    rust_src: &str,
    opts: &FmtOptions,
    exprfmt: Option<&ExprFormatter>,
    verify: bool,
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

    let body_len = body_src.len();

    // Collect the embedded-expr spans up front: we need them both to
    // batch-format the exprs AND to mask out expr-internal comments when
    // recovering the grammar comments to reattach. Comments INSIDE an
    // expr value are kept by slicing / rustfmt-ing the expr source; only
    // GRAMMAR comments are reattached here.
    let (formatted, grammar_comments) = match macro_name {
        "render" => match whisker_macro_syntax::render::parse_root(body_ts.clone()) {
            Ok(root) => {
                let mut spans = Vec::new();
                collect_render_expr_spans(&root.node, &mut spans);
                let comments = comments::collect_grammar_comments(body_src, &spans, &body_map);
                // Batch-format every embedded expr with one rustfmt spawn.
                // With no `exprfmt` (rustfmt-free core) the map stays
                // empty and every expr renders verbatim.
                let expr_map = build_expr_map(&spans, &body_map, exprfmt);
                let s = printer::print_render(
                    &root,
                    &body_map,
                    opts,
                    base_indent,
                    &expr_map,
                    &comments,
                    body_len,
                );
                (s, comments)
            }
            // Not a well-formed render! body (e.g. mid-edit) — leave it.
            Err(_) => return Ok(None),
        },
        "routes" => match whisker_macro_syntax::routes::parse_input(body_ts.clone()) {
            Ok(input) => {
                if input.roots.is_empty() {
                    return Ok(None);
                }
                let mut spans = Vec::new();
                collect_routes_expr_spans(&input.roots, &mut spans);
                let comments = comments::collect_grammar_comments(body_src, &spans, &body_map);
                let expr_map = build_expr_map(&spans, &body_map, exprfmt);
                let s = printer::print_routes(
                    &input,
                    &body_map,
                    opts,
                    base_indent,
                    &expr_map,
                    &comments,
                    body_len,
                );
                (s, comments)
            }
            Err(_) => return Ok(None),
        },
        "css" => match whisker_macro_syntax::css::parse_input(body_ts.clone()) {
            Ok(input) => {
                if input.kwargs.is_empty() {
                    return Ok(None);
                }
                let mut spans = Vec::new();
                for kw in &input.kwargs {
                    if let Some(expr) = &kw.value {
                        spans.push(span_of_expr(expr));
                    }
                }
                let comments = comments::collect_grammar_comments(body_src, &spans, &body_map);
                let expr_map = build_expr_map(&spans, &body_map, exprfmt);
                let s = printer::print_css(
                    &input,
                    &body_map,
                    opts,
                    base_indent,
                    &expr_map,
                    &comments,
                    body_len,
                );
                (s, comments)
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

    // ---- comment-preservation fail-safe ------------------------------
    //
    // If reattaching the comments could possibly have dropped one, or the
    // result isn't a fixed point, leave the body UNTOUCHED (today's old
    // behavior — no regression, no comment ever lost).
    if verify && !grammar_comments.is_empty() {
        // (1) No comment lost: every recovered comment's text must appear
        // in the formatted output, counting duplicates.
        if !all_comments_present(&replacement, &grammar_comments) {
            return Ok(None);
        }
        // (2) Idempotency: re-running the formatter on the produced body
        // must be a fixed point. We re-run the SAME macro pass over a
        // minimal synthetic wrapper containing the produced body and check
        // the body comes back unchanged.
        if !macro_replacement_is_fixed_point(macro_name, &replacement, base_indent, opts) {
            return Ok(None);
        }
    }

    Ok(Some(MacroEdit {
        open_byte,
        close_byte,
        replacement,
    }))
}

// ---- comment-preservation fail-safe helpers -----------------------------

/// Every recovered comment's (trimmed) text must appear in `output`,
/// counting duplicates: if the body has two identical comments, both must
/// survive. Uses a per-text occurrence count.
fn all_comments_present(output: &str, comments: &[comments::GrammarComment]) -> bool {
    use std::collections::HashMap;
    // Required count per comment text.
    let mut need: HashMap<&str, usize> = HashMap::new();
    for c in comments {
        *need.entry(c.text.trim()).or_insert(0) += 1;
    }
    for (text, count) in need {
        if text.is_empty() {
            continue;
        }
        if count_occurrences(output, text) < count {
            return false;
        }
    }
    true
}

/// Count non-overlapping occurrences of `needle` in `haystack`.
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut n = 0;
    let mut rest = haystack;
    while let Some(pos) = rest.find(needle) {
        n += 1;
        rest = &rest[pos + needle.len()..];
    }
    n
}

/// Re-run the macro pass over the just-produced body and confirm it is a
/// fixed point (`f(f(x)) == f(x)`). The `replacement` is the body text
/// INCLUDING its leading/trailing newlines (i.e. exactly what sits between
/// the macro delimiters). We splice it into a synthetic wrapper at the
/// right `base_indent`, run the rustfmt-FREE macro pass, and check the
/// macro body comes back identical.
fn macro_replacement_is_fixed_point(
    macro_name: &str,
    replacement: &str,
    base_indent: usize,
    opts: &FmtOptions,
) -> bool {
    let indent = opts.indent_prefix(base_indent);
    // Wrap in a trivial fn so the source is valid Rust. The macro sits at
    // `base_indent` (the wrapper body is at base_indent, the fn at 0).
    // To get `base_indent` levels of indent for the macro line we open
    // (base_indent) nested-block-free wrapper: a single fn plus manual
    // indent prefix on the macro line works because rustfmt isn't run.
    let src = format!("fn _w() {{\n{indent}{macro_name}! {{{replacement}}}\n}}\n");
    // Re-run WITHOUT the verify guard (`verify = false`) so this check does
    // not recurse into itself — otherwise each fixed-point check would
    // spawn another, blowing the stack on large / nested bodies.
    let once = match reformat_macros_pass(&src, opts, None, false) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let twice = match reformat_macros_pass(&once, opts, None, false) {
        Ok(s) => s,
        Err(_) => return false,
    };
    once == twice
}

// ---- embedded-expr collection + batched formatting ----------------------

/// The `Span` covering an `Expr` (start of first token to end of last).
fn span_of_expr(expr: &syn::Expr) -> Span {
    use syn::spanned::Spanned;
    expr.span()
}

/// Walk a parsed `render!` node tree collecting the span of every
/// embedded expr (kwarg values). `partial` kwargs hold a synthesized
/// placeholder with no real source span, so they're skipped. `css!`
/// kwargs are collected separately at the call site (different type).
fn collect_render_expr_spans(node: &whisker_macro_syntax::Node, out: &mut Vec<Span>) {
    use whisker_macro_syntax::Node;
    match node {
        Node::Element(el) => {
            for kw in &el.kwargs {
                if !kw.partial {
                    out.push(span_of_expr(&kw.value));
                }
            }
            for child in &el.children {
                collect_render_expr_spans(child, out);
            }
        }
        Node::UserComponent(uc) => {
            for kw in &uc.kwargs {
                if !kw.partial {
                    out.push(span_of_expr(&kw.value));
                }
            }
            for child in &uc.children {
                collect_render_expr_spans(child, out);
            }
        }
        Node::ChildrenSlot { .. } => {}
    }
}

/// Walk a parsed `routes!` node tree collecting the span of every
/// embedded expr (transition values, spread expressions).
fn collect_routes_expr_spans(nodes: &[whisker_macro_syntax::RoutesNode], out: &mut Vec<Span>) {
    use whisker_macro_syntax::RoutesNode;
    for node in nodes {
        match node {
            RoutesNode::Route {
                transition,
                children,
                ..
            } => {
                if let Some(expr) = transition {
                    out.push(span_of_expr(expr));
                }
                collect_routes_expr_spans(children, out);
            }
            RoutesNode::Switch { children, .. } | RoutesNode::Stack { children, .. } => {
                collect_routes_expr_spans(children, out);
            }
            RoutesNode::Spread(expr) => {
                out.push(span_of_expr(expr));
            }
            RoutesNode::Unknown(_) => {}
        }
    }
}

/// Slice each expr's verbatim source from `body_map` and batch-format
/// the whole set with one rustfmt spawn (via `exprfmt`). Returns an
/// [`ExprMap`] keyed by span. When `exprfmt` is `None` (rustfmt-free
/// core) the returned map is empty, so the printer renders verbatim.
///
/// Spans whose source slice fails to resolve are skipped (they'll hit
/// the printer's verbatim / token fallback anyway).
fn build_expr_map(
    spans: &[Span],
    body_map: &SourceMap,
    exprfmt: Option<&ExprFormatter>,
) -> ExprMap {
    let Some(exprfmt) = exprfmt else {
        return ExprMap::default();
    };
    let mut exprs: Vec<(Span, String)> = Vec::with_capacity(spans.len());
    for &span in spans {
        if let Some(slice) = body_map.slice(span) {
            exprs.push((span, slice.trim().to_string()));
        }
    }
    exprfmt.format_body(&exprs)
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
    fn trailing_block_comment_preserved_and_reflowed() {
        // A trailing block comment after a node is now KEPT (reattached to
        // the node's line) while the body is reflowed.
        let input = "fn ui() -> Element {\n    render! { view(style:\"x\") /* keep me */ }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        let expected = "fn ui() -> Element {\n    render! {\n        view(style: \"x\") /* keep me */\n    }\n}\n";
        assert_eq!(out, expected, "got:\n{out}");
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

    // ---- routes! ----------------------------------------------------------

    #[test]
    fn formats_routes_simple_stack() {
        let input = "fn r() -> Routes {\n    routes! { Stack{Route(path:\"a\",component:A)Route(path:\"b\",component:B)} }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        let expected = "fn r() -> Routes {\n    routes! {\n        Stack {\n            Route(path: \"a\", component: A)\n            Route(path: \"b\", component: B)\n        }\n    }\n}\n";
        assert_eq!(out, expected, "got:\n{out}");
    }

    #[test]
    fn formats_routes_nested_switch() {
        let input = "fn r() -> Routes {\n    routes! { Switch{Route(path:\"(home)\"){Stack{Route(path:\"\",component:Home)Route(path:\"detail/:id\",component:Detail)}}Route(path:\"(search)\"){Stack{Route(path:\"list\",component:List)}}} }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        assert!(
            out.contains("Switch {\n"),
            "Switch should have newline:\n{out}"
        );
        assert!(
            out.contains("Route(path: \"(home)\") {\n"),
            "group route:\n{out}"
        );
        assert!(
            out.contains("Route(path: \"\", component: Home)\n"),
            "leaf route:\n{out}"
        );
    }

    #[test]
    fn formats_routes_with_spread() {
        let input =
            "fn r() -> Routes {\n    routes! { Stack{Route(path:\"a\",component:A)..frag} }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        assert!(out.contains("..frag"), "spread preserved:\n{out}");
    }

    #[test]
    fn routes_idempotent() {
        let input = "fn r() -> Routes {\n    routes! { Stack{Route(path:\"a\",component:A)Route(path:\"b\",component:B)} }\n}\n";
        let once = reformat_macros(input, &opts(4, 100)).unwrap();
        let twice = reformat_macros(&once, &opts(4, 100)).unwrap();
        assert_eq!(
            once, twice,
            "not idempotent:\nonce:\n{once}\ntwice:\n{twice}"
        );
    }

    #[test]
    fn routes_leaf_route_no_braces() {
        let input = "fn r() -> Routes {\n    routes! { Route(path:\"x\",component:X) }\n}\n";
        let out = reformat_macros(input, &opts(4, 100)).unwrap();
        assert!(
            !out.contains("Route(path: \"x\", component: X) {"),
            "leaf should not get braces:\n{out}"
        );
        assert!(
            out.contains("Route(path: \"x\", component: X)"),
            "leaf format:\n{out}"
        );
    }

    // ---- edition resolution ----------------------------------------------

    /// Create a unique temp dir for an edition-resolution test, isolated
    /// from the repo's own `Cargo.toml` / `rustfmt.toml` (those live well
    /// above `temp_dir()` so the upward walk never reaches them).
    fn unique_tmp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "whisker-fmt-ed-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn rustfmt_toml_edition_wins_over_cargo_toml() {
        let tmp = unique_tmp("rustfmt-wins");
        std::fs::write(tmp.join("rustfmt.toml"), "edition = \"2018\"\n").unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"x\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let o = resolve_options(&tmp);
        assert_eq!(o.edition.as_deref(), Some("2018"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn cargo_toml_package_edition_used_without_rustfmt_edition() {
        // No rustfmt.toml at all, but a Cargo.toml up the tree.
        let tmp = unique_tmp("cargo-pkg");
        let sub = tmp.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"x\"\nedition = \"2018\"\n",
        )
        .unwrap();
        let o = resolve_options(&sub);
        assert_eq!(o.edition.as_deref(), Some("2018"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn workspace_package_edition_detected() {
        let tmp = unique_tmp("ws-pkg");
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = []\n[workspace.package]\nedition = \"2021\"\n",
        )
        .unwrap();
        let o = resolve_options(&tmp);
        assert_eq!(o.edition.as_deref(), Some("2021"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn defaults_to_2021_without_any_config() {
        // Neither rustfmt.toml nor Cargo.toml anywhere in the (temp) tree.
        let tmp = unique_tmp("none");
        let o = resolve_options(&tmp);
        assert_eq!(o.edition.as_deref(), Some("2021"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn resolved_edition_is_always_some() {
        let tmp = unique_tmp("always-some");
        // rustfmt.toml present but WITHOUT an edition key → must still
        // fall through to Cargo.toml / default, never None.
        std::fs::write(tmp.join("rustfmt.toml"), "tab_spaces = 2\n").unwrap();
        let o = resolve_options(&tmp);
        assert_eq!(o.tab_spaces, 2);
        assert!(o.edition.is_some(), "edition must never resolve to None");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// Integration: an `async move { … }` snippet — which rustfmt rejects
    /// under its 2015 default — formats SUCCESSFULLY when the resolved
    /// edition comes from a temp `Cargo.toml` with `edition = "2021"` and
    /// NO rustfmt.toml is present. Reproduces-then-verifies the reported
    /// failure. Gated on a real rustfmt binary.
    #[test]
    fn async_move_formats_with_cargo_edition_no_rustfmt_toml() {
        if !rustfmt_available() {
            return;
        }
        let tmp = unique_tmp("async-move");
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"x\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let opts = resolve_options(&tmp);
        assert_eq!(opts.edition.as_deref(), Some("2021"));
        let src = "fn f() {\n    let x = async move { 1 };\n}\n";
        let out = format_source_in_dir(src, &opts, &tmp)
            .expect("async move must format under the resolved 2021 edition");
        assert!(out.contains("async move"), "got:\n{out}");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// Regression: a `rustfmt.toml` at the project ROOT must govern even
    /// when formatting from a NESTED subdir that has no rustfmt.toml of
    /// its own. Before the fix, `run_rustfmt` passed `--config-path
    /// <subdir>` and rustfmt errored "unable to find a config file for
    /// the given path"; now it passes the found config file path.
    #[test]
    fn root_rustfmt_toml_governs_nested_subdir() {
        if !rustfmt_available() {
            return;
        }
        let root = unique_tmp("nested-config");
        // 2-space indent at the root, no rustfmt.toml in the subdir.
        std::fs::write(root.join("rustfmt.toml"), "tab_spaces = 2\n").unwrap();
        let nested = root.join("src").join("screens");
        std::fs::create_dir_all(&nested).unwrap();

        let opts = resolve_options(&nested);
        // Resolver picked up the root rustfmt.toml's tab_spaces.
        assert_eq!(opts.tab_spaces, 2);

        let src = "fn f() {\nlet x = 1;\n}\n";
        let out = format_source_in_dir(src, &opts, &nested)
            .expect("root rustfmt.toml must govern a nested subdir");
        // rustfmt applied 2-space indent from the root config.
        assert!(out.contains("\n  let x = 1;"), "got:\n{out}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}

// ---- comment-preservation tests -----------------------------------------
//
// All feed already-rust-formatted input to `reformat_macros`, so they do
// NOT need the rustfmt binary. They assert EXACT output to prove real
// comment-preserving formatting happens (not just fallback).
#[cfg(test)]
mod comment_tests {
    use super::*;

    fn o() -> FmtOptions {
        FmtOptions {
            max_width: 100,
            tab_spaces: 4,
            hard_tabs: false,
            edition: None,
        }
    }

    fn fmt(input: &str) -> String {
        reformat_macros(input, &o()).unwrap()
    }

    // 1. Own-line `//` before a top-level element (also reflows the body).
    #[test]
    fn own_line_before_top_element() {
        let input = "fn d() -> Element {\n    render! {\n        // header\n        view(style: \"x\") { text(value: \"hi\") }\n    }\n}\n";
        let expected = "fn d() -> Element {\n    render! {\n        // header\n        view(style: \"x\") {\n            text(value: \"hi\")\n        }\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    // 2. Own-line comment between two sibling children.
    #[test]
    fn own_line_between_siblings() {
        let input = "fn d() -> Element {\n    render! {\n        view {\n            text(value: \"a\")\n            // mid\n            text(value: \"b\")\n        }\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 3. Own-line comment as the first child (right after `{`).
    #[test]
    fn own_line_first_child() {
        let input = "fn d() -> Element {\n    render! {\n        view {\n            // first\n            text(value: \"a\")\n        }\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 4. Own-line comment after the last child, before `}` (inside block).
    #[test]
    fn own_line_after_last_child() {
        let input = "fn d() -> Element {\n    render! {\n        view {\n            text(value: \"a\")\n            // last\n        }\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 5. Trailing `//` on the same line as a child.
    #[test]
    fn trailing_line_comment_on_child() {
        let input = "fn d() -> Element {\n    render! {\n        view {\n            text(value: \"a\") // tail\n        }\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 6. Trailing comment after a `css!` field.
    #[test]
    fn trailing_comment_after_css_field() {
        let input = "fn s() -> Css {\n    css! {\n        color: red, // c\n        padding: px(8),\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 7. Own-line comment between two `css!` fields.
    #[test]
    fn own_line_between_css_fields() {
        let input = "fn s() -> Css {\n    css! {\n        color: red,\n        // gap\n        padding: px(8),\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 8. `/* block */` single-line comment.
    #[test]
    fn block_comment_single_line() {
        let input = "fn d() -> Element {\n    render! {\n        /* b */\n        view(style: \"x\")\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 9. Multi-line `/* … */` block comment, kept verbatim.
    #[test]
    fn block_comment_multiline_verbatim() {
        let input = "fn d() -> Element {\n    render! {\n        /* line1\n           line2 */\n        view(style: \"x\")\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 10. Two consecutive own-line comments.
    #[test]
    fn two_consecutive_comments() {
        let input = "fn d() -> Element {\n    render! {\n        // one\n        // two\n        view(style: \"x\")\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 11. A comment INSIDE an embedded expr is not touched by
    // reattachment and survives (handled by the expr path).
    #[test]
    fn comment_inside_embedded_expr_survives() {
        let input = "fn d() -> Element {\n    render! {\n        view(on_tap: move |_| { /* keep */ go() })\n    }\n}\n";
        let out = fmt(input);
        assert!(out.contains("/* keep */"), "got:\n{out}");
        // Not duplicated.
        assert_eq!(out.matches("/* keep */").count(), 1, "got:\n{out}");
    }

    // 12. Deeply nested element: own-line comment before an inner child
    // lands at the correct (deeper) indent.
    #[test]
    fn nested_inner_child_indent() {
        let input = "fn d() -> Element {\n    render! {\n        view {\n            scroll_view {\n                // inner\n                text(value: \"a\")\n            }\n        }\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 13. Mixed leading + trailing comments in the same body.
    #[test]
    fn mixed_leading_and_trailing() {
        let input = "fn d() -> Element {\n    render! {\n        // lead\n        view {\n            text(value: \"a\") // tail\n        }\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 14. Idempotency over several comment-bearing inputs.
    #[test]
    fn idempotent_with_comments() {
        let inputs = [
            "fn d() -> Element {\n    render! {\n        // header\n        view(style: \"x\") { text(value: \"hi\") }\n    }\n}\n",
            "fn d() -> Element {\n    render! {\n        view {\n            text(value: \"a\")\n            // mid\n            text(value: \"b\")\n        }\n    }\n}\n",
            "fn s() -> Css {\n    css! {\n        color: red, // c\n        padding: px(8),\n    }\n}\n",
            "fn d() -> Element {\n    render! {\n        /* line1\n           line2 */\n        view(style: \"x\")\n    }\n}\n",
        ];
        for input in inputs {
            let once = fmt(input);
            let twice = fmt(&once);
            assert_eq!(once, twice, "not idempotent for:\n{input}\nonce:\n{once}");
        }
    }

    // 15a. Fallback safety: directly exercise the `all_comments_present`
    // guard — a dropped duplicate makes it report "not present".
    #[test]
    fn fallback_guard_detects_dropped_comment() {
        let comments = vec![
            crate::comments::GrammarComment {
                start: 0,
                end: 5,
                text: "// hi".to_string(),
                own_line: true,
            },
            crate::comments::GrammarComment {
                start: 6,
                end: 11,
                text: "// hi".to_string(),
                own_line: true,
            },
        ];
        // Output with only ONE `// hi` must fail the duplicate-aware check.
        assert!(!all_comments_present("only one // hi here", &comments));
        // Output with BOTH passes.
        assert!(all_comments_present("// hi and // hi", &comments));
    }

    // 15b. Fallback safety end-to-end: even a comment in an awkward spot
    // (between a tag and its parens) is never lost — either reflowed or
    // left untouched, but always present.
    #[test]
    fn fallback_keeps_comment_when_in_doubt() {
        let input =
            "fn d() -> Element {\n    render! {\n        view /* odd */ (style: \"x\")\n    }\n}\n";
        let out = fmt(input);
        assert!(out.contains("/* odd */"), "comment must survive:\n{out}");
    }

    // 16. Wallet-style section comments kept on their own lines.
    #[test]
    fn wallet_style_section_comments() {
        let input = "fn d() -> Element {\n    render! {\n        view {\n            // \u{2500}\u{2500} Header \u{2500}\u{2500}\n            text(value: \"hi\")\n            // \u{2500}\u{2500} Body \u{2500}\u{2500}\n            text(value: \"yo\")\n        }\n    }\n}\n";
        assert_eq!(fmt(input), input);
    }

    // 17. Wallet-faithful reduction: a `render!` with section comments,
    // irregular OVER-INDENTATION (page→view migration leftovers), and a
    // user-component call whose closing `)` sits on its own line at a weird
    // column. This is the exact shape that previously fell back (the body
    // got NO formatting). It must now actually reflow — comments kept on
    // their own lines at the corrected indent — AND be idempotent.
    #[test]
    fn wallet_faithful_reduction_formats_and_preserves_comments() {
        let input = "fn d() -> Element {\n    render! {\n        view(style: css!(\n            flex_grow: 1.0,\n            background_color: BG,\n        )) {\n        view {\n                // \u{2500}\u{2500} Recent \u{2500}\u{2500}\n                Tx(icon: cart, name: \"Groceries\", positive: false\n    )\n                Tx(icon: coffee, name: \"Coffee\", positive: false)\n        }\n        }\n    }\n}\n";
        let expected = "fn d() -> Element {\n    render! {\n        view(\n            style: css!(\n                flex_grow: 1.0,\n                background_color: BG,\n            ),\n        ) {\n            view {\n                // \u{2500}\u{2500} Recent \u{2500}\u{2500}\n                Tx(icon: cart, name: \"Groceries\", positive: false)\n                Tx(icon: coffee, name: \"Coffee\", positive: false)\n            }\n        }\n    }\n}\n";
        let out = fmt(input);
        // The body MUST be reformatted (not the fallback), with the section
        // comment preserved at the right indent.
        assert_ne!(out, input, "must not fall back:\n{out}");
        assert_eq!(out, expected, "got:\n{out}");
        // And it must be a fixed point.
        assert_eq!(fmt(&out), out, "not idempotent");
    }

    // 18. Regression: a multi-line embedded-expr value (e.g. a wrapped
    // `css!( … )` kwarg) must format idempotently via the rustfmt-free
    // core — the verbatim slice is dedented before re-indenting so the
    // continuation lines don't gain indentation on every pass. This is the
    // bug that made the whole wallet body fail the idempotency fail-safe.
    #[test]
    fn multiline_expr_value_is_idempotent() {
        let input = "fn s() -> Element {\n    render! {\n        view(style: css!(\n            flex_grow: 1.0,\n            background_color: BG,\n            display: Display::Flex,\n        ))\n    }\n}\n";
        let once = fmt(input);
        let twice = fmt(&once);
        assert_eq!(
            once, twice,
            "not idempotent:\nonce:\n{once}\ntwice:\n{twice}"
        );
        // The css! values survive verbatim.
        assert!(once.contains("flex_grow: 1.0,"), "got:\n{once}");
        assert!(once.contains("background_color: BG,"), "got:\n{once}");
    }
}
