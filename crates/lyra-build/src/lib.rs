//! Build helper for user crates with `#[lyra::main]`.
//!
//! Each app's `build.rs` collapses to:
//!
//! ```ignore
//! fn main() {
//!     lyra_build::compile().unwrap();
//! }
//! ```
//!
//! Everything else — cc::Build invocation, header search paths,
//! Android linker quirks, iOS xcframework slice selection,
//! `-fobjc-arc`, … — lives behind that one call. Users shouldn't have
//! to know the details to ship a Lyra app.
//!
//! Path conventions are re-exported via [`paths`] so the build
//! orchestration side (`xtask`) and the per-crate build side
//! (`compile`) read from the same constants. Drift between "where the
//! xcframework was put" and "where build.rs looked for it" is the
//! class of bug we explicitly want to make impossible.

use anyhow::Result;

pub mod paths;

mod android;
mod ios;

/// Drop-in `build.rs::main` body. Selects the right platform
/// implementation based on `CARGO_CFG_TARGET_OS` and no-ops on
/// host targets (so `cargo check`, `cargo test --target <host>`,
/// rust-analyzer-style invocations, … all stay quiet).
pub fn compile() -> Result<()> {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "android" => android::compile(),
        "ios" => ios::compile(),
        _ => Ok(()),
    }
}
