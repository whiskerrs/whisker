//! Integration tests that exercise the FULL pipeline, including the
//! rustfmt subprocess. Gated on `rustfmt` being available so the suite
//! still passes in environments without it (the macro-only unit tests
//! in `src/lib.rs` cover the formatter logic without rustfmt).

use whisker_fmt::{check_source, format_source, rustfmt_available, FmtOptions};

fn opts(tab: usize, width: usize) -> FmtOptions {
    FmtOptions {
        max_width: width,
        tab_spaces: tab,
        hard_tabs: false,
        edition: Some("2021".to_string()),
    }
}

#[test]
fn full_pipeline_formats_rust_and_macro() {
    if !rustfmt_available() {
        eprintln!("skipping: rustfmt binary not available");
        return;
    }
    let messy =
        "fn   ui()->Element{ render!{view(style:\"x\",class:\"y\"){text(value:\"hi\")}} }\n";
    let out = format_source(messy, &opts(4, 100)).expect("format_source");

    // rustfmt normalized the fn signature …
    assert!(out.contains("fn ui() -> Element {"), "rust pass:\n{out}");
    // … and the macro body got reformatted onto its own indented lines.
    assert!(out.contains("    render! {"), "macro indent:\n{out}");
    assert!(
        out.contains("        view(style: \"x\", class: \"y\") {"),
        "kwargs formatted:\n{out}"
    );
    assert!(
        out.contains("            text(value: \"hi\")"),
        "child indent:\n{out}"
    );
}

#[test]
fn full_pipeline_idempotent() {
    if !rustfmt_available() {
        return;
    }
    let messy = "fn ui()->Element{render!{view(style:\"x\"){text(value:\"hi\")}}}\n";
    let once = format_source(messy, &opts(4, 100)).unwrap();
    let twice = format_source(&once, &opts(4, 100)).unwrap();
    assert_eq!(
        once, twice,
        "not idempotent:\n--once--\n{once}\n--twice--\n{twice}"
    );
}

#[test]
fn check_reports_diff_then_clean() {
    if !rustfmt_available() {
        return;
    }
    let messy = "fn ui()->Element{render!{view(style:\"x\")}}\n";
    // First check: not formatted → Some(diff).
    let diff = check_source(messy, &opts(4, 100)).unwrap();
    assert!(diff.is_some(), "expected a diff for messy input");
    // After formatting, check is clean.
    let formatted = format_source(messy, &opts(4, 100)).unwrap();
    let clean = check_source(&formatted, &opts(4, 100)).unwrap();
    assert!(
        clean.is_none(),
        "formatted input should be clean, got:\n{clean:?}"
    );
}

// ---- embedded-expr rustfmt formatting -----------------------------------

#[test]
fn formats_embedded_format_macro_expr() {
    if !rustfmt_available() {
        return;
    }
    // The kwarg value is a `format!` call with a missing comma-space:
    // rustfmt should normalize it.
    let src =
        "fn ui() -> Element {\n    render! { text(value: format!(\"count: {}\",c.get())) }\n}\n";
    let out = format_source(src, &opts(4, 100)).unwrap();
    assert!(
        out.contains("format!(\"count: {}\", c.get())"),
        "embedded format! should be rustfmt-normalized:\n{out}"
    );
}

#[test]
fn long_kwarg_value_wraps_at_max_width() {
    if !rustfmt_available() {
        return;
    }
    // A long call expression as a kwarg value should be wrapped by
    // rustfmt onto multiple lines.
    let long = "some_function_with_a_fairly_long_name(argument_one, argument_two, argument_three, argument_four)";
    let src = format!("fn ui() -> Element {{\n    render! {{ text(value: {long}) }}\n}}\n");
    let narrow = format_source(&src, &opts(4, 40)).unwrap();
    let wide = format_source(&src, &opts(4, 200)).unwrap();
    // Narrow max_width forces the expr to wrap (more body lines); wide
    // keeps it on one line — proving rustfmt.toml/max_width flows in.
    assert!(
        narrow.matches('\n').count() > wide.matches('\n').count(),
        "narrow max_width should wrap the embedded expr more than wide:\n--narrow--\n{narrow}\n--wide--\n{wide}"
    );
}

#[test]
fn multi_statement_closure_handler_reindented() {
    if !rustfmt_available() {
        return;
    }
    // A multi-statement closure handler should be rustfmt-formatted and
    // re-indented under the kwarg column in the macro body.
    let src = "fn ui() -> Element {\n    render! { view(on_tap: move |_| { let x=1;do_thing(x); }) }\n}\n";
    let out = format_source(src, &opts(4, 100)).unwrap();
    // rustfmt splits the two statements onto their own lines …
    assert!(out.contains("let x = 1;"), "statement formatted:\n{out}");
    assert!(out.contains("do_thing(x);"), "statement formatted:\n{out}");
    // … and they sit indented inside the macro body (not at col 0).
    assert!(
        out.contains("\n            let x = 1;") || out.contains("\n                let x = 1;"),
        "closure body re-indented into the macro:\n{out}"
    );
}

#[test]
fn comment_inside_expr_preserved() {
    if !rustfmt_available() {
        return;
    }
    // A block comment INSIDE the expr must survive — proving we format
    // the SOURCE SLICE, not the (comment-stripped) AST.
    let src = "fn ui() -> Element {\n    render! { text(value: foo(/* keep me */ x)) }\n}\n";
    let out = format_source(src, &opts(4, 100)).unwrap();
    assert!(
        out.contains("/* keep me */"),
        "comment inside expr must be preserved:\n{out}"
    );
}

#[test]
fn full_pipeline_idempotent_with_exprs() {
    if !rustfmt_available() {
        return;
    }
    let src = "fn ui() -> Element {\n    render! { view(on_tap: move |_| { let x=1;do_thing(x); }, style: \"flex:1;\") { text(value: format!(\"count: {}\",c.get())) } }\n}\n";
    let once = format_source(src, &opts(4, 100)).unwrap();
    let twice = format_source(&once, &opts(4, 100)).unwrap();
    assert_eq!(
        once, twice,
        "expr formatting must be idempotent:\n--once--\n{once}\n--twice--\n{twice}"
    );
}

#[test]
fn tab_spaces_option_changes_output() {
    if !rustfmt_available() {
        return;
    }
    // Proves the layout flows from FmtOptions, not a hardcoded value.
    // NOTE: rustfmt itself defaults to 4-space indent for the *Rust*
    // part regardless of `opts.tab_spaces` (it reads rustfmt.toml, not
    // our opts), so we assert on the MACRO-BODY indentation, which the
    // whisker pretty-printer controls via `opts.tab_spaces`.
    let src =
        "fn ui() -> Element {\n    render! { view(style: \"x\") { text(value: \"hi\") } }\n}\n";
    let four = format_source(src, &opts(4, 100)).unwrap();
    let two = format_source(src, &opts(2, 100)).unwrap();
    assert_ne!(four, two, "tab_spaces must change macro indentation");
}
