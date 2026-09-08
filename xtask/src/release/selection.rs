use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use toml_edit::{Item, Table, TableLike, value};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct SelectivePlan {
    pub id: String,
    pub baseline: String,
    pub packages: BTreeMap<String, String>,
    pub publish: BTreeSet<String>,
    pub reasons: BTreeMap<String, String>,
    pub contents: BTreeMap<String, String>,
    pub lockfile: String,
}

impl SelectivePlan {
    pub fn validate(&self, root: &Path, versions: &BTreeMap<String, String>) -> Result<()> {
        validate_id(&self.id)?;
        ensure!(!self.publish.is_empty(), "release has no selected crates");
        ensure!(
            self.publish.iter().all(|name| versions.contains_key(name)),
            "release selects an unknown crate"
        );
        ensure!(
            self.baseline.len() == 40 && self.baseline.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid release baseline"
        );
        git(
            root,
            &["merge-base", "--is-ancestor", &self.baseline, "HEAD"],
        )?;
        let previous: ReleasePlan = serde_json::from_str(&git(
            root,
            &["show", &format!("{}:{PLAN_PATH}", self.baseline)],
        )?)?;
        ensure!(
            previous
                .crates
                .keys()
                .all(|name| versions.contains_key(name)),
            "a published crate was removed from the release inventory"
        );
        let updated: BTreeSet<_> = versions
            .iter()
            .filter(|(name, next)| previous.crates.get(*name) != Some(*next))
            .map(|(name, _)| name.clone())
            .collect();
        ensure!(
            updated == self.publish,
            "publication selection does not match changed package versions"
        );
        for name in &updated {
            if let Some(old) = previous.crates.get(name) {
                ensure!(
                    version(&versions[name])? > version(old)?,
                    "{name} must use a newer version than {old}"
                );
            }
        }
        ensure!(
            self.contents == Inventory::read(root)?.contents(root)?,
            "package contents changed after release preparation; prepare a new release"
        );
        ensure!(
            self.lockfile == lockfile(root)?,
            "Cargo.lock changed after release preparation; prepare a new release"
        );
        Ok(())
    }
}

pub(super) fn validate_id(id: &str) -> Result<()> {
    let (date, sequence) = id
        .split_once('.')
        .context("release_id must look like 20260908.1")?;
    ensure!(
        date.len() == 8
            && date.bytes().all(|b| b.is_ascii_digit())
            && !sequence.is_empty()
            && sequence.bytes().all(|b| b.is_ascii_digit())
            && sequence.parse::<u32>().is_ok_and(|n| n > 0),
        "release_id must look like 20260908.1"
    );
    Ok(())
}

pub(super) fn lockfile(root: &Path) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(fs::read(root.join("Cargo.lock"))?)
    ))
}

pub(super) struct Baseline {
    directory: tempfile::TempDir,
    repository: PathBuf,
}

impl Baseline {
    pub fn new(root: &Path, commit: &str) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        git(
            root,
            &[
                "worktree",
                "add",
                "--detach",
                directory.path().to_str().context("baseline path")?,
                commit,
            ],
        )?;
        Ok(Self {
            directory,
            repository: root.to_owned(),
        })
    }

    pub fn path(&self) -> &Path {
        self.directory.path()
    }
}

impl Drop for Baseline {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .current_dir(&self.repository)
            .args(["worktree", "remove", "--force"])
            .arg(self.directory.path())
            .output();
    }
}

pub(super) struct Package {
    pub version: String,
    pub manifest: PathBuf,
    pub group: String,
    metadata: serde_json::Value,
}

pub(super) struct Inventory {
    pub packages: BTreeMap<String, Package>,
}

impl Inventory {
    pub fn read(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let data = metadata(&root)?;
        let members = data["workspace_members"]
            .as_array()
            .context("workspace members")?;
        let mut packages = BTreeMap::new();
        for package in data["packages"].as_array().context("workspace packages")? {
            if !members.contains(&package["id"]) || package["publish"] == serde_json::json!([]) {
                continue;
            }
            ensure!(
                package["publish"].is_null()
                    || package["publish"] == serde_json::json!(["crates-io"]),
                "only crates.io is supported"
            );
            let name = package["name"].as_str().context("package name")?.to_owned();
            let manifest =
                PathBuf::from(package["manifest_path"].as_str().context("manifest path")?);
            let relative = manifest.strip_prefix(&root)?;
            let group = if name == "whisker-subsecond" {
                name.clone()
            } else if relative.starts_with("packages") {
                relative
                    .iter()
                    .nth(1)
                    .context("package group")?
                    .to_string_lossy()
                    .into_owned()
            } else {
                "core".into()
            };
            packages.insert(
                name,
                Package {
                    version: package["version"]
                        .as_str()
                        .context("package version")?
                        .into(),
                    manifest,
                    group,
                    metadata: package.clone(),
                },
            );
        }
        ensure!(!packages.is_empty(), "no publishable packages");
        Ok(Self { packages })
    }

