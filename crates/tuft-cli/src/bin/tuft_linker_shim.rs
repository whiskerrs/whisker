//! `tuft-linker-shim` — `-C linker=<shim>` target.
//!
//! rustc invokes us as
//!     tuft-linker-shim <linker-driver-args...>
//! when `-C linker=tuft-linker-shim` is in the rustc command line.
//! All the work is in `tuft_cli::linker_shim::run`.

fn main() -> anyhow::Result<()> {
    tuft_cli::linker_shim::run()
}
