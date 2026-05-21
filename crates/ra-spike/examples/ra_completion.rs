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

use ra_spike::{compose_a, compose_b, compose_c, render};

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
