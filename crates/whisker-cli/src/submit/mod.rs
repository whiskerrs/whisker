//! `whisker submit` — upload an already-built release artifact to
//! its store.
//!
//! Responsibility boundary: submit never builds. It takes the
//! artifact the last `whisker build ipa` / `appbundle` left on disk
//! (or `--path`) and talks to the store's API directly — no
//! Transporter, altool, or fastlane. Credentials come from the same
//! `credentials/` store builds use; like build, submit only consumes
//! them.
//!
//! What accompanies the binary (its release notes) comes from the
//! app's optional `store.rs`; the store page itself is `whisker store
//! push`'s job.

mod android;
mod ios;

use anyhow::{Result, bail, ensure};
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct SubmitArgs {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Upload the `.ipa` to App Store Connect (TestFlight / App
    /// Store) through the Build Upload API, authenticated with the
    /// stored App Store Connect API key.
    Ios(ios::Args),
    /// Upload the `.aab` to Google Play (and release it with
    /// `--track`), authenticated with the stored service-account key.
    Android(android::Args),
}

pub fn run(args: SubmitArgs) -> Result<()> {
    match args.cmd {
        Cmd::Ios(a) => ios::run(a),
        Cmd::Android(a) => android::run(a),
    }
}

/// `--path` if given, else the last build's artifact.
fn artifact(
    explicit: Option<PathBuf>,
    last_build: Option<PathBuf>,
    build_command: &str,
) -> Result<PathBuf> {
    match explicit {
        Some(path) if path.is_file() => Ok(path),
        Some(path) => bail!("{} is not a file", path.display()),
        None => match last_build.filter(|p| p.is_file()) {
            Some(path) => Ok(path),
            None => bail!("nothing to submit — run `{build_command}` first, or pass --path"),
        },
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// `(locale, text)` for every locale that declares release notes,
/// refusing any text over the store's `limit` before anything is
/// uploaded.
fn release_notes<'a>(
    declared: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    store: &str,
    limit: usize,
) -> Result<Vec<(String, String)>> {
    let mut notes = Vec::new();
    for (locale, text) in declared {
        let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
            continue;
        };
        let length = text.chars().count();
        ensure!(
            length <= limit,
            "store.rs: {store} release notes for `{locale}` are {length} characters; the limit is {limit}"
        );
        notes.push((locale.to_string(), text.to_string()));
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECLARED: [(&str, Option<&str>); 3] = [
        ("ja-JP", Some("  修正しました\n")),
        ("en-US", None),
        ("fr-FR", Some("   ")),
    ];

    #[test]
    fn notes_skip_locales_without_text() {
        let notes = release_notes(DECLARED, "Google Play", 500).unwrap();
        assert_eq!(
            notes,
            vec![("ja-JP".to_string(), "修正しました".to_string())]
        );
    }

    #[test]
    fn notes_over_the_limit_are_refused_counting_characters_not_bytes() {
        // 6 characters, 18 bytes.
        assert!(release_notes(DECLARED, "Google Play", 6).is_ok());
        let err = release_notes(DECLARED, "Google Play", 5)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("`ja-JP`") && err.contains("limit is 5"),
            "{err}"
        );
    }
}
