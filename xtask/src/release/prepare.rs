use super::*;
use toml_edit::{Item, TableLike, value};

pub(super) fn run(root: &Path) -> Result<()> {
    ensure!(
        std::env::var("GITHUB_REF").as_deref() == Ok("refs/heads/main"),
        "prepare releases using Actions on main"
    );
    ensure!(
        git(root, &["status", "--porcelain", "--untracked-files=no"])?
            .trim()
            .is_empty(),
        "release preparation requires a clean checkout"
    );
    let requested = std::env::var("RELEASE_VERSION").context("RELEASE_VERSION is required")?;
    version(&requested)?;
    let source = git(root, &["rev-parse", "HEAD"])?.trim().to_owned();
    let mut plan = ReleasePlan {
        version: requested,
        source,
        sdk: optional("SDK_VERSION")?,
        gradle: optional("GRADLE_VERSION")?,
        ios: optional("IOS_VERSION")?,
        subsecond: optional("SUBSECOND_VERSION")?,
        crates: BTreeMap::new(),
    };
    let branch = format!("codex/release-v{}", plan.version);
    let remote = git(
        root,
        &[
            "ls-remote",
            "--heads",
            "origin",
            &format!("refs/heads/{branch}"),
        ],
    )?;
    if !remote.trim().is_empty() {
        git(root, &["fetch", "origin", &branch])?;
        git(root, &["checkout", "--detach", "FETCH_HEAD"])?;
        let previous = ReleasePlan::read(root)?;
        plan.crates = previous.crates.clone();
        ensure!(
            plan == previous,
            "{branch} already has a different release plan; reuse the original run or choose a new version"
        );
        plan.validate_checkout(root)?;
    } else {
        let old = read_toml(&root.join("Cargo.toml"))?["workspace"]["package"]["version"]
            .as_str()
            .context("workspace version")?
            .to_owned();
        ensure!(
            version(&plan.version)? > version(&old)?,
            "Rust version must be newer than {old}"
        );
        if plan.subsecond.is_none() {
            let changed = git(
                root,
                &[
                    "diff",
                    "--name-only",
                    &format!("whisker-v{old}"),
                    "HEAD",
                    "--",
                    "crates/whisker-subsecond",
                ],
            )?;
            ensure!(
                changed.trim().is_empty(),
                "whisker-subsecond changed; specify its independent version:\n{changed}"
            );
        }
        for (stream, selected) in [
            ("sdk", &plan.sdk),
            ("gradle", &plan.gradle),
            ("ios", &plan.ios),
        ] {
            native::check_selection(root, stream, selected.as_deref())?;
        }
        ensure!(
            remote_tag(root, &plan.tag())?.is_none(),
            "{} already exists",
            plan.tag()
        );
        let open: Vec<serde_json::Value> = serde_json::from_str(&gh(
            root,
            &["pr", "list", "--state", "open", "--json", "headRefName,url"],
        )?)?;
        ensure!(
            !open.iter().any(|pr| pr["headRefName"]
                .as_str()
                .is_some_and(|name| name.starts_with("codex/release-v"))),
            "another release PR is open; finish or close it first"
        );
        git(root, &["switch", "-c", &branch])?;
        stamp_versions(root, &plan)?;
        crate::run(
            Command::new(crate::cargo())
                .current_dir(root)
                .args(["update", "--workspace"]),
        )?;
        plan.crates = published_packages(root)?;
        let notes = release_notes(root, &plan, &old)?;
        fs::create_dir_all(root.join("releases"))?;
        fs::write(root.join(plan.notes_path()), notes)?;
        fs::write(
            root.join(PLAN_PATH),
            format!("{}\n", serde_json::to_string_pretty(&plan)?),
        )?;
        plan.validate_checkout(root)?;
        git(root, &["add", "--update"])?;
        git(root, &["add", PLAN_PATH, &plan.notes_path()])?;
        git(
            root,
            &["commit", "-m", &format!("chore: release v{}", plan.version)],
        )?;
        git(root, &["push", "--set-upstream", "origin", &branch])?;
    }
    let existing: Vec<serde_json::Value> = serde_json::from_str(&gh(
        root,
        &[
            "pr",
            "list",
            "--head",
            &branch,
            "--state",
            "all",
            "--json",
            "url,state",
        ],
    )?)?;
    let url = if let Some(pr) = existing.first() {
        ensure!(
            pr["state"] == "OPEN",
            "release PR is already closed or merged: {}",
            pr["url"]
        );
        pr["url"].as_str().context("PR URL")?.to_owned()
    } else {
        gh(
            root,
            &[
                "pr",
                "create",
                "--base",
                "main",
                "--head",
                &branch,
                "--title",
                &format!("chore: release v{}", plan.version),
                "--body-file",
                &plan.notes_path(),
            ],
        )?
        .trim()
        .to_owned()
    };
    // GITHUB_TOKEN pushes don't trigger ordinary CI. An explicit dispatch does,
    // and its check runs attach to the release branch's commit for protection.
    gh(root, &["workflow", "run", "ci.yml", "--ref", &branch])?;
    output("pr", &url)?;
    output("branch", &branch)?;
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        writeln!(
            OpenOptions::new().append(true).open(path)?,
            "Release PR: {url}\n\nApprove and merge after CI passes. Native SDKs and Rust will publish automatically, followed by one GitHub Release."
        )?;
    }
    Ok(())
}

