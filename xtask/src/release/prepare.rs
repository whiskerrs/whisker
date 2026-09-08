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
    let (mut plan, core) = request(root)?;
    let branch = format!("codex/release-{}", plan.label());
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
        let previous_selection = previous
            .selective
            .as_ref()
            .context("existing release is not selective")?;
        ensure!(
            plan.selective.as_ref().unwrap().id == previous_selection.id
                && plan.selective.as_ref().unwrap().packages == previous_selection.packages,
            "existing release has a different identifier or package overrides"
        );
        plan.crates = previous.crates.clone();
        plan.selective = previous.selective.clone();
        ensure!(
            plan == previous,
            "{branch} already has a different release plan; reuse the original run or choose another release_id"
        );
        plan.validate_checkout(root)?;
    } else {
        if let Some(next) = &core {
            ensure!(
                version(next)?
                    > version(
                        read_toml(&root.join("Cargo.toml"))?["workspace"]["package"]["version"]
                            .as_str()
                            .context("workspace version")?
                    )?,
                "core version must be newer than the current workspace version"
            );
        }
        let previous = ReleasePlan::read(root)?;
        let previous_tag = previous.tag();
        let baseline = remote_tag(root, &previous_tag)?.with_context(|| {
            format!(
                "previous release {previous_tag} is incomplete; resume its publishing job first"
            )
        })?;
        git(root, &["fetch", "origin", "tag", &previous_tag])?;
        git(
            root,
            &["merge-base", "--is-ancestor", &baseline, &plan.source],
        )?;
        plan.selective.as_mut().unwrap().baseline = baseline;
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
                .is_some_and(|name| name.starts_with("codex/release-"))),
            "another release PR is open; finish or close it first"
        );
        git(root, &["switch", "-c", &branch])?;
        prepare_selection(root, &mut plan, core.as_deref())?;
        let notes = release_notes(root, &plan, &previous_tag)?;
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
            &["commit", "-m", &format!("chore: release {}", plan.label())],
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
                &format!("chore: release {}", plan.label()),
                "--body-file",
                &plan.notes_path(),
            ],
        )?
        .trim()
        .to_owned()
    };
    gh(root, &["workflow", "run", "ci.yml", "--ref", &branch])?;
    output("pr", &url)?;
    output("branch", &branch)?;
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        writeln!(
            OpenOptions::new().append(true).open(path)?,
            "Release PR: {url}\n\nReview selected packages and version bumps before merging. Selected native SDKs and Rust crates publish automatically, followed by one GitHub Release."
        )?;
    }
    Ok(())
}

fn request(root: &Path) -> Result<(ReleasePlan, Option<String>)> {
    let id = std::env::var("RELEASE_ID").context("RELEASE_ID is required (e.g. 20260908.1)")?;
    selection::validate_id(&id)?;
    let core = optional("RELEASE_VERSION")?;
    let old = read_toml(&root.join("Cargo.toml"))?["workspace"]["package"]["version"]
        .as_str()
        .context("workspace version")?
        .to_owned();
    let packages: BTreeMap<String, String> = serde_json::from_str(
        &std::env::var("PACKAGE_VERSIONS").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| "{}".into())
    ).context("package_versions must be a JSON object mapping package groups to patch/minor/major or a version")?;
    let plan = ReleasePlan {
        version: core.clone().unwrap_or_else(|| old.clone()),
        source: git(root, &["rev-parse", "HEAD"])?.trim().into(),
        sdk: optional("SDK_VERSION")?,
        gradle: optional("GRADLE_VERSION")?,
        ios: optional("IOS_VERSION")?,
        subsecond: optional("SUBSECOND_VERSION")?,
        crates: BTreeMap::new(),
        selective: Some(selection::SelectivePlan {
            id: id.clone(),
            baseline: String::new(),
            packages,
            publish: Default::default(),
            reasons: BTreeMap::new(),
            contents: BTreeMap::new(),
            lockfile: String::new(),
        }),
    };
    Ok((plan, core))
}