    pub fn contents(&self, root: &Path) -> Result<BTreeMap<String, String>> {
        let root = root.canonicalize()?;
        self.packages
            .iter()
            .map(|(name, package)| {
                let list = crate::capture(Command::new(crate::cargo()).current_dir(&root).args([
                    "package",
                    "--list",
                    "--allow-dirty",
                    "-p",
                    name,
                ]))?;
                let directory = package.manifest.parent().context("crate directory")?;
                let mut hash = Sha256::new();
                hash_part(&mut hash, &manifest_contents(package, &root)?);
                for path in list.lines().collect::<BTreeSet<_>>() {
                    if matches!(
                        path,
                        "Cargo.toml" | "Cargo.toml.orig" | "Cargo.lock" | ".cargo_vcs_info.json"
                    ) {
                        continue;
                    }
                    let mut source = directory.join(path);
                    if !source.is_file() {
                        for field in ["readme", "license_file"] {
                            if let Some(external) = package.metadata[field].as_str() {
                                if Path::new(external).file_name() == Path::new(path).file_name() {
                                    source = directory.join(external);
                                    break;
                                }
                            }
                        }
                    }
                    hash_part(&mut hash, path.as_bytes());
                    let contents = if let Some(nested) = self
                        .packages
                        .values()
                        .find(|package| package.manifest == source)
                    {
                        manifest_contents(nested, &root)?
                    } else {
                        fs::read(&source)
                            .with_context(|| format!("read packaged file {}", source.display()))?
                    };
                    hash_part(&mut hash, &contents);
                }
                Ok((name.clone(), format!("{:x}", hash.finalize())))
            })
            .collect()
    }

    pub fn versions(&self) -> BTreeMap<String, String> {
        self.packages
            .iter()
            .map(|(name, package)| (name.clone(), package.version.clone()))
            .collect()
    }

    pub fn select(
        &self,
        baseline: &Inventory,
        changed: &BTreeSet<String>,
        core: Option<&str>,
        fork: Option<&str>,
        overrides: &BTreeMap<String, String>,
    ) -> Result<(BTreeMap<String, String>, BTreeMap<String, String>)> {
        let mut group_versions = BTreeMap::new();
        for package in self.packages.values() {
            if let Some(existing) = group_versions.insert(&package.group, &package.version) {
                ensure!(
                    existing == &package.version,
                    "all crates in group {} must share a version",
                    package.group
                );
            }
        }
        let mut groups = BTreeMap::new();
        for name in changed {
            groups.insert(
                self.packages[name].group.clone(),
                "package contents changed".to_owned(),
            );
        }
        for group in overrides.keys() {
            ensure!(
                group != "core"
                    && group != "whisker-subsecond"
                    && self.packages.values().any(|p| &p.group == group),
                "unknown package group {group}"
            );
            groups.insert(group.clone(), "explicit package version".into());
        }
        if core.is_some() {
            groups.insert("core".into(), "core release".into());
        }
        if fork.is_some() {
            groups.insert("whisker-subsecond".into(), "fork release".into());
        }
        loop {
            ensure!(
                !groups.contains_key("core") || core.is_some(),
                "core has changes; specify version (the new core version)"
            );
            ensure!(
                !groups.contains_key("whisker-subsecond") || fork.is_some(),
                "whisker-subsecond has changes; specify subsecond_version"
            );
            let mut versions = self.versions();
            for (name, package) in &self.packages {
                if !groups.contains_key(&package.group) {
                    continue;
                }
                let requested = match package.group.as_str() {
                    "core" => core.unwrap(),
                    "whisker-subsecond" => fork.unwrap(),
                    group => overrides.get(group).map(String::as_str).unwrap_or("patch"),
                };
                let next = next_version(
                    &package.version,
                    requested,
                    !baseline
                        .packages
                        .values()
                        .any(|old| old.group == package.group),
                )?;
                versions.insert(name.clone(), next);
            }
            let mut expanded = false;
            for (name, package) in &self.packages {
                if groups.contains_key(&package.group) {
                    continue;
                }
                for dependency in package.metadata["dependencies"]
                    .as_array()
                    .context("dependencies")?
                {
                    if dependency["path"].is_null() {
                        continue;
                    }
                    let target = dependency["name"].as_str().context("dependency name")?;
                    if let Some(next) = versions.get(target) {
                        let requirement = semver::VersionReq::parse(
                            dependency["req"].as_str().context("dependency version")?,
                        )?;
                        if !requirement.matches(&version(next)?) {
                            groups.insert(
                                package.group.clone(),
                                format!("{name} requires an update for {target} {next}"),
                            );
                            expanded = true;
                        }
                    }
                }
            }
            if !expanded {
                let reasons = self
                    .packages
                    .iter()
                    .filter_map(|(name, p)| {
                        groups
                            .get(&p.group)
                            .map(|reason| (name.clone(), reason.clone()))
                    })
                    .collect();
                return Ok((versions, reasons));
            }
        }
    }
}

