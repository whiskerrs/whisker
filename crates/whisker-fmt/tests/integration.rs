//! Integration tests that exercise the FULL pipeline, including the
//! rustfmt subprocess. Gated on `rustfmt` being available so the suite
//! still passes in environments without it (the macro-only unit tests
//! in `src/lib.rs` cover the formatter logic without rustfmt).

use whisker_fmt::{FmtOptions, check_source, format_source, rustfmt_available};

fn opts(tab: usize, width: usize) -> FmtOptions {
    FmtOptions {
        max_width: width,
        tab_spaces: tab,
        hard_tabs: false,
        edition: Some("2021".to_string()),
        single_line_if_else_max_width: None,
    }
}

#[test]
fn full_pipeline_formats_rust_and_macro() {
    if !rustfmt_available() {
        eprintln!("skipping: rustfmt binary not available");
        return;
    }
    let messy =
        "fn   ui()->Element{ render!{View(style:\"x\",class:\"y\"){Text(value:\"hi\")}} }\n";
    let out = format_source(messy, &opts(4, 100)).expect("format_source");

    assert!(out.contains("fn ui() -> Element {"), "rust pass:\n{out}");
    assert!(out.contains("    render! {"), "macro indent:\n{out}");
    assert!(
        out.contains("        View(style: \"x\", class: \"y\") {"),
        "kwargs formatted:\n{out}"
    );
    assert!(
        out.contains("            Text(value: \"hi\")"),
        "child indent:\n{out}"
    );
}

#[test]
fn full_pipeline_idempotent() {
    if !rustfmt_available() {
        return;
    }
    let messy = "fn ui()->Element{render!{View(style:\"x\"){Text(value:\"hi\")}}}\n";
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
    let messy = "fn ui()->Element{render!{View(style:\"x\")}}\n";
    let diff = check_source(messy, &opts(4, 100)).unwrap();
    assert!(diff.is_some(), "expected a diff for messy input");
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
    let src =
        "fn ui() -> Element {\n    render! { Text(value: format!(\"count: {}\",c.get())) }\n}\n";
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
    let long = "some_function_with_a_fairly_long_name(argument_one, argument_two, argument_three, argument_four)";
    let src = format!("fn ui() -> Element {{\n    render! {{ Text(value: {long}) }}\n}}\n");
    let narrow = format_source(&src, &opts(4, 40)).unwrap();
    let wide = format_source(&src, &opts(4, 200)).unwrap();
    // Narrow max_width must wrap the expr where wide keeps it inline,
    // proving rustfmt.toml's max_width reaches the embedded-expr pass.
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
    let src = "fn ui() -> Element {\n    render! { View(on_tap: move |_| { let x=1;do_thing(x); }) }\n}\n";
    let out = format_source(src, &opts(4, 100)).unwrap();
    assert!(out.contains("let x = 1;"), "statement formatted:\n{out}");
    assert!(out.contains("do_thing(x);"), "statement formatted:\n{out}");
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
    // A comment inside the expr only survives if the SOURCE SLICE is
    // formatted rather than the comment-stripped AST.
    let src = "fn ui() -> Element {\n    render! { Text(value: foo(/* keep me */ x)) }\n}\n";
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
    let src = "fn ui() -> Element {\n    render! { View(on_tap: move |_| { let x=1;do_thing(x); }, style: \"flex:1;\") { Text(value: format!(\"count: {}\",c.get())) } }\n}\n";
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
    // rustfmt indents the *Rust* part from rustfmt.toml, not from
    // `opts`, so only the MACRO-BODY indentation reflects `tab_spaces`.
    let src =
        "fn ui() -> Element {\n    render! { View(style: \"x\") { Text(value: \"hi\") } }\n}\n";
    let four = format_source(src, &opts(4, 100)).unwrap();
    let two = format_source(src, &opts(2, 100)).unwrap();
    assert_ne!(four, two, "tab_spaces must change macro indentation");
}

#[test]
fn full_pipeline_nested_css_in_render_reformats() {
    if !rustfmt_available() {
        return;
    }
    // The macro pass must still reach into the nested `css!(...)` kwarg
    // after the real rustfmt pass has run over the file — exercising
    // `ExprFormatter` batching, not just the rustfmt-free core.
    let messy = "fn   ui( )->Element{render!{View(style:css!(flex_grow:1.0,background_color:BG)){Text(value:\"hi\")}}}\n";
    let out = format_source(messy, &opts(4, 100)).unwrap();
    assert!(out.contains("fn ui() -> Element {"), "rust pass:\n{out}");
    assert!(
        out.contains("style: css!(flex_grow: 1.0, background_color: BG)"),
        "nested css! reformatted:\n{out}"
    );
}

#[test]
fn full_pipeline_nested_routes_in_render_idempotent() {
    if !rustfmt_available() {
        return;
    }
    let messy = "fn app()->Element{render!{View{Router(routes:routes!{Switch{Route(path:\"a\",component:A)Route(path:\"b\",component:B)}}){Outlet{}}}}}\n";
    let once = format_source(messy, &opts(4, 100)).unwrap();
    let twice = format_source(&once, &opts(4, 100)).unwrap();
    assert_eq!(
        once, twice,
        "not idempotent:\n--once--\n{once}\n--twice--\n{twice}"
    );
    assert!(once.contains("Switch {\n"), "got:\n{once}");
}