pub(super) fn preview(root: &Path) -> Result<()> {
    let (mut plan, core) = request(root)?;
    let tag = ReleasePlan::read(root)?.tag();
    let commit = git(root, &["rev-parse", &format!("{tag}^{{commit}}")])?
        .trim()
        .to_owned();
    plan.selective.as_mut().unwrap().baseline = commit;
    let checkout = selection::Baseline::new(root, &plan.source)?;
    prepare_selection(checkout.path(), &mut plan, core.as_deref())?;
    println!("{}", serde_json::to_string_pretty(&plan)?);
    Ok(())
}

pub(super) fn prepare_selection(
    root: &Path,
    plan: &mut ReleasePlan,
    core: Option<&str>,
) -> Result<()> {
    let selected = plan.selective.as_ref().context("selective plan")?;
    let baseline = selection::Baseline::new(root, &selected.baseline)?;
    let previous = selection::Inventory::read(baseline.path())?;
    let previous_contents = previous.contents(baseline.path())?;
    selection::detach_packages(root)?;
    for (stream, next) in [
        ("sdk", &plan.sdk),
        ("gradle", &plan.gradle),
        ("ios", &plan.ios),
    ] {
        if let Some(next) = next {
            native::update_pin(root, stream, next)?;
        }
    }
    let current = selection::Inventory::read(root)?;
    ensure!(
        previous
            .packages
            .keys()
            .all(|name| current.packages.contains_key(name)),
        "removing published crates requires an explicit release migration"
    );
    for (name, package) in &current.packages {
        if let Some(old) = previous.packages.get(name) {
            ensure!(
                package.version == old.version,
                "{name} version changed outside release preparation; use package_versions or version inputs"
            );
        }
    }
    let contents = current.contents(root)?;
    let changed = contents
        .iter()
        .filter(|(name, content)| previous_contents.get(*name) != Some(*content))
        .map(|(name, _)| name.clone())
        .collect();
    let (versions, reasons) = current.select(
        &previous,
        &changed,
        core,
        plan.subsecond.as_deref(),
        &selected.packages,
    )?;
    ensure!(
        !reasons.is_empty(),
        "no unpublished package changes or requested versions"
    );
    plan.crates = versions;
    let selected = plan.selective.as_mut().unwrap();
    selected.publish = reasons.keys().cloned().collect();
    selected.reasons = reasons;
    stamp_versions(root, plan)?;
    crate::run(
        Command::new(crate::cargo())
            .current_dir(root)
            .args(["update", "--workspace"]),
    )?;
    let final_inventory = selection::Inventory::read(root)?;
    ensure!(
        final_inventory.versions() == plan.crates,
        "stamped package versions differ from selection"
    );
    let selected = plan.selective.as_mut().unwrap();
    selected.contents = final_inventory.contents(root)?;
    for (name, content) in &selected.contents {
        ensure!(
            selected.publish.contains(name) || previous_contents.get(name) == Some(content),
            "unselected package {name} changed while preparing dependencies; include it in package_versions"
        );
    }
    selected.lockfile = selection::lockfile(root)?;
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
    if plan.selective.is_some() {
        return stamp_selected_versions(root, plan);
    }
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

fn release_notes(root: &Path, plan: &ReleasePlan, previous_tag: &str) -> Result<String> {
    let selected = plan.selective.as_ref().context("selective notes")?;
    let previous_plan: ReleasePlan = serde_json::from_str(&git(
        root,
        &["show", &format!("{previous_tag}:{PLAN_PATH}")],
    )?)?;
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
        "# Whisker {}\n\nCore version: {}. One release for the selected Rust packages and native SDKs.\n\n| Crate | Previous | Next | Reason |\n| --- | --- | --- | --- |\n",
        plan.label(),
        plan.version
    );
    for name in &selected.publish {
        notes.push_str(&format!(
            "| {name} | {} | {} | {} |\n",
            previous_plan
                .crates
                .get(name)
                .map(String::as_str)
                .unwrap_or("new"),
            plan.crates[name],
            selected.reasons[name]
        ));
    }
    notes.push_str("\nUnlisted Rust packages retain their published versions. Automatic patch bumps are proposals; review API compatibility before merging.\n\n| Native stream | Version | Publication |\n| --- | --- | --- |\n");
    for (stream, next) in [
        ("sdk", &plan.sdk),
        ("gradle", &plan.gradle),
        ("ios", &plan.ios),
    ] {
        notes.push_str(&format!(
            "| {stream} | {} | {} |\n",
            native::pin(root, stream)?,
            if next.is_some() { "publish" } else { "reuse" }
        ));
    }
    notes.push_str(&format!("\n## Changes\n\n{commits}\n## Publishing\n\nMerging publishes the selected native SDKs, then the selected Rust crates in dependency order. Failed jobs resume only missing versions. One GitHub Release is created after verification.\n"));
    Ok(notes)
}