fn manifest_contents(package: &Package, root: &Path) -> Result<Vec<u8>> {
    let mut manifest = package.metadata.clone();
    let object = manifest.as_object_mut().context("package metadata")?;
    object.remove("id");
    object.remove("manifest_path");
    for dependency in object["dependencies"]
        .as_array_mut()
        .context("dependencies")?
    {
        dependency
            .as_object_mut()
            .context("dependency")?
            .remove("path");
    }
    let targets = object["targets"].as_array_mut().context("targets")?;
    targets.retain(|target| {
        !target["kind"].as_array().is_some_and(|kinds| {
            kinds
                .iter()
                .any(|kind| matches!(kind.as_str(), Some("test" | "bench" | "example")))
        })
    });
    for target in targets {
        let source = Path::new(target["src_path"].as_str().context("target source")?);
        target["src_path"] = source.strip_prefix(root)?.to_string_lossy().as_ref().into();
    }
    Ok(serde_json::to_vec(&manifest)?)
}

fn hash_part(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn next_version(current: &str, request: &str, new: bool) -> Result<String> {
    let old = version(current)?;
    let mut next = old.clone();
    match request {
        "patch" if new => {}
        "patch" => next.patch += 1,
        "minor" => {
            next.minor += 1;
            next.patch = 0;
        }
        "major" => {
            next.major += 1;
            next.minor = 0;
            next.patch = 0;
        }
        explicit => next = version(explicit)?,
    }
    ensure!(
        next > old || (new && next == old),
        "new version {next} must be newer than {old}"
    );
    Ok(next.to_string())
}

pub(super) fn detach_packages(root: &Path) -> Result<()> {
    let inventory = Inventory::read(root)?;
    let workspace = read_toml(&root.join("Cargo.toml"))?;
    let dependencies = workspace["workspace"]["dependencies"]
        .as_table_like()
        .context("workspace dependencies")?;
    let root = root.canonicalize()?;
    for package in inventory
        .packages
        .values()
        .filter(|p| p.group != "core" && p.group != "whisker-subsecond")
    {
        let mut doc = read_toml(&package.manifest)?;
        doc["package"]["version"] = value(&package.version);
        detach_dependencies(
            doc.as_table_mut(),
            dependencies,
            &root,
            package.manifest.parent().unwrap(),
        )?;
        fs::write(&package.manifest, doc.to_string())?;
    }
    Ok(())
}

fn detach_dependencies(
    table: &mut dyn TableLike,
    workspace: &dyn TableLike,
    root: &Path,
    directory: &Path,
) -> Result<()> {
    for (key, item) in table.iter_mut() {
        if matches!(
            &*key,
            "dependencies" | "dev-dependencies" | "build-dependencies"
        ) {
            if let Some(dependencies) = item.as_table_like_mut() {
                for (name, item) in dependencies.iter_mut() {
                    if item.get("workspace").and_then(Item::as_bool) != Some(true) {
                        continue;
                    }
                    let Some(inherited) = workspace.get(&name).and_then(Item::as_table_like) else {
                        continue;
                    };
                    let Some(path) = inherited.get("path").and_then(Item::as_str) else {
                        continue;
                    };
                    let mut local = Table::new();
                    for (key, val) in inherited.iter() {
                        local.insert(key, val.clone());
                    }
                    local.insert("path", value(relative_path(directory, &root.join(path))?));
                    let overrides = item.as_table_like().context("inherited dependency")?;
                    for (key, val) in overrides.iter().filter(|(key, _)| *key != "workspace") {
                        if key == "features" {
                            let mut features = local
                                .get("features")
                                .and_then(Item::as_array)
                                .cloned()
                                .unwrap_or_default();
                            for feature in val.as_array().context("dependency features")?.iter() {
                                if !features
                                    .iter()
                                    .any(|existing| existing.as_str() == feature.as_str())
                                {
                                    features.push(feature.clone());
                                }
                            }
                            local.insert(key, value(features));
                        } else {
                            local.insert(key, val.clone());
                        }
                    }
                    *item = value(local.into_inline_table());
                }
            }
        } else if let Some(nested) = item.as_table_like_mut() {
            detach_dependencies(nested, workspace, root, directory)?;
        }
    }
    Ok(())
}

fn relative_path(from: &Path, to: &Path) -> Result<String> {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    ensure!(
        common > 0,
        "dependency path is outside the repository volume"
    );
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for component in &to[common..] {
        path.push(component.as_os_str());
    }
    Ok(path.to_string_lossy().replace('\\', "/"))
}
