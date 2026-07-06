//! Tiny interactive-input helpers for the credential wizards.
//!
//! Deliberately plain stdin/stdout — the wizards are sequential Q&A,
//! not a TUI, so the output stays copy-pastable and agent-readable
//! (same reasoning as `whisker doctor`'s plain scrollback).

use anyhow::{Context, Result};
use std::io::{IsTerminal, Write};

/// Both ends of the conversation must be a terminal — a wizard with
/// stdin piped from a file would silently consume garbage.
pub fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// `question [y/N]` — default no.
pub fn confirm(question: &str) -> Result<bool> {
    let answer = line(&format!("{question} [y/N]"))?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

/// Prompt on stdout, read one trimmed line from stdin.
pub fn line(question: &str) -> Result<String> {
    print!("{question} ");
    std::io::stdout().flush().context("flush stdout")?;
    let mut buf = String::new();
    std::io::stdin()
        .read_line(&mut buf)
        .context("read from stdin")?;
    Ok(buf.trim().to_string())
}

/// Hidden input (no echo) — passwords and the age secret key.
pub fn password(question: &str) -> Result<String> {
    rpassword::prompt_password(format!("{question} "))
        .context("read hidden input from terminal")
        .map(|s| s.trim().to_string())
}
