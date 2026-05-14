//! `tuft-rustc-shim` — `RUSTC_WORKSPACE_WRAPPER` target.
//!
//! Cargo invokes us as
//!     tuft-rustc-shim <rustc-path> <rustc-args...>
//! when `RUSTC_WORKSPACE_WRAPPER=tuft-rustc-shim` is in the env.
//! All the work is in `tuft_cli::rustc_shim::run`.

fn main() -> anyhow::Result<()> {
    tuft_cli::rustc_shim::run()
}
