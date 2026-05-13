//! Build script for tuft-ffi-lynx.
//!
//! In the future this will:
//! - Run bindgen against `native/bridge/include/tuft_bridge.h` to generate
//!   raw FFI bindings.
//! - Configure linker to find libtuft_bridge / Lynx prebuilt.
//!
//! For now it is a no-op so the workspace builds cleanly without any native
//! dependency present.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}
