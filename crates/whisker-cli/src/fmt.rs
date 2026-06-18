//! `whisker fmt` — a rustfmt drop-in that also formats Whisker's
//! `render!` / `css!` macro bodies.
//!
//! Three modes:
//!
//! - `whisker fmt <files...>` — format each file in place (or print to
//!   stdout with `--check`).
//! - `whisker fmt --stdin` — read stdin, write formatted to stdout.
//!   This is the rust-analyzer integration point:
//!   `rust-analyzer.rustfmt.overrideCommand = ["whisker", "fmt", "--stdin"]`.
//! - `whisker fmt --check <files...>` — don't write; print a unified
//!   diff and exit non-zero if any file would change.
//!
//! There are NO whisker-specific formatting options: the layout values
//! ([`whisker_fmt::FmtOptions`]) come from the nearest `rustfmt.toml`
//! (resolved per file directory), and the base Rust pass shells out to
//! the real rustfmt binary which reads `rustfmt.toml` itself.

use anyhow::{Context, Result, bail};
use clap::Args;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use whisker_fmt::FmtOptions;

#[derive(Args, Debug)]
pub struct FmtArgs {
    /// Rust source files to format. Ignored when `--stdin` is set.
    pub files: Vec<PathBuf>,

    /// Read source from stdin and write the formatted result to stdout.
    /// For `rust-analyzer.rustfmt.overrideCommand`.
    #[arg(long)]
    pub stdin: bool,

    /// Don't write anything. Print a unified diff of what would change
    /// and exit non-zero if any input is not already formatted.
    #[arg(long)]
    pub check: bool,
}

pub fn run(args: FmtArgs) -> Result<()> {
    if args.stdin {
        return run_stdin(&args);
    }
    if args.files.is_empty() {
        bail!("whisker fmt: no input files (pass file paths, or use --stdin)");
    }
    run_files(&args)
}

/// stdin → stdout. `--check` on stdin prints the diff to stderr and
/// exits non-zero if a change would be made.
fn run_stdin(args: &FmtArgs) -> Result<()> {
    let mut src = String::new();
    std::io::stdin()
        .read_to_string(&mut src)
        .context("reading source from stdin")?;
    // For stdin we resolve rustfmt.toml from the current directory.
    let opts = resolve_options(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let formatted = whisker_fmt::format_source_in_dir(
        &src,
        &opts,
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    )
    .context("formatting stdin")?;

    if args.check {
        if formatted != src {
            eprint!("{}", whisker_fmt::unified_diff(&src, &formatted));
            std::process::exit(1);
        }
        return Ok(());
    }

    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(formatted.as_bytes())
        .context("writing formatted output to stdout")?;
    Ok(())
}

fn run_files(args: &FmtArgs) -> Result<()> {
    let mut any_changed = false;
    let mut errored = false;

    for file in &args.files {
        match format_one_file(file, args.check) {
            Ok(changed) => {
                if changed {
                    any_changed = true;
                    if args.check {
                        // Diff already printed by format_one_file.
                    } else {
                        eprintln!("formatted {}", file.display());
                    }
                }
            }
            Err(e) => {
                errored = true;
                eprintln!("error: {}: {e:#}", file.display());
            }
        }
    }

    if errored {
        std::process::exit(1);
    }
    if args.check && any_changed {
        std::process::exit(1);
    }
    Ok(())
}

/// Format a single file. Returns `Ok(true)` if the file's content would
/// change. In `--check` mode prints a unified diff; otherwise writes the
/// result back in place.
fn format_one_file(path: &Path, check: bool) -> Result<bool> {
    let src =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    // `Path::parent` of a bare filename is `Some("")` (empty), which is
    // not a valid cwd to spawn rustfmt in — normalize it to `.`.
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let opts = resolve_options(dir);

    let formatted = whisker_fmt::format_source_in_dir(&src, &opts, dir)
        .with_context(|| format!("formatting {}", path.display()))?;

    if formatted == src {
        return Ok(false);
    }

    if check {
        println!("Diff in {}:", path.display());
        print!("{}", whisker_fmt::unified_diff(&src, &formatted));
    } else {
        std::fs::write(path, &formatted).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(true)
}

/// Build [`FmtOptions`] for `dir`, delegating to the whisker-fmt library
/// resolver so file-arg and `--stdin` paths both get the full edition
/// resolution: nearest `rustfmt.toml` `edition` → nearest `Cargo.toml`
/// edition (`[package]` / `[workspace.package]`) → `"2021"` default. The
/// base Rust pass re-reads `rustfmt.toml` via rustfmt itself; here we
/// supply the layout keys the macro-body printer needs plus the resolved
/// `--edition` (so 2018+ syntax like `async move` doesn't hit rustfmt's
/// 2015 default).
fn resolve_options(dir: &Path) -> FmtOptions {
    whisker_fmt::resolve_options(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_options_defaults_without_toml() {
        // A directory with no rustfmt.toml anywhere up the chain falls
        // back to rustfmt defaults. Use a tmp dir to avoid picking up a
        // repo-level config.
        let tmp = std::env::temp_dir().join(format!("whisker-fmt-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let o = resolve_options(&tmp);
        // No rustfmt.toml here, but a parent might have one in some
        // environments — only assert the function returns a valid set.
        assert!(o.tab_spaces >= 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_options_reads_local_toml() {
        let tmp = std::env::temp_dir().join(format!(
            "whisker-fmt-test-toml-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("rustfmt.toml"), "tab_spaces = 2\nmax_width = 80\n").unwrap();
        let o = resolve_options(&tmp);
        assert_eq!(o.tab_spaces, 2);
        assert_eq!(o.max_width, 80);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