fn stamp_selected_versions(root: &Path, plan: &ReleasePlan) -> Result<()> {
    let inventory = selection::Inventory::read(root)?;
    let selected = plan.selective.as_ref().unwrap();
    let core: std::collections::BTreeSet<_> = inventory
        .packages
        .iter()
        .filter(|(_, package)| package.group == "core")
        .map(|(name, _)| name.clone())
        .collect();
    let core_selected = selected.publish.iter().any(|name| core.contains(name));
    for path in manifests(root)? {
        let mut doc = read_toml(&path)?;
        let name = doc
            .get("package")
            .and_then(|item| item.get("name"))
            .and_then(Item::as_str)
            .map(str::to_owned);
        let is_root = path == root.join("Cargo.toml");
        let is_selected = name
            .as_ref()
            .is_some_and(|name| selected.publish.contains(name));
        if name
            .as_ref()
            .is_some_and(|name| inventory.packages.contains_key(name))
            && !is_selected
        {
            continue;
        }
        if is_root {
            doc["workspace"]["package"]["version"] = value(&plan.version);
        }
        if let Some(next) = name.as_ref().and_then(|name| plan.crates.get(name)) {
            if doc["package"]["version"].as_str().is_some() {
                doc["package"]["version"] = value(next);
            }
        }
        let exact_core =
            core_selected && (is_root || name.as_ref().is_some_and(|name| core.contains(name)));
        stamp_dependencies(
            doc.as_table_mut(),
            &plan.crates,
            &core,
            exact_core,
            is_selected,
        )?;
        fs::write(path, doc.to_string())?;
    }
    Ok(())
}

fn stamp_dependencies(
    table: &mut dyn TableLike,
    versions: &BTreeMap<String, String>,
    core: &std::collections::BTreeSet<String>,
    exact_core: bool,
    force: bool,
) -> Result<()> {
    for (key, item) in table.iter_mut() {
        if let Some(nested) = item.as_table_like_mut() {
            let name = nested
                .get("package")
                .and_then(Item::as_str)
                .unwrap_or(&key)
                .to_owned();
            if let (Some(next), Some(current)) = (
                versions.get(&name),
                nested.get("version").and_then(Item::as_str),
            ) {
                if nested.contains_key("path") {
                    let matches = semver::VersionReq::parse(current)?.matches(&version(next)?);
                    if force || !matches || (exact_core && core.contains(&name)) {
                        let next = if exact_core && core.contains(&name) {
                            format!("={next}")
                        } else {
                            next.clone()
                        };
                        nested.insert("version", value(next));
                    }
                }
            }
            stamp_dependencies(nested, versions, core, exact_core, force)?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn update_test_manifest(text: &str, versions: &BTreeMap<String, String>) -> String {
    let mut doc: DocumentMut = text.parse().unwrap();
    update_path_dependencies(doc.as_table_mut(), versions);
    doc.to_string()
}
