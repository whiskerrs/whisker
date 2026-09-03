//! `whisker-fmt` — a rustfmt drop-in that also formats Whisker's
//! builder-composition macro bodies (`compose!`, `render!`, `css!`, and
//! `routes!`).
//!
//! # Architecture (mirrors yew-fmt)
//!
//! rustfmt leaves macro *bodies* untouched. So we:
//!
//! 1. Shell out to the real **rustfmt binary** (`--emit stdout`),
//!    letting it read the project's `rustfmt.toml` itself. This is the
//!    base Rust formatting. (the private `run_rustfmt` helper)
//! 2. Parse that output with `syn` + `proc-macro2` (`span-locations`),
//!    walk for composition macro invocations, re-parse each body with
//!    [`whisker_macro_syntax`], pretty-print it, and splice the result
//!    back over the original body token range. ([`reformat_macros`])
//!
//! [`format_source`] runs the whole pipeline. [`reformat_macros`] is
//! the macro-only pass; it needs no rustfmt binary, so it stays
//! testable where rustfmt is absent.
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
//! whitespace between tokens, so reprinting a composition macro body
//! from the parsed AST would lose them. They are recovered from the
//! body source text (the private `comments` module) and reattached while
//! pretty-printing (the private `printer` module): own-line comments go on their own line at the block's
//! indent, trailing comments are appended to the end of the preceding
//! line. Comments INSIDE an embedded expr value are excluded — they
//! ride along with the verbatim / rustfmt-formatted expr source.
//!
//! A **fail-safe** guards the result: if any recovered comment would be
//! dropped, or the output is not idempotent (`f(f(x)) != f(x)`), the
//! body is left **untouched** — so a comment can never be silently
//! lost. See the private `macro_body_edit` helper.

mod comments;
mod expr_fmt;
mod ir;
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
/// composition macro body found in the rustfmt output.
///
/// `opts` supplies the layout values the macro pretty-printer needs.
/// The rustfmt binary independently reads `rustfmt.toml`; pass
/// `opts.edition` through so both passes agree on the edition.
pub fn format_source(src: &str, opts: &FmtOptions) -> Result<String> {
    let exprfmt = ExprFormatter::new(opts);
    format_converged(src, opts, None, &exprfmt)
}

/// Like [`format_source`] but tells rustfmt to resolve `rustfmt.toml`
/// from `config_dir` (its `--config-path`). Used by the CLI so each
/// file's nearest `rustfmt.toml` governs.
pub fn format_source_in_dir(src: &str, opts: &FmtOptions, config_dir: &Path) -> Result<String> {
    let exprfmt = ExprFormatter::new_in_dir(opts, config_dir);
    format_converged(src, opts, Some(config_dir), &exprfmt)
}

