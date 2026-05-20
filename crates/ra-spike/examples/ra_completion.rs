//! Rust-analyzer completion experiments.
//!
//! Open this file in VS Code (with rust-analyzer enabled). At each
//! marker ↓ position the cursor where indicated and try Ctrl+Space
//! (or trigger completion however your editor does). Note in the
//! adjacent comment which kind of completion fires.
//!
//! Build once first so RA has resolved the proc-macros:
//!
//!     cargo check -p ra-spike
//!
//! Then restart rust-analyzer in VS Code if needed.
//!
//! Partial-input cases live under `#[cfg(any())]` (never compiled,
//! always indexed by RA) so this example builds cleanly while still
//! exposing the test sites to the analyzer.

use ra_spike::{compose_a, compose_b, compose_c};

fn main() {}

// ---- Baseline (no macro) -------------------------------------------------

// Sanity check: completion on a plain method chain should always
// work. If even this doesn't show `style`, the issue is rust-analyzer
// itself, not our macro.
#[cfg(any())]
fn _baseline() {
    let _ = ra_spike::__tags::__view_ctor()
        // ← TEST 0: type `.s` here and trigger completion.
        //   Expected: `style`, `class` … (whatever starts with `s`).
        ;
}

// ---- Variant A: inline chain ---------------------------------------------

// `compose_a!` emits `__tags::__view_ctor().sty(()).__h()`.
fn _variant_a_full() {
    let _ = compose_a! { view(style: "x") };
}

#[cfg(any())]
fn _variant_a_partial() {
    // ← TEST A1: cursor at `sty` (no `:` yet). Does RA suggest `style`?
    let _ = compose_a! { view(sty) };
}

#[cfg(any())]
fn _variant_a_single_char() {
    // ← TEST A2: cursor at `s` alone.
    let _ = compose_a! { view(s) };
}

// ---- Variant B: typed-local-binding chain --------------------------------

// `compose_b!` emits `let __b: __tags::view = … ; __b.sty(()).__h()`.
fn _variant_b_full() {
    let _ = compose_b! { view(style: "x") };
}

#[cfg(any())]
fn _variant_b_partial() {
    // ← TEST B1: same prompt as A1 but routed through `let __b: view`.
    let _ = compose_b! { view(sty) };
}

// ---- Variant C: typed-builder shape (matches user-component path) --------

// `compose_c!` emits `view(ViewProps::builder().sty(()).build())` —
// same shape #[component]-generated code uses, which IS known to
// support completion.
fn _variant_c_full() {
    let _ = compose_c! { view(style: "x") };
}

#[cfg(any())]
fn _variant_c_partial() {
    // ← TEST C1: cursor at `sty`. Closest analogue to the (working)
    // custom-component path.
    let _ = compose_c! { view(sty) };
}
