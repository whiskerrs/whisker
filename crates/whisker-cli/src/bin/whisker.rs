//! `whisker` binary entry point.

fn main() {
    if let Err(e) = whisker_cli::run(std::env::args()) {
        // anyhow's default formatter prints `Error: <top>\n\nCaused by:\n    <chain>`,
        // which surfaces our internal call-stack to the user (`resolve user-crate
        // manifest (Cargo.toml + whisker.rs)`) on every misuse. We re-render through
        // `whisker_build::ui::error()` so the user sees a single line in the same
        // visual style as the rest of `whisker run`'s output. Verbose mode prints
        // the full chain so we don't actually lose debuggability — the chain is
        // still in `e:#`.
        let message = if std::env::var("WHISKER_VERBOSE")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
        {
            format!("{e:#}")
        } else {
            // `root_cause()` is the leaf of the anyhow chain — the
            // actually-actionable detail (e.g. "no `[package]` Cargo.toml at or
            // above …"). The intermediate frames are only useful for debugging
            // the CLI itself, which is what `--verbose` is for.
            e.root_cause().to_string()
        };
        whisker_build::ui::error(&message);
        std::process::exit(1);
    }
}