/// Run rustfmt + the macro pass repeatedly until the output is a fixed
/// point, capped at 3 rounds. The two passes are individually idempotent
/// but their COMPOSITION need not be: the macro pass can emit a shape
/// (e.g. a multi-line `move || css!(…)` closure body) that the next
/// round's rustfmt rewrites again (into `move || { css!(…) }`), which
/// then IS stable.
fn format_converged(
    src: &str,
    opts: &FmtOptions,
    config_dir: Option<&Path>,
    exprfmt: &ExprFormatter,
) -> Result<String> {
    let round = |input: &str| -> Result<String> {
        let base = run_rustfmt(input, opts, config_dir, &[])?;
        reformat_macros_inner(&base, opts, Some(exprfmt))
    };
    let mut cur = round(src)?;
    if cur == src {
        return Ok(cur);
    }
    for _ in 0..2 {
        let next = round(&cur)?;
        if next == cur {
            break;
        }
        cur = next;
    }
    Ok(cur)
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
    if let Ok(p) = std::env::var("RUSTFMT")
        && !p.is_empty()
    {
        return p;
    }
    if let Ok(out) = Command::new("rustup").args(["which", "rustfmt"]).output()
        && out.status.success()
    {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return p;
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
/// `rustfmt.toml`; otherwise it runs in the current dir. `extra_config`
/// key=value pairs are passed via `--config`, overriding the config
/// file — the embedded-expr pass uses this for its slot-specific keys.
fn run_rustfmt(
    src: &str,
    opts: &FmtOptions,
    config_dir: Option<&Path>,
    extra_config: &[(&str, String)],
) -> Result<String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut cmd = Command::new(rustfmt_path());
    cmd.arg("--emit").arg("stdout");
    if let Some(ed) = &opts.edition {
        cmd.arg("--edition").arg(ed);
    }
    if !extra_config.is_empty() {
        let joined: Vec<String> = extra_config
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        cmd.arg("--config").arg(joined.join(","));
    }
    if let Some(dir) = config_dir {
        // `--config-path` must be the config FILE, not `dir`: the config
        // often lives at a parent while `dir` is a subdir with none of
        // its own, and `--config-path <dir>` then makes rustfmt error
        // "unable to find a config file for the given path".
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
/// rejects 2018+ syntax (`async move`, etc.).
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
/// 3. else the crate's default edition.
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

    // Never leave `edition` as `None` — rustfmt would then assume 2015.
    if opts.edition.is_none() {
        opts.edition = Some(cargo_toml_edition(dir).unwrap_or_else(|| DEFAULT_EDITION.to_string()));
    }

    opts
}

// ---- macro reformatting pass --------------------------------------------

/// Reformat every supported composition macro body found in `rust_src`
/// (which must already be valid, rustfmt-formatted Rust).
///
/// This is the testable core that does NOT need the rustfmt binary.
///
/// ## Comments
///
/// Comments inside a composition macro body are preserved: they're
/// recovered from the body source and reattached during pretty-printing. A
/// fail-safe in the private `macro_body_edit` helper falls back to leaving the body untouched if any
/// comment would be dropped or the result is not idempotent, so comments
/// are never lost.
pub fn reformat_macros(rust_src: &str, opts: &FmtOptions) -> Result<String> {
    // No `ExprFormatter`, so every embedded expr renders verbatim (the
    // printer's empty-map path) and no rustfmt binary is needed.
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

/// Recursively walk a token stream, finding supported composition macro
/// invocations and queueing an edit for each body.
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
        // Look for `IDENT ! GROUP` where IDENT is a supported macro.
        if let TokenTree::Ident(ident) = &trees[i] {
            let name = ident.to_string();
            if matches!(name.as_str(), "compose" | "render" | "css" | "routes")
                && i + 2 < trees.len()
                && matches!(&trees[i + 1], TokenTree::Punct(p) if p.as_char() == '!')
                && let TokenTree::Group(group) = &trees[i + 2]
            {
                if let Some(edit) =
                    macro_body_edit(&name, group, file_map, rust_src, opts, exprfmt, verify)?
                {
                    edits.push(edit);
                }
                // Never recurse into a macro body: nested
                // Nested composition macros are re-printed by the body's own
                // printer, so an inner edit would splice into byte
                // ranges the outer edit already owns.
                i += 3;
                continue;
            }
        }
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
    // Byte offsets of the WHOLE group, delimiters included.
    let Some((group_start, group_end)) = file_map.byte_range(span) else {
        return Ok(None);
    };
    // Delimiters are single chars, so the body is one byte inside each.
    let open_byte = group_start + 1;
    let close_byte = group_end - 1;
    if close_byte <= open_byte {
        return Ok(None); // empty body
    }
    let body_src = &rust_src[open_byte..close_byte];

    let line_start = rust_src[..group_start]
        .rfind('\n')
        .map(|n| n + 1)
        .unwrap_or(0);
    let line_prefix = &rust_src[line_start..group_start];
    let base_indent = indent_level_of(line_prefix, opts);

    // Span locations inside `body_ts` are relative to `body_src`, so the
    // SourceMap must cover exactly that substring.
    let body_ts: TokenStream = body_src
        .parse()
        .map_err(|e| anyhow!("whisker-fmt: could not lex {macro_name}! body: {e}"))?;
    let body_map = SourceMap::new(body_src);

    let body_len = body_src.len();

    // The embedded-expr spans are collected up front because they serve
    // twice: batch-formatting the exprs, and masking out expr-internal
    // comments so only GRAMMAR comments get reattached (expr-internal
    // ones ride along with the expr's own source).
    let (formatted, grammar_comments) = match macro_name {
        "compose" | "render" => match whisker_macro_syntax::compose::parse_input(body_ts.clone()) {
            Ok(input) if input.nodes.len() == 1 => {
                let mut roots = ir::adapt_compose_input(&input);
                let ir_root = roots.remove(0);
                let mut spans = Vec::new();
                ir::collect_ir_expr_spans(&ir_root, &mut spans);
                let comments = comments::collect_grammar_comments(body_src, &spans, &body_map);
                let expr_map = build_expr_map(&spans, &body_map, exprfmt);
                let s = printer::print_render(
                    &ir_root,
                    &body_map,
                    opts,
                    base_indent,
                    &expr_map,
                    exprfmt,
                    &comments,
                    body_len,
                );
                (s, comments)
            }
            // Not a well-formed body (e.g. mid-edit) — leave it.
            Ok(_) | Err(_) => return Ok(None),
        },
        "routes" => match whisker_macro_syntax::compose::parse_input(body_ts.clone()) {
            Ok(input) => {
                if input.nodes.is_empty() {
                    return Ok(None);
                }
                let ir_roots = ir::adapt_compose_input(&input);
                let mut spans = Vec::new();
                for root in &ir_roots {
                    ir::collect_ir_expr_spans(root, &mut spans);
                }
                let comments = comments::collect_grammar_comments(body_src, &spans, &body_map);
                let expr_map = build_expr_map(&spans, &body_map, exprfmt);
                let s = printer::print_routes(
                    &ir_roots,
                    &body_map,
                    opts,
                    base_indent,
                    &expr_map,
                    exprfmt,
                    &comments,
                    body_len,
                );
                (s, comments)
            }
            Err(_) => return Ok(None),
        },
        "css" => {
            match syn::parse2::<whisker_macro_syntax::compose::ComposeArguments>(body_ts.clone()) {
                Ok(input) => {
                    if input.arguments.is_empty() {
                        return Ok(None);
                    }
                    let mut spans = Vec::new();
                    for kw in &input.arguments {
                        if !kw.partial {
                            spans.push(span_of_expr(&kw.value));
                        }
                    }
                    let comments = comments::collect_grammar_comments(body_src, &spans, &body_map);
                    let expr_map = build_expr_map(&spans, &body_map, exprfmt);
                    let inline_budget = inline_body_budget(
                        group.delimiter(),
                        rust_src,
                        line_start,
                        group_start,
                        close_byte,
                        opts,
                    );
                    let s = printer::print_css(
                        &input,
                        &body_map,
                        opts,
                        base_indent,
                        &expr_map,
                        exprfmt,
                        &comments,
                        body_len,
                        inline_budget,
                    );
                    (s, comments)
                }
                Err(_) => return Ok(None),
            }
        }
        _ => return Ok(None),
    };

    // A single-line body stays on the macro's own line when the whole
    // line (prefix, delimiters, and whatever follows the macro) fits
    // `max_width`; otherwise the body breaks onto its own lines. The
    // printer already indented a multi-line body to `base_indent + 1`.
    let closing_indent = opts.indent_prefix(base_indent);
    let replacement = if let Some(inline) = inline_replacement(
        &formatted,
        group.delimiter(),
        rust_src,
        line_start,
        group_start,
        close_byte,
        opts,
    ) {
        inline
    } else {
        format!("\n{formatted}\n{closing_indent}")
    };

    if body_src == replacement {
        return Ok(None);
    }

    // Comment-preservation fail-safe: if reattaching the comments could
    // have dropped one, or the result isn't a fixed point, leave the body
    // UNTOUCHED rather than risk losing a comment.
    if verify && !grammar_comments.is_empty() {
        if !all_comments_present(&replacement, &grammar_comments) {
            return Ok(None);
        }
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

/// Char budget a single-line BODY (delimiter padding excluded) may use
/// and still keep the macro's whole original line — the text before the
/// opening delimiter through the text after the closing one — within
/// `max_width`.
fn inline_body_budget(
    delimiter: Delimiter,
    rust_src: &str,
    line_start: usize,
    group_start: usize,
    close_byte: usize,
    opts: &FmtOptions,
) -> usize {
    let prefix = rust_src[line_start..group_start].chars().count();
    let after_close = close_byte + 1;
    let line_end = rust_src[after_close..]
        .find('\n')
        .map(|n| after_close + n)
        .unwrap_or(rust_src.len());
    let suffix = rust_src[after_close..line_end].trim_end().chars().count();
    let pads = if delimiter == Delimiter::Brace { 2 } else { 0 };
    opts.max_width.saturating_sub(prefix + 2 + pads + suffix)
}

/// The inline (single-line) body replacement, or `None` when the body
/// must break: it is multi-line or over [`inline_body_budget`]. Brace
/// bodies get rustfmt's `name! { … }` interior padding; paren/bracket
/// bodies none.
#[allow(clippy::too_many_arguments)]
fn inline_replacement(
    formatted: &str,
    delimiter: Delimiter,
    rust_src: &str,
    line_start: usize,
    group_start: usize,
    close_byte: usize,
    opts: &FmtOptions,
) -> Option<String> {
    if formatted.contains('\n') {
        return None;
    }
    let body = formatted.trim_start();
    let budget = inline_body_budget(
        delimiter,
        rust_src,
        line_start,
        group_start,
        close_byte,
        opts,
    );
    if body.chars().count() > budget {
        return None;
    }
    let pad = if delimiter == Delimiter::Brace {
        " "
    } else {
        ""
    };
    Some(format!("{pad}{body}{pad}"))
}

// ---- comment-preservation fail-safe helpers -----------------------------

/// Every recovered comment's (trimmed) text must appear in `output`,
/// counting duplicates: if the body has two identical comments, both must
/// survive. Uses a per-text occurrence count.
fn all_comments_present(output: &str, comments: &[comments::GrammarComment]) -> bool {
    use std::collections::HashMap;
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
    // A trivial fn wrapper plus a manual indent prefix is enough to put
    // the macro line at `base_indent`, since rustfmt never runs on this.
    let src = format!("fn _w() {{\n{indent}{macro_name}! {{{replacement}}}\n}}\n");
    // `verify = false` so this check does not recurse into itself —
    // otherwise each fixed-point check spawns another, blowing the stack
    // on large / nested bodies.
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
#[path = "lib/tests.rs"]
mod tests;
