//! `lyra` binary entry point.

fn main() -> anyhow::Result<()> {
    lyra_cli::run(std::env::args())
}
