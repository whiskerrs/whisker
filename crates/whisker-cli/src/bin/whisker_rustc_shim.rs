//! `whisker-rustc-shim` — `RUSTC_WORKSPACE_WRAPPER` target.
//!
//! Cargo invokes us as
//!     whisker-rustc-shim <rustc-path> <rustc-args...>
//! when `RUSTC_WORKSPACE_WRAPPER=whisker-rustc-shim` is in the env.
//! All the work is in `whisker_cli::rustc_shim::run`.

fn main() -> anyhow::Result<()> {
    whisker_cli::rustc_shim::run()
}
