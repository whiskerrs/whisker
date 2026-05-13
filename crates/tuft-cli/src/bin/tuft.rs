//! `tuft` binary entry point.

fn main() -> anyhow::Result<()> {
    tuft_cli::run(std::env::args())
}
