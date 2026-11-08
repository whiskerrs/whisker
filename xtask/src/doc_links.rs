use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPOSITORY_URLS: &[&str] = &[
    "https://github.com/whiskerrs/whisker/blob/main/",
    "https://github.com/whiskerrs/whisker/tree/main/",
];

fn tracked_files(root: &Path) -> Vec<PathBuf> {
    super::capture(
        Command::new("git")
            .current_dir(root)
            .args(["ls-files", "-z"]),
    )
    .expect("git ls-files")
    .split('\0')
    .filter(|path| !path.is_empty())
    .map(PathBuf::from)
    .collect()
}

/// Relative Markdown link targets outside code, without `#fragment` or a
/// trailing `:line` suffix.
fn relative_links(markdown: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut fenced = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let prose = line.split('`').step_by(2).collect::<Vec<_>>().join("");
        let mut rest = prose.as_str();
        while let Some(start) = rest.find("](") {
            rest = &rest[start + 2..];
            let end = rest.find([')', ' ']).unwrap_or(rest.len());
            let target = &rest[..end];
            rest = &rest[end..];
            let is_url = target
                .split_once(':')
                .is_some_and(|(scheme, _)| scheme.chars().all(|c| c.is_ascii_alphabetic()));
            if target.is_empty() || target.starts_with(['#', '<']) || is_url {
                continue;
            }
            let path = target.split('#').next().unwrap_or_default();
            let path = match path.rsplit_once(':') {
                Some((file, line)) if line.chars().all(|c| c.is_ascii_digit() || c == '-') => file,
                _ => path,
            };
            if !path.is_empty() {
                targets.push(path.to_owned());
            }
        }
    }
    targets
}

/// Repository paths named by `blob/main` and `tree/main` GitHub URLs.
fn repository_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for prefix in REPOSITORY_URLS {
        let mut rest = text;
        while let Some(start) = rest.find(prefix) {
            rest = &rest[start + prefix.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || "_./-".contains(c)))
                .unwrap_or(rest.len());
            let path = rest[..end].trim_end_matches('.');
            if !path.is_empty() {
                paths.push(path.to_owned());
            }
        }
    }
    paths
}

#[test]
fn documentation_links_name_files_that_exist() {
    let root = super::workspace_root().expect("resolve workspace root");
    let mut broken = Vec::new();
    for file in tracked_files(&root) {
        let Ok(text) = fs::read_to_string(root.join(&file)) else {
            continue;
        };
        if file.extension().is_some_and(|extension| extension == "md") {
            let directory = file.parent().unwrap_or(Path::new(""));
            for target in relative_links(&text) {
                if !root.join(directory).join(&target).exists() {
                    broken.push(format!("{}: {target}", file.display()));
                }
            }
        }
        for path in repository_paths(&text) {
            if !root.join(&path).exists() {
                broken.push(format!("{}: {path}", file.display()));
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}

#[test]
fn relative_links_skip_urls_anchors_and_code() {
    let markdown = "\
See [a](../a.md#top), [b](b.rs:42), [web](https://x.dev/y), [s](#s), `[c](c.md)`.
```
[d](d.md)
```
[e](<e f.md>) and [g](g.md \"title\")";
    assert_eq!(relative_links(markdown), ["../a.md", "b.rs", "g.md"]);
}

#[test]
fn repository_paths_stop_at_url_punctuation() {
    // Built at runtime so this file doesn't itself name a missing path.
    let text = format!(
        "({}missing.md#x). \"{}crates\"",
        REPOSITORY_URLS[0], REPOSITORY_URLS[1]
    );
    assert_eq!(repository_paths(&text), ["missing.md", "crates"]);
}
