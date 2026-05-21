//! `cargo-whisker` binary entry point.
//!
//! Cargo invokes us as `cargo-whisker whisker <args>`, with `whisker` inserted as
//! argv[1]. Strip it so the inner CLI sees the same shape as a direct
//! `whisker <args>` invocation.

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("whisker") {
        args.remove(1);
    }
    if let Err(e) = whisker_cli::run(args) {
        let message = if std::env::var("WHISKER_VERBOSE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
        {
            format!("{e:#}")
        } else {
            e.root_cause().to_string()
        };
        whisker_build::ui::error(&message);
        std::process::exit(1);
    }
}
