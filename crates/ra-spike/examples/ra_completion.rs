//! Rust-analyzer completion experiments.
//!
//! **This file is NOT expected to compile.** The partial-input
//! cases below intentionally invoke methods that don't exist
//! (`.sty` etc.) — they're there so rust-analyzer can index them
//! and surface what completions it offers at the cursor position.
//! Run completion checks in VS Code; ignore `cargo check`'s errors.
//!
//! Setup:
//!
//!     cargo check -p ra-spike   # may fail — expected
//!
//! Then open this file in VS Code, restart rust-analyzer if needed,
//! and try Ctrl+Space at each marker.

#![allow(dead_code, unused_imports, unused_variables, unused_must_use)]

use ra_spike::{
    compose_a, compose_b, compose_c, expr, render, render_g, render_h, render_i, render_j, text,
};

fn main() {}

// ---- Baseline (no macro) -------------------------------------------------

// Sanity check: completion on a plain method chain on the same
// builder type. If even this doesn't surface `style`, the issue is
// the rust-analyzer setup / extension, not our macro.
fn baseline() {
    let _ = ra_spike::__tags::__view_ctor()
        .sty // ← TEST 0: cursor right after `sty`. Expected: `style`.
        ;
}

// ---- Variant A: inline chain ---------------------------------------------

// `compose_a!` emits `__tags::__view_ctor().sty(()).__h()`.

fn variant_a_partial() {
    // ← TEST A1: cursor right after `sty` (no `:` yet).
    let _ = compose_a! { view(sty) };
}

fn variant_a_single_char() {
    // ← TEST A2: cursor right after `s` alone.
    let _ = compose_a! { view(s) };
}

// Reference (compiles): full kwarg.
fn variant_a_full() {
    let _ = compose_a! { view(style: "x") };
}

// ---- Variant B: typed-local-binding chain --------------------------------

// `compose_b!` emits `let __b: __tags::view = … ; __b.sty(()).__h()`.

fn variant_b_partial() {
    // ← TEST B1: cursor right after `sty`, routed via `let __b: view`.
    let _ = compose_b! { view(sty) };
}

fn variant_b_full() {
    let _ = compose_b! { view(style: "x") };
}

// ---- Variant C: typed-builder shape (matches user-component path) --------

// `compose_c!` emits `view(ViewProps::builder().sty(()).build())` —
// same shape #[component]-generated code uses.

fn variant_c_partial() {
    // ← TEST C1: cursor right after `sty`.
    let _ = compose_c! { view(sty) };
}

fn variant_c_full() {
    let _ = compose_c! { view(style: "x") };
}

// ---- Variant D: render! with children block ------------------------------

// `render! { view(prop: value) { view(...) } }` — full compose with
// nested children. Same inline-chain shape as A, no `let __h = …`
// binding at any level.

fn variant_d_outer_partial() {
    // ← TEST D1: outer kwarg completion, with an empty child block.
    let _ = render! { view(sty) { } };
}

fn variant_d_outer_partial_with_child() {
    // ← TEST D2: outer kwarg completion, with a real child present.
    // Tests whether the `.child({…})` tail call confuses RA.
    let _ = render! { view(sty) { view(class: "y") } };
}

fn variant_d_inner_partial() {
    // ← TEST D3: completion on a kwarg INSIDE the children block.
    // This is the case hello-world hits and currently fails.
    let _ = render! { view(style: "x") { view(sty) } };
}

fn variant_d_deep_inner_partial() {
    // ← TEST D4: completion two levels deep.
    let _ = render! {
        view(style: "outer") {
            view(class: "mid") {
                view(sty)
            }
        }
    };
}

fn variant_d_sibling_partial() {
    // ← TEST D5: completion on a sibling after a complete sibling.
    let _ = render! {
        view(style: "outer") {
            view(class: "sib1")
            view(sty)
        }
    };
}

// Reference (compiles): full kwargs, all levels.
fn variant_d_full() {
    let _ = render! {
        view(style: "outer") {
            view(class: "mid") {
                view(style: "inner")
            }
        }
    };
}

// ---- Variant E: render! with text + {expr} children ----------------------

// Same shape as D, but children include string literals and
// `{expr}` interpolation blocks. Verifies that mixing child kinds
// doesn't break completion on adjacent kwargs.

fn variant_e_with_text_sibling() {
    // ← TEST E1: partial kwarg next to a text child.
    let _ = render! {
        view(sty) {
            "hello world"
        }
    };
}

fn variant_e_with_expr_sibling() {
    let greeting = "hi";
    // ← TEST E2: partial kwarg next to a {expr} child.
    let _ = render! {
        view(sty) {
            { greeting }
        }
    };
}

fn variant_e_partial_inside_with_text() {
    // ← TEST E3: partial kwarg on an inner element that also has
    // text/expr siblings.
    //
    // Sibling order matters: `view(...)` immediately followed by
    // `{ … }` always binds as that element's children block (same
    // trailing-lambda rule compose-Kotlin uses), so put the
    // `{expr}` and text siblings BEFORE the partial element.
    let count = 0;
    let _ = render! {
        view(style: "outer") {
            "before"
            { count }
            "between"
            view(sty)
        }
    };
}

fn variant_e_full() {
    let count = 0;
    let _ = render! {
        view(style: "outer") {
            "before"
            { count }
            "between"
            view(class: "mid")
        }
    };
}

