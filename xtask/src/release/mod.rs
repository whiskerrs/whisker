//! One release plan and PR coordinate the independently versioned native SDKs
//! and the Rust workspace. Publishing is resumable from that immutable plan.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

mod native;
mod prepare;
mod publish;
#[cfg(test)]
mod tests;

const PLAN_PATH: &str = ".github/release.json";
const CLI_PATH: &str = "crates/whisker-cli/src/platforms.rs";
const IOS_PATH: &str = "crates/whisker-cng/src/ios_modules.rs";
const REPOSITORY: &str = "whiskerrs/whisker";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ReleasePlan {
    version: String,
    source: String,
    sdk: Option<String>,
    gradle: Option<String>,
    ios: Option<String>,
    subsecond: Option<String>,
    crates: BTreeMap<String, String>,
}

impl ReleasePlan {
    fn read(root: &Path) -> Result<Self> {
        let plan: Self = serde_json::from_str(&fs::read_to_string(root.join(PLAN_PATH))?)?;
        version(&plan.version)?;
        for value in [&plan.sdk, &plan.gradle, &plan.ios, &plan.subsecond]
            .into_iter()
            .flatten()
        {
            version(value)?;
        }
        Ok(plan)
    }

    fn notes_path(&self) -> String {
        format!("releases/{}.md", self.version)
    }

    fn tag(&self) -> String {
        format!("whisker-v{}", self.version)
    }

    fn validate_checkout(&self, root: &Path) -> Result<()> {
        ensure!(
            self.crates == published_packages(root)?,
            "release plan differs from Cargo manifests"
        );
        let manifest = read_toml(&root.join("Cargo.toml"))?;
        ensure!(
            manifest["workspace"]["package"]["version"].as_str() == Some(&self.version),
            "workspace version differs from release plan"
        );
        for (stream, selected) in [
            ("sdk", &self.sdk),
            ("gradle", &self.gradle),
            ("ios", &self.ios),
        ] {
            if let Some(selected) = selected {
                ensure!(
                    &native::pin(root, stream)? == selected,
                    "{stream} pin differs from release plan"
                );
            }
        }
        native::check_swift_pins(root, &native::pin(root, "ios")?)?;
        ensure!(
            root.join(self.notes_path()).is_file(),
            "release notes are missing"
        );
        git(root, &["merge-base", "--is-ancestor", &self.source, "HEAD"])?;
        Ok(())
    }

    fn validate_selected_streams(&self, root: &Path) -> Result<()> {
        // Main can advance between preparation and merge. Re-evaluate omitted
        // streams against the actual release commit before publishing anything.
        for (stream, selected) in [
            ("sdk", &self.sdk),
            ("gradle", &self.gradle),
            ("ios", &self.ios),
        ] {
            if selected.is_none() {
                native::check_selection(root, stream, None)?;
            }
        }
        if self.subsecond.is_none() {
            let changed = git(
                root,
                &[
                    "diff",
                    "--name-only",
                    &self.source,
                    "HEAD",
                    "--",
                    "crates/whisker-subsecond",
                ],
            )?;
            ensure!(
                changed.trim().is_empty(),
                "fork changed after preparation; prepare a new release with subsecond_version"
            );
        }
        Ok(())
    }
}

pub fn run(root: &Path, mode: &str, args: Vec<String>) -> Result<()> {
    match (mode, args.as_slice()) {
        ("prepare", []) => prepare::run(root),
        ("plan", []) => plan(root),
        ("publish", []) => publish::run(root),
        ("native-check", [stream, value]) => native::check(root, stream, value),
        ("native-pins", [stream, value]) => native::check_pins(root, stream, value),
        ("native-stamp", [stream, value]) => native::stamp(root, stream, value),
        ("native-tag", [stream, value]) => native::tag(root, stream, value),
        ("native-verify", [stream, value]) => native::verify(stream, value),
        _ => bail!(
            "usage: cargo xtask release <prepare|plan|publish>\n       cargo xtask release <native-check|native-stamp|native-tag|native-verify> <sdk|gradle|ios> <version>"
        ),
    }
}

fn plan(root: &Path) -> Result<()> {
    let changed = git(
        root,
        &["diff", "--name-only", "HEAD^", "HEAD", "--", PLAN_PATH],
    )?;
    if changed.trim().is_empty() {
        return output("release", "false");
    }
    let plan = ReleasePlan::read(root)?;
    plan.validate_checkout(root)?;
    plan.validate_selected_streams(root)?;
    output("release", "true")?;
    output("version", &plan.version)?;
    for (key, value) in [
        ("sdk", plan.sdk),
        ("gradle", plan.gradle),
        ("ios", plan.ios),
    ] {
        output(key, value.as_deref().unwrap_or(""))?;
    }
    Ok(())
}

