//! `cargo-flint` binary entry point.
//!
//! Cargo invokes us as `cargo-flint flint <args>`, with `flint` inserted as
//! argv[1]. Strip it so the inner CLI sees the same shape as a direct
//! `flint <args>` invocation.

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("flint") {
        args.remove(1);
    }
    flint_cli::run(args)
}
