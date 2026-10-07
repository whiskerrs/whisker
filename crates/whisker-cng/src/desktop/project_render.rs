//! Lower composed declarations to staged files plus an explicit distribution plan.
use super::*;
use crate::project_files::{check_destination, insert, stage_declared};
use std::collections::{BTreeMap, BTreeSet};
use whisker_plugin::FileEntry;
const PLAN: &str = ".whisker/desktop-build.json";

#[derive(Debug, Clone, serde::Serialize)]
pub struct DesktopProjectInputs {
    pub project: ProjectIr,
    pub app_crate_dir: Option<PathBuf>,
    pub cargo_selection: crate::CargoSelection,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopBuildPlan {
    pub version: u32,
    pub platform: crate::GenerationTarget,
    pub application: String,
    pub executables: BTreeMap<String, DesktopExecutable>,
    /// Distribution-relative destination -> staged source, excluding executables.
    pub files: BTreeMap<ProjectPath, ProjectPath>,
    pub cargo_selection: crate::CargoSelection,
}
impl DesktopBuildPlan {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported desktop plan; regenerate project"
        );
        ensure!(
            matches!(
                self.platform,
                crate::GenerationTarget::Windows | crate::GenerationTarget::Linux
            ),
            "invalid desktop platform"
        );
        self.cargo_selection.validate_platform(self.platform)?;
        ensure!(
            self.cargo_selection.target.is_some(),
            "desktop plan needs an explicit Cargo target"
        );
        ensure!(
            self.executables.contains_key(&self.application),
            "missing desktop application"
        );
        let mut paths = BTreeSet::new();
        for p in self
            .files
            .keys()
            .chain(self.executables.values().map(|e| &e.destination))
        {
            let p = if self.platform == crate::GenerationTarget::Windows {
                p.as_str().to_lowercase()
            } else {
                p.as_str().into()
            };
            ensure!(paths.insert(p), "duplicate distribution output");
        }
        for p in &paths {
            for (i, _) in p.match_indices('/') {
                ensure!(
                    !paths.contains(&p[..i]),
                    "overlapping distribution output {p}"
                );
            }
        }
        for exe in self.executables.values() {
            if let ExecutableSource::Cargo(rust) = &exe.source {
                ensure!(
                    rust.kind == RustArtifactKind::Bin && !rust.package.is_empty(),
                    "desktop Cargo artifact must be a named bin"
                );
                ensure!(
                    !rust.target.contains('/') && !rust.target.starts_with('-'),
                    "invalid Cargo binary name"
                );
                ProjectPath::new(&rust.target)?;
            }
        }
        if self.platform == crate::GenerationTarget::Windows {
            ProjectIr::Windows(WindowsProjectIr {
                application: self.application.clone(),
                executables: self
                    .executables
                    .iter()
                    .map(|(id, e)| {
                        (
                            id.clone(),
                            WindowsExecutable {
                                executable: e.clone(),
                                manifest: None,
                                icon: None,
                                version_info: None,
                                resource_scripts: vec![],
                            },
                        )
                    })
                    .collect(),
                resources: self
                    .files
                    .iter()
                    .map(|(dest, src)| Resource {
                        source: src.clone(),
                        destination: dest.clone(),
                        kind: ResourceKind::File,
                    })
                    .collect(),
                ..Default::default()
            })
            .validate_structure()?;
        }
        Ok(())
    }
}
pub fn load_build_plan(root: &Path) -> Result<DesktopBuildPlan> {
    let plan: DesktopBuildPlan = serde_json::from_slice(
        &std::fs::read(root.join(PLAN)).context("read desktop build plan; regenerate project")?,
    )?;
    plan.validate()?;
    Ok(plan)
}
pub fn distribution_files(
    root: &Path,
    plan: &DesktopBuildPlan,
) -> Result<BTreeMap<ProjectPath, FileEntry>> {
    plan.validate()?;
    let mut files = BTreeMap::new();
    let declared = plan
        .files
        .iter()
        .map(|(dest, source)| {
            (
                dest.clone(),
                ProjectFile::AppFile {
                    source: source.clone(),
                },
            )
        })
        .collect();
    stage_declared(&mut files, &declared, Some(root))?;
    crate::project_files::validate(&files)?;
    Ok(files)
}
pub fn render_project(inputs: &DesktopProjectInputs) -> Result<BTreeMap<ProjectPath, FileEntry>> {
    inputs.project.validate_structure()?;
    let (platform, application, executables, resources, declared) = match &inputs.project {
        ProjectIr::Windows(w) => {
            ensure!(
                w.packages.is_empty() && w.application_package.is_none(),
                "MSIX packaging is not supported by the Cargo distribution backend"
            );
            (
                crate::GenerationTarget::Windows,
                &w.application,
                w.executables
                    .iter()
                    .map(|(id, e)| (id.clone(), e.executable.clone()))
                    .collect(),
                &w.resources,
                &w.files,
            )
        }
        ProjectIr::Linux(l) => {
            ensure!(
                l.packages.is_empty(),
                "Linux package backends (Flatpak/recipes) are not connected; use the installation tree"
            );
            (
                crate::GenerationTarget::Linux,
                &l.application,
                l.executables.clone(),
                &l.resources,
                &l.files,
            )
        }
        _ => anyhow::bail!("desktop renderer requires Windows or Linux"),
    };
    let mut files = BTreeMap::new();
    stage_declared(&mut files, declared, inputs.app_crate_dir.as_deref())?;
    for path in files.keys() {
        let first = path.as_str().split('/').next().unwrap();
        let first = if platform == crate::GenerationTarget::Windows {
            first.to_ascii_lowercase()
        } else {
            first.into()
        };
        ensure!(
            !matches!(
                first.as_str(),
                "target" | ".whisker" | ".whisker-fingerprint"
            ),
            "reserved desktop staging path {}",
            path.as_str()
        );
    }
    let mut plan = DesktopBuildPlan {
        version: 1,
        platform,
        application: application.clone(),
        executables,
        files: BTreeMap::new(),
        cargo_selection: inputs.cargo_selection.for_platform(platform),
    };
    for resource in resources {
        add_resource(&mut plan.files, &files, resource)?;
    }
    if let ProjectIr::Linux(l) = &inputs.project {
        let mut metadata = BTreeMap::new();
        for (path, entry) in &l.desktop_entries {
            metadata.insert(path.clone(), super::metadata::desktop(entry)?);
        }
        for (path, xml) in l.mime_packages.iter().chain(&l.metainfo) {
            metadata.insert(
                path.clone(),
                crate::project_xml::xml(xml, &BTreeMap::new())?,
            );
        }
        for (path, service) in &l.dbus_services {
            metadata.insert(path.clone(), super::metadata::dbus(service)?);
        }
        for (i, (destination, text)) in metadata.into_iter().enumerate() {
            let source = ProjectPath::new(format!(".whisker/metadata/{i}"))?;
            insert(&mut files, source.clone(), FileEntry::text(text))?;
            ensure!(
                plan.files.insert(destination, source).is_none(),
                "duplicate metadata destination"
            );
        }
    }
    // A manifest is a package, not a binary: group Windows resources by package
    // and use compile_for to attach them only to the declared bin.
    let mut scripts: BTreeMap<ProjectPath, Vec<(String, String)>> = BTreeMap::new();
    if let ProjectIr::Windows(w) = &inputs.project {
        for (index, exe) in w.executables.values().enumerate() {
            if exe.manifest.is_none()
                && exe.icon.is_none()
                && exe.version_info.is_none()
                && exe.resource_scripts.is_empty()
            {
                continue;
            }
            let ExecutableSource::Cargo(rust) = &exe.executable.source else {
                anyhow::bail!("cannot embed metadata into a prebuilt Windows executable");
            };
            ensure!(
                rust.manifest.as_str() == "Cargo.toml",
                "Windows resource embedding currently requires the root Cargo manifest"
            );
            for path in exe.icon.iter().chain(&exe.resource_scripts) {
                ensure!(
                    files.contains_key(path),
                    "missing Windows resource input {}",
                    path.as_str()
                );
            }
            let manifest_path = if let Some(xml) = &exe.manifest {
                let path = format!(".whisker/windows/{index}.manifest");
                insert(
                    &mut files,
                    ProjectPath::new(&path)?,
                    FileEntry::text(crate::project_xml::xml(xml, &BTreeMap::new())?),
                )?;
                Some(path)
            } else {
                None
            };
            // embed-resource invokes LLVM RC from the script directory. Keep
            // the entry script at the project root so resource filenames have
            // the same base with RC.EXE, windres, and LLVM RC.
            let path = format!(".whisker-windows-{index}.rc");
            insert(
                &mut files,
                ProjectPath::new(&path)?,
                FileEntry::text(super::metadata::rc(exe, manifest_path.as_deref())?),
            )?;
            scripts
                .entry(rust.manifest.clone())
                .or_default()
                .push((rust.target.clone(), path));
        }
    }
    for (manifest_path, binaries) in scripts {
        let bytes = files
            .get(&manifest_path)
            .context("missing Cargo manifest for Windows resources")?
            .to_bytes()?;
        let mut manifest: toml::Value = std::str::from_utf8(&bytes)?.parse()?;
        let package = manifest
            .get_mut("package")
            .and_then(toml::Value::as_table_mut)
            .context("Windows resources need a package manifest")?;
        ensure!(
            !package.contains_key("build") && !files.contains_key(&ProjectPath::new("build.rs")?),
            "Windows resource backend owns build.rs; custom build scripts need another backend"
        );
        package.insert("build".into(), toml::Value::String("build.rs".into()));
        let deps = manifest
            .as_table_mut()
            .unwrap()
            .entry("build-dependencies")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .context("invalid build dependencies")?;
        ensure!(
            !deps.contains_key("embed-resource"),
            "Windows resource backend owns embed-resource dependency"
        );
        deps.insert("embed-resource".into(), toml::Value::String("3.0".into()));
        files.insert(manifest_path, FileEntry::text(toml::to_string(&manifest)?));
        let mut script = String::from(
            "fn main() {\n    println!(\"cargo:rerun-if-changed=.whisker/windows\");\n",
        );
        // Include staged files as rerun inputs so custom RC includes also rebuild.
        for path in files.keys() {
            script += &format!(
                "    println!(\"{{}}\", {:?});\n",
                format!("cargo:rerun-if-changed={}", path.as_str())
            );
        }
        for (bin, path) in binaries {
            script += &format!(
                "    embed_resource::compile_for({path:?}, &[{bin:?}], embed_resource::ParamsIncludeDirs(&[\".\"])).manifest_required().expect(\"compile Windows resources\");\n"
            );
        }
        script += "}\n";
        insert(
            &mut files,
            ProjectPath::new("build.rs")?,
            FileEntry::text(script),
        )?;
    }
    for exe in plan.executables.values() {
        match &exe.source {
            ExecutableSource::Prebuilt(path) => {
                ensure!(
                    files.contains_key(path),
                    "missing prebuilt executable {}",
                    path.as_str()
                );
            }
            ExecutableSource::Cargo(rust) => {
                let bytes = files
                    .get(&rust.manifest)
                    .context("missing staged Cargo manifest")?
                    .to_bytes()?;
                let manifest: toml::Value = std::str::from_utf8(&bytes)?.parse()?;
                ensure!(
                    manifest
                        .get("package")
                        .and_then(|p| p.get("name"))
                        .and_then(toml::Value::as_str)
                        == Some(&rust.package),
                    "Cargo executable package must match its manifest"
                );
            }
        }
    }
    plan.validate()?;
    insert(
        &mut files,
        ProjectPath::new(PLAN)?,
        FileEntry::text(serde_json::to_string_pretty(&plan)?),
    )?;
    crate::project_files::validate(&files)?;
    // Expanded directory contents need native filename checks too.
    if let ProjectIr::Windows(w) = &inputs.project {
        let mut expanded = w.clone();
        expanded.files = files
            .iter()
            .map(|(p, e)| (p.clone(), ProjectFile::Generated { entry: e.clone() }))
            .collect();
        ProjectIr::Windows(expanded).validate_structure()?;
    }
    Ok(files)
}
fn add_resource(
    outputs: &mut BTreeMap<ProjectPath, ProjectPath>,
    files: &BTreeMap<ProjectPath, FileEntry>,
    resource: &Resource,
) -> Result<()> {
    match resource.kind {
        ResourceKind::File => {
            ensure!(
                files.contains_key(&resource.source),
                "missing resource {}",
                resource.source.as_str()
            );
            ensure!(
                outputs
                    .insert(resource.destination.clone(), resource.source.clone())
                    .is_none(),
                "duplicate resource destination"
            );
        }
        ResourceKind::Directory => {
            let prefix = format!("{}/", resource.source.as_str());
            let mut found = false;
            for path in files.keys() {
                if let Some(tail) = path.as_str().strip_prefix(&prefix) {
                    found = true;
                    let dest =
                        ProjectPath::new(format!("{}/{}", resource.destination.as_str(), tail))?;
                    ensure!(
                        outputs.insert(dest, path.clone()).is_none(),
                        "duplicate resource destination"
                    );
                }
            }
            ensure!(
                found,
                "missing or empty resource directory {}",
                resource.source.as_str()
            );
        }
    }
    Ok(())
}
pub fn sync_project(out: &Path, inputs: &DesktopProjectInputs) -> Result<bool> {
    let files = render_project(inputs)?;
    let fingerprint =
        crate::fingerprint::fingerprint(&serde_json::to_vec(&(inputs, &files, 1u32))?);
    let stamp = out.join(".whisker-fingerprint");
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s == fingerprint) {
        return Ok(false);
    }
    for path in files.keys() {
        check_destination(out, &out.join(path.as_str()))?;
    }
    check_destination(out, &stamp)?;
    if out.exists() {
        for entry in std::fs::read_dir(out)? {
            let entry = entry?;
            if entry.file_name().to_str().is_some_and(|name| {
                if matches!(inputs.project, ProjectIr::Windows(_)) {
                    name.eq_ignore_ascii_case("target")
                } else {
                    name == "target"
                }
            }) {
                continue;
            }
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    for (path, entry) in files {
        super::write_entry(&out.join(path.as_str()), &entry)?;
    }
    std::fs::write(stamp, fingerprint)?;
    Ok(true)
}
