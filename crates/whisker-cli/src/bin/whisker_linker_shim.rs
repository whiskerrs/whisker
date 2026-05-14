//! `whisker-linker-shim` — `-C linker=<shim>` target.
//!
//! rustc invokes us as
//!     whisker-linker-shim <linker-driver-args...>
//! when `-C linker=whisker-linker-shim` is in the rustc command line.
//! All the work is in `whisker_cli::linker_shim::run`.

fn main() -> anyhow::Result<()> {
    whisker_cli::linker_shim::run()
}