// ---- Variant F: closure attrs (event handlers) ---------------------------

// Verifies completion when one of the sibling kwargs is a closure
// expression (`on_tap: || …`). The closure body is regular Rust
// code, but the surrounding macro context could in principle
// disrupt RA's per-kwarg method resolution.

fn variant_f_partial_after_closure() {
    // ← TEST F1: partial kwarg AFTER a closure-typed kwarg.
    let _ = render! {
        view(on_tap: || println!("hi"), sty) {}
    };
}

fn variant_f_partial_before_closure() {
    // ← TEST F2: partial kwarg BEFORE a closure-typed kwarg.
    let _ = render! {
        view(sty, on_tap: || println!("hi")) {}
    };
}

fn variant_f_partial_on_handler_name() {
    // ← TEST F3: completion on the handler name itself (`on_t`).
    // Expected: suggests `on_tap`, `on`.
    let _ = render! {
        view(on_t) {}
    };
}

fn variant_f_full() {
    let _ = render! {
        view(style: "x", on_tap: || println!("hi")) {
            "tap me"
        }
    };
}

// ---- Variant G: text/expr children as `__text_make(value)` ---------------

// E1–E3 failed (text/expr children inside `.child({ chain })`
// blocked completion on the parent's kwarg). G replaces that
// emission with a free function call `__text_make(value)`,
// dropping the nested chain entirely.

fn variant_g_with_text_sibling() {
    // ← TEST G1: same shape as E1 with the G emission.
    let _ = render_g! {
        view(sty) {
            "hello world"
        }
    };
}

fn variant_g_with_expr_sibling() {
    let greeting = "hi";
    // ← TEST G2: same shape as E2 with the G emission.
    let _ = render_g! {
        view(sty) {
            { greeting }
        }
    };
}

fn variant_g_full() {
    let count = 0;
    let _ = render_g! {
        view(style: "outer") {
            "before"
            { count }
            "between"
            view(class: "mid")
        }
    };
}

// ---- Variant H: text/expr children as `.text_child(value)` ---------------

// H lifts the text child up onto the parent builder as a direct
// method, eliminating `.child(…)` entirely for text/expr cases.

fn variant_h_with_text_sibling() {
    // ← TEST H1: same shape as E1 with the H emission.
    let _ = render_h! {
        view(sty) {
            "hello world"
        }
    };
}

fn variant_h_with_expr_sibling() {
    let greeting = "hi";
    // ← TEST H2: same shape as E2 with the H emission.
    let _ = render_h! {
        view(sty) {
            { greeting }
        }
    };
}

fn variant_h_full() {
    let count = 0;
    let _ = render_h! {
        view(style: "outer") {
            "before"
            { count }
            "between"
            view(class: "mid")
        }
    };
}

// ---- Variant I: text/expr children dropped from emission ----------------

// G and H both failed: even with the nested chain removed (G) and
// even with `.child(…)` removed (H), text/expr siblings still
// blocked completion. The remaining hypothesis is that it's not
// about the emission at all — it's about RA's handling of the
// macro *input* once it contains a LitStr or `{expr}` child.
//
// Variant I drops text/expr children from emission entirely (just
// keeps element children). If I works but E/G/H don't, the
// problem is in our emission for literal/expr tokens (e.g., span
// duplication). If I ALSO doesn't work, the problem is upstream
// — the macro input shape alone is enough to break RA's mapping,
// and we can't fix it from inside the proc-macro.

fn variant_i_with_text_sibling() {
    // ← TEST I1: same shape as E1, text child dropped at emission.
    let _ = render_i! {
        view(sty) {
            "hello world"
        }
    };
}

fn variant_i_with_expr_sibling() {
    let greeting = "hi";
    // ← TEST I2: same shape as E2, expr child dropped at emission.
    let _ = render_i! {
        view(sty) {
            { greeting }
        }
    };
}

fn variant_i_full() {
    let _ = render_i! {
        view(style: "outer") {
            view(class: "mid")
        }
    };
}

// ---- Variant J: text/expr children wrapped as `text(EXPR)` / `expr(EXPR)` -

// I confirmed the issue is upstream of emission — RA's input
// fixup gives up on a children block that contains bare LitStrs
// or `{expr}` blocks at top level. F worked because its closure
// kwargs sit inside `()` which RA already parses as a function
// argument list.
//
// J's bet: if EVERY top-level item in the children block is
// function-call-shaped (`view(...)`, `text("…")`, `expr(val)`),
// the whole block looks like Rust statements and RA's fixup
// stays on its happy path.

fn variant_j_with_text_sibling() {
    // ← TEST J1: partial kwarg next to a `text(...)` child.
    let _ = render_j! {
        view(sty) {
            text("hello world")
        }
    };
}

fn variant_j_with_expr_sibling() {
    let greeting = "hi";
    // ← TEST J2: partial kwarg next to an `expr(...)` child.
    let _ = render_j! {
        view(sty) {
            expr(greeting)
        }
    };
}

fn variant_j_mixed_partial_inside() {
    // ← TEST J3: partial kwarg on an inner element next to text/expr.
    let count = 0;
    let _ = render_j! {
        view(style: "outer") {
            text("before")
            expr(count)
            view(sty)
        }
    };
}

fn variant_j_full() {
    let count = 0;
    let _ = render_j! {
        view(style: "outer") {
            text("before")
            expr(count)
            view(class: "mid")
        }
    };
}
