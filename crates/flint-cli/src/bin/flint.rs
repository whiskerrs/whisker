//! `flint` binary entry point.

fn main() -> anyhow::Result<()> {
    flint_cli::run(std::env::args())
}
