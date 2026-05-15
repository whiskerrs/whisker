//! Fixture for whisker-dev-server's hot-patch integration tests.
//!
//! Two flavours of function on purpose:
//!
//! - `#[no_mangle]` `answer` — the easy case. Symbol name is
//!   stable across rustc releases, so the test can find it by
//!   name without thinking about mangling. Used by the
//!   `thin_rebuild` smoke test.
//!
//! - mangled `calculate` — the **important** case. This is what
//!   real Whisker user code looks like (no `#[no_mangle]`); it
//!   exercises the question "is rustc's name-mangling stable
//!   enough that build → edit → build produces the same symbol
//!   name on both sides so build_jump_table can match them?"
//!   Used by the `patcher` integration test as the load-bearing
//!   check that Tier 1 hot-patch is even possible for ordinary
//!   Rust functions.

#[no_mangle]
pub extern "C" fn answer() -> i32 {
    42
}

#[no_mangle]
pub extern "C" fn other_function() -> i32 {
    99
}

/// Mangled — the test verifies that the symbol name produced by
/// rustc here is the same on both v1 and v2 of the source.
#[inline(never)]
pub fn calculate(x: i32) -> i32 {
    x * 2
}

// Dummy stubs for the symbols `Patcher::build_patch` exports on
// Mach-O. The real Whisker `#[whisker::main]` macro generates these;
// fixtures don't go through the macro, so we define them by hand to
// keep the integration-test link happy.
#[no_mangle]
pub extern "C" fn whisker_aslr_anchor() -> i32 {
    0
}
#[no_mangle]
pub extern "C" fn whisker_app_main() {}
#[no_mangle]
pub extern "C" fn whisker_tick() -> bool {
    false
}