fn optional(key: &str) -> Result<Option<String>> {
    match std::env::var(key).ok().filter(|value| !value.is_empty()) {
        Some(value) => {
            version(&value)?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

pub(super) fn stamp_versions(root: &Path, plan: &ReleasePlan) -> Result<()> {
    let mut versions = published_packages(root)?;
    let fork = versions
        .get("whisker-subsecond")
        .context("whisker-subsecond version")?
        .clone();
    if let Some(next) = &plan.subsecond {
        ensure!(
            version(next)? > version(&fork)?,
            "subsecond version must be newer than {fork}"
        );
    }
    for (name, current) in &mut versions {
        *current = if name == "whisker-subsecond" {
            plan.subsecond.clone().unwrap_or_else(|| fork.clone())
        } else {
            plan.version.clone()
        };
    }
    for path in manifests(root)? {
        let mut doc = read_toml(&path)?;
        if path == root.join("Cargo.toml") {
            doc["workspace"]["package"]["version"] = value(&plan.version);
        }
        if let Some(name) = doc
            .get("package")
            .and_then(|item| item.get("name"))
            .and_then(Item::as_str)
        {
            if let Some(next) = versions.get(name) {
                if doc["package"]["version"].as_str().is_some() {
                    doc["package"]["version"] = value(next);
                }
            }
        }
        update_path_dependencies(doc.as_table_mut(), &versions);
        fs::write(path, doc.to_string())?;
    }
    for (stream, selected) in [
        ("sdk", &plan.sdk),
        ("gradle", &plan.gradle),
        ("ios", &plan.ios),
    ] {
        if let Some(selected) = selected {
            native::update_pin(root, stream, selected)?;
        }
    }
    Ok(())
}

// Only local path dependencies are aligned. Registry dependencies with the same
// spelling, renamed dependencies, and independent fork versions remain valid.
fn update_path_dependencies(table: &mut dyn TableLike, versions: &BTreeMap<String, String>) {
    for (key, item) in table.iter_mut() {
        if let Some(nested) = item.as_table_like_mut() {
            let name = nested.get("package").and_then(Item::as_str).unwrap_or(&key);
            if let Some(next) = versions.get(name) {
                if nested.contains_key("path") && nested.contains_key("version") {
                    nested.insert("version", value(next));
                }
            }
            update_path_dependencies(nested, versions);
        }
    }
}

fn release_notes(root: &Path, plan: &ReleasePlan, previous: &str) -> Result<String> {
    let previous_tag = format!("whisker-v{previous}");
    ensure!(
        remote_tag(root, &previous_tag)?.is_some(),
        "missing previous release tag {previous_tag}; establish a release baseline first"
    );
    let commits = git(
        root,
        &[
            "log",
            "--first-parent",
            "--format=- %s (%h)",
            &format!("{previous_tag}..{}", plan.source),
        ],
    )?;
    let mut notes = format!(
        "# Whisker {}\n\nOne release for the Rust workspace and selected native SDKs.\n\n| Stream | Version |\n| --- | --- |\n| Rust | {} |\n",
        plan.version, plan.version
    );
    for stream in ["sdk", "gradle", "ios"] {
        notes.push_str(&format!("| {stream} | {} |\n", native::pin(root, stream)?));
    }
    notes.push_str(&format!("| whisker-subsecond | {} |\n\n## Changes\n\n{commits}\n## Publishing\n\nAfter this PR is approved and merged, the selected native SDKs publish first, then all unpublished Rust versions. A single GitHub Release is created after verification.\n", plan.crates["whisker-subsecond"]));
    Ok(notes)
}

#[cfg(test)]
pub(super) fn update_test_manifest(text: &str, versions: &BTreeMap<String, String>) -> String {
    let mut doc: DocumentMut = text.parse().unwrap();
    update_path_dependencies(doc.as_table_mut(), versions);
    doc.to_string()
}