#[test]
fn statement_macro_in_closure_converges_in_one_call() {
    if !rustfmt_available() {
        return;
    }
    // The macro pass emits a multi-line `move || css!(…)`; a later
    // rustfmt round rewrites the closure body into a block. One
    // format_source call must already return that stable form.
    let src = "fn ui() -> Element {\n    let style = computed(move || css!(flex_direction: FlexDirection::Column, gap: px(12.0), margin_bottom: px(16.0), padding_left: px(20.0)));\n    render! { View(style: style) }\n}\n";
    let once = format_source(src, &opts(4, 100)).unwrap();
    let twice = format_source(&once, &opts(4, 100)).unwrap();
    assert_eq!(
        once, twice,
        "must converge within one call:\n--once--\n{once}\n--twice--\n{twice}"
    );
}

#[test]
fn closure_wrapped_render_kwarg_converges_in_one_call() {
    if !rustfmt_available() {
        return;
    }
    let src = "fn ui() -> Element {\n    render! { View { ForEach(each: move || rows(), key: |i: &usize| i.to_string(), children: move |_: usize| render! { View(style: row_style) }) } }\n}\n";
    let once = format_source(src, &opts(4, 100)).unwrap();
    let twice = format_source(&once, &opts(4, 100)).unwrap();
    assert_eq!(
        once, twice,
        "must converge within one call:\n--once--\n{once}\n--twice--\n{twice}"
    );
    assert!(
        once.contains("render! {\n"),
        "closure-wrapped render! must format:\n{once}"
    );
}

#[test]
fn closure_wrapped_render_stays_unblocked_through_rustfmt() {
    if !rustfmt_available() {
        return;
    }
    // rustfmt blocks a multi-line `|…| render! {…}` closure body in the
    // embedded-expr pass; inside a macro body the slot is whisker-fmt's,
    // so the sole-macro body must come back unblocked.
    let src = "fn ui() -> Element {\n    render! {\n        ForEach(\n            each: move || rows(),\n            key: |i: &usize| i.to_string(),\n            children: move |_: usize| render! {\n                View(style: row_style, on_tap: move |_| handle_row_tap_for(row_identifier))\n            },\n        )\n    }\n}\n";
    let once = format_source(src, &opts(4, 100)).unwrap();
    assert!(
        once.contains("children: move |_: usize| render! {\n"),
        "closure body must stay unblocked:\n{once}"
    );
    assert!(
        !once.contains("|_: usize| {"),
        "closure body must not be blockified:\n{once}"
    );
    let twice = format_source(&once, &opts(4, 100)).unwrap();
    assert_eq!(once, twice, "not idempotent:\n--once--\n{once}");
}

#[test]
fn single_line_if_else_slot_value_stays_inline() {
    if !rustfmt_available() {
        return;
    }
    // 52 chars — over rustfmt's default single_line_if_else_max_width
    // of 50, which the embedded-expr pass widens to max_width.
    let src = "fn s() -> Css {\n    css! {\n        opacity: if dimming_is_enabled_now.get() { 0.5 } else { 1.0 },\n        color: red,\n    }\n}\n";
    let once = format_source(src, &opts(4, 100)).unwrap();
    assert!(
        once.contains("opacity: if dimming_is_enabled_now.get() { 0.5 } else { 1.0 },"),
        "slot if-else must stay on one line:\n{once}"
    );
    let twice = format_source(&once, &opts(4, 100)).unwrap();
    assert_eq!(once, twice, "not idempotent:\n--once--\n{once}");
}

#[test]
fn deep_if_else_slot_value_expands_when_line_lacks_room() {
    if !rustfmt_available() {
        return;
    }
    // A 78-char if at column 25: the expression alone fits max_width,
    // but its line doesn't — the per-column allowance must break it.
    let src = "fn s() -> Css {\n    css! {\n        justify_content: if value.get() { JustifyContent::FlexEnd } else { JustifyContent::FlexStart },\n        color: red,\n    }\n}\n";
    let once = format_source(src, &opts(4, 100)).unwrap();
    for line in once.lines() {
        assert!(
            line.chars().count() <= 100,
            "line exceeds max_width:\n{line}\nfull output:\n{once}"
        );
    }
    assert!(
        once.contains("justify_content: if value.get() {\n"),
        "over-allowance if must break:\n{once}"
    );
    let twice = format_source(&once, &opts(4, 100)).unwrap();
    assert_eq!(once, twice, "not idempotent:\n--once--\n{once}");
}

#[test]
fn full_pipeline_composite_podcast_like_tree() {
    if !rustfmt_available() {
        return;
    }
    // A shape close to the real `examples/podcast` app, through the FULL
    // pipeline, checked for idempotency and the max_width budget.
    let messy = "fn app()->Element{render!{View(style:css!(flex_grow:1.0,width:vw(100),height:vh(100),background_color:BG,display:Display::Flex)){Router(routes:routes!{Stack{Route(path:\"\",component:Home)Route(path:\"detail/:id\",component:Detail)}}){Outlet{}}}}}\n";
    let once = format_source(messy, &opts(4, 100)).unwrap();
    let twice = format_source(&once, &opts(4, 100)).unwrap();
    assert_eq!(
        once, twice,
        "not idempotent:\n--once--\n{once}\n--twice--\n{twice}"
    );
    for line in once.lines() {
        assert!(
            line.chars().count() <= 100,
            "line exceeds max_width:\n{line}\nfull output:\n{once}"
        );
    }
}
