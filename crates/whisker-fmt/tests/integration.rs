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
