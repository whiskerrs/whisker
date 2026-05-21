//! Native-module / native-element runtime support.
//!
//! Stub crate at this revision (Phase 7-A.2). The real surface —
//! `PubValue`, `call_module`, `subscribe_module`, `EventStream` —
//! lands with Phase 7-C alongside the `pub::Value` C ABI in Lynx.
//!
//! Kept in tree now so:
//!
//! - Downstream crates (proc-macros, sample modules) can take a dep
//!   on it without waiting for the implementation.
//! - The workspace's build / clippy / test pipeline catches any
//!   schema drift at the API level early.
//!
//! See `docs/phase-7-design.md` for the broader architecture and
//! `docs/whisker-module-toml.md` for the manifest schema this
//! crate's eventual API connects to.

/// Placeholder. The real type wraps a `*mut lynx_pub_value_t` from
/// the Lynx fork's C ABI and exposes a safe Rust surface for
/// constructing / inspecting `pub::Value`s. Phase 7-C lands the
/// definition; for now this is a marker so module crates can
/// already write `whisker_native_runtime::PubValue` in their
/// signatures.
#[derive(Debug)]
pub struct PubValue(());

/// Placeholder error type. Phase 7-C lands a real `ModuleError`
/// enum covering platform-side errors + transport errors + decode
/// failures.
#[derive(Debug)]
pub struct ModuleError(());

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        // Smoke test: keep CI honest while the crate is still a stub.
        // Real tests arrive with Phase 7-C.
    }
}
