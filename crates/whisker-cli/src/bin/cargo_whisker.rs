//! `cargo-whisker` binary entry point.
//!
//! Cargo invokes us as `cargo-whisker whisker <args>`, with `whisker` inserted as
//! argv[1]. Strip it so the inner CLI sees the same shape as a direct
//! `whisker <args>` invocation.

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("whisker") {
        args.remove(1);
    }
    whisker_cli::run(args)
}