fn version(value: &str) -> Result<semver::Version> {
    let parsed = semver::Version::parse(value)
        .context("expected a version such as 0.13.4, without a v prefix")?;
    ensure!(
        parsed.pre.is_empty() && parsed.build.is_empty(),
        "release versions must be stable major.minor.patch versions"
    );
    Ok(parsed)
}

fn read_toml(path: &Path) -> Result<DocumentMut> {
    fs::read_to_string(path)?
        .parse()
        .with_context(|| format!("read {}", path.display()))
}

fn metadata(root: &Path) -> Result<serde_json::Value> {
    let text = crate::capture(Command::new(crate::cargo()).current_dir(root).args([
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
    ]))?;
    Ok(serde_json::from_str(&text)?)
}

fn published_packages(root: &Path) -> Result<BTreeMap<String, String>> {
    let data = metadata(root)?;
    let members = data["workspace_members"]
        .as_array()
        .context("workspace members")?;
    let mut packages = BTreeMap::new();
    for package in data["packages"].as_array().context("workspace packages")? {
        if !members.contains(&package["id"]) || package["publish"] == serde_json::json!([]) {
            continue;
        }
        ensure!(
            package["publish"].is_null() || package["publish"] == serde_json::json!(["crates-io"]),
            "release supports crates.io packages only"
        );
        packages.insert(
            package["name"].as_str().context("package name")?.to_owned(),
            package["version"]
                .as_str()
                .context("package version")?
                .to_owned(),
        );
    }
    ensure!(!packages.is_empty(), "no publishable packages");
    Ok(packages)
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    crate::capture(Command::new("git").current_dir(root).args(args))
}

fn gh(root: &Path, args: &[&str]) -> Result<String> {
    crate::capture(Command::new("gh").current_dir(root).args(args))
}

fn github_json(root: &Path, endpoint: &str, raw: bool) -> Result<Option<serde_json::Value>> {
    let accept = if raw {
        "Accept: application/vnd.github.raw+json"
    } else {
        "Accept: application/vnd.github+json"
    };
    let response = Command::new("gh")
        .current_dir(root)
        .args(["api", "-H", accept, endpoint])
        .output()?;
    if response.status.success() {
        Ok(Some(serde_json::from_slice(&response.stdout)?))
    } else {
        ensure!(
            String::from_utf8_lossy(&response.stderr).contains("(HTTP 404)"),
            "GitHub read failed: {}",
            String::from_utf8_lossy(&response.stderr)
        );
        Ok(None)
    }
}

fn output(key: &str, value: &str) -> Result<()> {
    ensure!(!value.contains(['\r', '\n']), "invalid workflow output");
    println!("{key}={value}");
    if let Some(path) = std::env::var_os("GITHUB_OUTPUT") {
        writeln!(OpenOptions::new().append(true).open(path)?, "{key}={value}")?;
    }
    Ok(())
}

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .user_agent("whisker-release (https://github.com/whiskerrs/whisker)")
        .build()
}

fn remote_tag(root: &Path, tag: &str) -> Result<Option<String>> {
    let text = git(
        root,
        &[
            "ls-remote",
            "--tags",
            "origin",
            &format!("refs/tags/{tag}"),
            &format!("refs/tags/{tag}^{{}}"),
        ],
    )?;
    // Prefer the peeled commit for annotated tags.
    Ok(text
        .lines()
        .last()
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned))
}

fn ensure_tag(root: &Path, tag: &str, create: bool) -> Result<()> {
    let head = git(root, &["rev-parse", "HEAD"])?;
    if let Some(existing) = remote_tag(root, tag)? {
        ensure!(
            existing == head.trim(),
            "{tag} already points to another commit; use a new version"
        );
    } else if create {
        // A failed push can leave a local tag behind in a developer checkout.
        let local = Command::new("git")
            .current_dir(root)
            .args([
                "rev-parse",
                "--verify",
                &format!("refs/tags/{tag}^{{commit}}"),
            ])
            .output()?;
        if local.status.success() {
            ensure!(
                String::from_utf8_lossy(&local.stdout).trim() == head.trim(),
                "local {tag} points to another commit"
            );
        } else {
            git(root, &["tag", tag])?;
        }
        git(root, &["push", "origin", &format!("refs/tags/{tag}")])?;
    }
    Ok(())
}

fn manifests(root: &Path) -> Result<Vec<PathBuf>> {
    let files = git(root, &["ls-files", "--", "Cargo.toml", "**/Cargo.toml"])?;
    Ok(files.lines().map(|path| root.join(path)).collect())
}
