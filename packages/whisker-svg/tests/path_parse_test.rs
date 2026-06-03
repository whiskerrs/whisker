//! Unit-level tests for `path_parse::parse` — the SVG `<path d>`
//! attribute tokeniser. Each test pins one tricky corner of the
//! SVG spec so a regression in the parser shows up immediately
//! rather than as a subtle rendering glitch in the integration
//! tests.

use whisker_svg::path_parse::{parse, PathCommand};

#[test]
fn absolute_move_line_close() {
    let cmds = parse("M 10 10 L 20 20 Z");
    assert_eq!(
        cmds,
        vec![
            PathCommand::MoveTo(10.0, 10.0),
            PathCommand::LineTo(20.0, 20.0),
            PathCommand::Close,
        ]
    );
}

#[test]
fn relative_commands_resolve_against_pen() {
    let cmds = parse("M 10 10 l 5 5 l -2 -2");
    assert_eq!(
        cmds,
        vec![
            PathCommand::MoveTo(10.0, 10.0),
            PathCommand::LineTo(15.0, 15.0),
            PathCommand::LineTo(13.0, 13.0),
        ]
    );
}

#[test]
fn implicit_lineto_after_moveto() {
    // Per SVG spec, additional coord pairs after M become L.
    let cmds = parse("M 0 0 10 10 20 20");
    assert_eq!(
        cmds,
        vec![
            PathCommand::MoveTo(0.0, 0.0),
            PathCommand::LineTo(10.0, 10.0),
            PathCommand::LineTo(20.0, 20.0),
        ]
    );
}

#[test]
fn horizontal_vertical_reduce_to_lineto() {
    let cmds = parse("M 0 0 H 10 V 20 h -5 v 5");
    assert_eq!(
        cmds,
        vec![
            PathCommand::MoveTo(0.0, 0.0),
            PathCommand::LineTo(10.0, 0.0),
            PathCommand::LineTo(10.0, 20.0),
            PathCommand::LineTo(5.0, 20.0),
            PathCommand::LineTo(5.0, 25.0),
        ]
    );
}

#[test]
fn cubic_then_smooth_reflects_control_point() {
    // After C ... 30 10 (last c2 = (30, 10)) at pen (20, 20),
    // S reflects to (10, 30) as implicit first control.
    let cmds = parse("M 0 0 C 0 10 30 10 20 20 S 50 30 60 40");
    let last_cubic = match cmds[2] {
        PathCommand::CubicTo(c1x, c1y, c2x, c2y, x, y) => (c1x, c1y, c2x, c2y, x, y),
        _ => panic!("expected cubic"),
    };
    assert!((last_cubic.0 - 10.0).abs() < 1e-5);
    assert!((last_cubic.1 - 30.0).abs() < 1e-5);
    assert!((last_cubic.2 - 50.0).abs() < 1e-5);
    assert!((last_cubic.3 - 30.0).abs() < 1e-5);
    assert!((last_cubic.4 - 60.0).abs() < 1e-5);
    assert!((last_cubic.5 - 40.0).abs() < 1e-5);
}

#[test]
fn quad_then_smooth_reflects_control_point() {
    // After Q 10 0 20 10 with pen at (20, 10), T reflects (10, 0)
    // through the pen to give implicit (30, 20).
    let cmds = parse("M 0 10 Q 10 0 20 10 T 40 10");
    let last_quad = match cmds[2] {
        PathCommand::QuadTo(cx, cy, x, y) => (cx, cy, x, y),
        _ => panic!("expected quad"),
    };
    assert!((last_quad.0 - 30.0).abs() < 1e-5);
    assert!((last_quad.1 - 20.0).abs() < 1e-5);
    assert!((last_quad.2 - 40.0).abs() < 1e-5);
    assert!((last_quad.3 - 10.0).abs() < 1e-5);
}

#[test]
fn comma_and_whitespace_separators_interchangeable() {
    let a = parse("M0,0L10,10");
    let b = parse("M 0 0 L 10 10");
    let c = parse("M0 0L10,10");
    assert_eq!(a, b);
    assert_eq!(b, c);
}

#[test]
fn negative_coords_without_separator() {
    // SVG allows `L-1-2` (the `-` is the separator).
    let cmds = parse("M0 0L-1-2");
    assert_eq!(
        cmds,
        vec![
            PathCommand::MoveTo(0.0, 0.0),
            PathCommand::LineTo(-1.0, -2.0),
        ]
    );
}

#[test]
fn scientific_notation_supported() {
    let cmds = parse("M 1e1 1e1 L 2e0 2e0");
    assert_eq!(
        cmds,
        vec![
            PathCommand::MoveTo(10.0, 10.0),
            PathCommand::LineTo(2.0, 2.0),
        ]
    );
}

#[test]
fn close_returns_pen_to_subpath_start() {
    let cmds = parse("M 5 5 L 10 5 Z l 1 0");
    // After Z, pen is back at (5,5), then relative l 1 0 → (6,5).
    assert_eq!(
        cmds,
        vec![
            PathCommand::MoveTo(5.0, 5.0),
            PathCommand::LineTo(10.0, 5.0),
            PathCommand::Close,
            PathCommand::LineTo(6.0, 5.0),
        ]
    );
}

#[test]
fn empty_input_yields_no_commands() {
    assert!(parse("").is_empty());
    assert!(parse("   \t  \n  ").is_empty());
}

#[test]
fn arc_emits_at_least_one_cubic() {
    // Quarter-arc from (10, 0) to (0, 10) with rx=ry=10 → one
    // cubic per ≤90° segment. We don't assert exact values
    // (kappa approximation has standard rounding); just that
    // arc was decomposed to cubics and the endpoint is hit.
    let cmds = parse("M 10 0 A 10 10 0 0 0 0 10");
    assert!(!cmds.is_empty());
    assert!(matches!(cmds.first(), Some(PathCommand::MoveTo(10.0, 0.0))));
    let last_cubic_endpoint = cmds.iter().rev().find_map(|c| match c {
        PathCommand::CubicTo(_, _, _, _, x, y) => Some((*x, *y)),
        _ => None,
    });
    let (ex, ey) = last_cubic_endpoint.expect("arc decomposed to cubics");
    assert!((ex - 0.0).abs() < 1e-3, "expected x ≈ 0, got {ex}");
    assert!((ey - 10.0).abs() < 1e-3, "expected y ≈ 10, got {ey}");
}

#[test]
fn malformed_input_recovers() {
    // Bad token mid-way — parser should keep what it got rather
    // than throw the entire path away.
    let cmds = parse("M 0 0 L 5 5 ??? L 10 10");
    assert_eq!(cmds[0], PathCommand::MoveTo(0.0, 0.0));
    assert_eq!(cmds[1], PathCommand::LineTo(5.0, 5.0));
    // Whatever happens after the `???` is implementation-defined,
    // but the prefix is preserved.
}
