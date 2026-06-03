//! Fixture-driven integration test.
//!
//! Walks every `tests/fixtures/*.svg`, compiles it through
//! [`whisker_svg::compile`], runs the resulting bytes through
//! [`TraceVisitor`], and asserts the trace matches the matching
//! `*.trace.txt`. Also asserts the produced bytes match
//! `*.bin` — those byte files are the canonical cross-platform
//! fixtures consumed by the iOS / Android replayer tests in
//! `packages/whisker-svg/`.
//!
//! ## Updating goldens
//!
//! Run with `WHISKER_SVG_UPDATE_GOLDEN=1 cargo test -p whisker-svg`
//! to (re)write the `*.trace.txt` and `*.bin` files instead of
//! comparing. Used when the SPEC changes intentionally.

use std::fs;
use std::path::{Path, PathBuf};

use whisker_svg::{compile, replay, TraceVisitor};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn update_mode() -> bool {
    std::env::var_os("WHISKER_SVG_UPDATE_GOLDEN").is_some()
}

fn run_fixture(svg_path: &Path) {
    let svg = fs::read_to_string(svg_path).expect("read svg");
    let compiled = compile(&svg).expect("compile svg");
    assert!(
        compiled.warnings.is_empty(),
        "fixture {} produced warnings: {:?}",
        svg_path.display(),
        compiled.warnings
    );

    // ---- trace golden ----
    let mut tracer = TraceVisitor::new();
    replay(&compiled.bytes, &mut tracer).expect("replay clean");
    let actual_trace = tracer.into_string();
    let trace_path = svg_path.with_extension("trace.txt");

    if update_mode() {
        fs::write(&trace_path, &actual_trace).expect("write trace golden");
    } else {
        let expected = fs::read_to_string(&trace_path).unwrap_or_else(|_| {
            panic!(
                "missing golden trace `{}`. Re-run with WHISKER_SVG_UPDATE_GOLDEN=1.",
                trace_path.display()
            )
        });
        assert_eq!(
            actual_trace,
            expected,
            "trace mismatch for `{}`",
            svg_path.display()
        );
    }

    // ---- bin golden — the cross-platform fixture ----
    let bin_path = svg_path.with_extension("bin");
    if update_mode() {
        fs::write(&bin_path, &compiled.bytes).expect("write bin golden");
    } else {
        let expected = fs::read(&bin_path).unwrap_or_else(|_| {
            panic!(
                "missing golden bin `{}`. Re-run with WHISKER_SVG_UPDATE_GOLDEN=1.",
                bin_path.display()
            )
        });
        assert_eq!(
            compiled.bytes,
            expected,
            "bin mismatch for `{}`",
            svg_path.display()
        );
    }
}

#[test]
fn rect_solid() {
    run_fixture(&fixtures_dir().join("rect_solid.svg"));
}

#[test]
fn path_triangle() {
    run_fixture(&fixtures_dir().join("path_triangle.svg"));
}

#[test]
fn path_quad() {
    run_fixture(&fixtures_dir().join("path_quad.svg"));
}

#[test]
fn path_cubic() {
    run_fixture(&fixtures_dir().join("path_cubic.svg"));
}

#[test]
fn stroke_outline() {
    run_fixture(&fixtures_dir().join("stroke_outline.svg"));
}

#[test]
fn currentcolor() {
    run_fixture(&fixtures_dir().join("currentcolor.svg"));
}

#[test]
fn nested_transform() {
    run_fixture(&fixtures_dir().join("nested_transform.svg"));
}

#[test]
fn opacity_group() {
    run_fixture(&fixtures_dir().join("opacity_group.svg"));
}

#[test]
fn circle_basic() {
    run_fixture(&fixtures_dir().join("circle_basic.svg"));
}
