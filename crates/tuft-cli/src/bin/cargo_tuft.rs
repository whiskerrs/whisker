//! `cargo-tuft` binary entry point.
//!
//! Cargo invokes us as `cargo-tuft tuft <args>`, with `tuft` inserted as
//! argv[1]. Strip it so the inner CLI sees the same shape as a direct
//! `tuft <args>` invocation.

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("tuft") {
        args.remove(1);
    }
    tuft_cli::run(args)
}
