//! `whisker` binary entry point.

fn main() -> anyhow::Result<()> {
    whisker_cli::run(std::env::args())
}
