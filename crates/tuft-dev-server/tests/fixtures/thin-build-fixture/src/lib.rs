//! Fixture for tuft-dev-server's `thin_rebuild` integration test.
//!
//! `#[no_mangle]` keeps the symbol name predictable across rustc
//! releases so the test can locate `answer` in the resulting dylib's
//! symbol table without dealing with name mangling.

#[no_mangle]
pub extern "C" fn answer() -> i32 {
    42
}

#[no_mangle]
pub extern "C" fn other_function() -> i32 {
    99
}
