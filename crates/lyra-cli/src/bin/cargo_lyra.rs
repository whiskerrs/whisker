//! `cargo-lyra` binary entry point.
//!
//! Cargo invokes us as `cargo-lyra lyra <args>`, with `lyra` inserted as
//! argv[1]. Strip it so the inner CLI sees the same shape as a direct
//! `lyra <args>` invocation.

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("lyra") {
        args.remove(1);
    }
    lyra_cli::run(args)
}
