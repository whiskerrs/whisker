//! Lower a completed Cargo application declaration to staged files and a bundle plan.
use super::*;
use crate::{
    project_files::{check_destination, insert, stage_declared},
    project_plist::plist_xml,
};
use anyhow::ensure;
use std::collections::{BTreeMap, BTreeSet};
use whisker_plugin::{FileEntry, project::*};
const PLAN_PATH: &str = ".whisker/macos-build.json";

#[derive(Debug, Clone, serde::Serialize)]
pub struct MacosProjectInputs {
    pub project: MacosProjectIr,
    pub app_crate_dir: Option<PathBuf>,
    pub cargo_selection: crate::CargoSelection,
    pub template_version: u32,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacosIcon {
    pub source: ProjectPath,
    /// Bundle-relative output path.
    pub destination: ProjectPath,
}
/// Versioned contract between CNG and the Cargo bundle builder.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacosBuildPlan {
    pub version: u32,
    pub app_name: String,
    pub rust: RustBuild,
    pub minimum_system_version: String,
    pub entitlements: ProjectPath,
    /// Bundle-relative destination -> staged file source. Staging alone never publishes a file.
    pub files: BTreeMap<ProjectPath, ProjectPath>,
    pub icons: Vec<MacosIcon>,
}
fn single_component(name: &str) -> Result<()> {
    ProjectPath::new(name)?;
    ensure!(
        !name.contains('/'),
        "macOS name must be a single path component"
    );
    Ok(())
}
impl MacosBuildPlan {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported macOS build plan; regenerate gen/macos"
        );
        single_component(&self.app_name)?;
        single_component(&self.rust.target)?;
        ensure!(
            self.rust.kind == RustArtifactKind::Bin,
            "macOS Cargo backend requires a Rust binary"
        );
        ensure!(!self.rust.package.is_empty(), "missing macOS Cargo package");
        ensure!(
            !self.minimum_system_version.is_empty()
                && self
                    .minimum_system_version
                    .split('.')
                    .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())),
            "invalid macOS deployment target"
        );
        ensure!(
            self.files
                .contains_key(&ProjectPath::new("Contents/Info.plist")?),
            "missing macOS Info.plist in bundle plan"
        );
        let mut paths = BTreeSet::new();
        paths.insert(format!("Contents/MacOS/{}", self.rust.target).to_lowercase());
        // Reserve signing metadata before the builder invokes codesign.
        paths.insert("contents/_codesignature".into());
        for path in self
            .files
            .keys()
            .chain(self.icons.iter().map(|i| &i.destination))
        {
            ensure!(
                paths.insert(path.as_str().to_lowercase()),
                "duplicate macOS bundle output: {}",
                path.as_str()
            );
        }
        for path in &paths {
            for (i, _) in path.match_indices('/') {
                ensure!(
                    !paths.contains(&path[..i]),
                    "overlapping macOS bundle outputs: {path}"
                );
            }
        }
        for icon in &self.icons {
            ensure!(
                icon.source.as_str().ends_with(".iconset")
                    && icon.destination.as_str().ends_with(".icns"),
                "macOS supports only iconset processing"
            );
        }
        Ok(())
    }
}
pub fn load_build_plan(project_dir: &Path) -> Result<MacosBuildPlan> {
    let plan: MacosBuildPlan = serde_json::from_slice(
        &std::fs::read(project_dir.join(PLAN_PATH))
            .context("read macOS build plan; regenerate gen/macos")?,
    )?;
    plan.validate()?;
    Ok(plan)
}
/// Preflight distribution input bytes before the builder mutates a bundle.
pub fn bundle_files(
    project_dir: &Path,
    plan: &MacosBuildPlan,
) -> Result<BTreeMap<ProjectPath, FileEntry>> {
    plan.validate()?;
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
    let mut files = BTreeMap::new();
    stage_declared(&mut files, &declared, Some(project_dir))?;
    crate::project_files::validate(&files)?;
    Ok(files)
}
pub fn signing_entitlements(project_dir: &Path, plan: &MacosBuildPlan) -> Result<Vec<u8>> {
    let mut files = BTreeMap::new();
    stage_declared(
        &mut files,
        &[(
            plan.entitlements.clone(),
            ProjectFile::AppFile {
                source: plan.entitlements.clone(),
            },
        )]
        .into(),
        Some(project_dir),
    )?;
    files[&plan.entitlements].to_bytes()
}
/// Snapshot icon inputs so iconutil never follows unchecked project symlinks.
pub fn icon_files(
    project_dir: &Path,
    icon: &MacosIcon,
) -> Result<BTreeMap<ProjectPath, FileEntry>> {
    let mut files = BTreeMap::new();
    stage_declared(
        &mut files,
        &[(
            icon.source.clone(),
            ProjectFile::AppDirectory {
                source: icon.source.clone(),
            },
        )]
        .into(),
        Some(project_dir),
    )?;
    ensure!(!files.is_empty(), "empty iconset");
    Ok(files)
}
pub fn sync_project(out: &Path, inputs: &MacosProjectInputs) -> Result<bool> {
    let files = render_project(inputs)?;
    let fp = fingerprint::fingerprint(&serde_json::to_vec(&(inputs, &files, 1u32))?);
    let stamp = out.join(".whisker-fingerprint");
    if std::fs::read_to_string(&stamp).is_ok_and(|v| v.trim() == fp) {
        return Ok(false);
    }
    for path in files.keys() {
        check_destination(out, &out.join(path.as_str()))?;
    }
    clean_managed_tree(out)?;
    for (path, entry) in files {
        let path = out.join(path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, entry.to_bytes()?)?;
        #[cfg(unix)]
        if let Some(mode) = entry.mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
        }
    }
    // Kept as a visible staging convention; only plan-declared files get bundled.
    std::fs::create_dir_all(out.join("Resources"))?;
    write_text(&stamp, &fp)?;
    Ok(true)
}
fn plist_string<'a>(target: &'a AppleTarget, key: &str) -> Result<&'a str> {
    match target.info_plist.get(key) {
        Some(PropertyListValue::String(value)) if !value.is_empty() => Ok(value),
        _ => anyhow::bail!("macOS Info.plist requires a nonempty string {key}"),
    }
}
pub fn render_project(inputs: &MacosProjectInputs) -> Result<BTreeMap<ProjectPath, FileEntry>> {
    ProjectIr::Macos(inputs.project.clone()).validate_structure()?;
    let apple = &inputs.project.apple;
    ensure!(
        apple.targets.len() == 1,
        "macOS Cargo backend supports one application target; additional targets need another backend"
    );
    ensure!(
        apple.build_settings.is_empty()
            && apple.configurations.is_empty()
            && apple.default_configuration.is_none()
            && apple.swift_packages.is_empty()
            && apple.schemes.is_empty(),
        "macOS Cargo backend cannot apply Xcode project settings, packages, configurations, or schemes"
    );
    let target = &apple.targets[&apple.application];
    ensure!(
        target.kind == AppleTargetKind::Native
            && target.product_type.as_deref() == Some("com.apple.product-type.application"),
        "macOS Cargo backend requires an application product"
    );
    ensure!(
        target.sources.is_empty()
            && target.headers.is_empty()
            && target.build_settings.is_empty()
            && target.configurations.is_empty()
            && target.dependencies.is_empty()
            && target.embeds.is_empty()
            && target.scripts.is_empty(),
        "macOS Cargo backend cannot apply native sources, Xcode settings, dependencies, embeds, or scripts"
    );
    let rust = target
        .rust
        .clone()
        .context("macOS application requires a Cargo binary")?;
    ensure!(
        plist_string(target, "CFBundleExecutable")? == rust.target,
        "macOS CFBundleExecutable must match the Rust binary target"
    );
    ensure!(
        plist_string(target, "CFBundlePackageType")? == "APPL",
        "macOS bundle must have APPL package type"
    );
    plist_string(target, "CFBundleIdentifier")?;
    let mut files = BTreeMap::new();
    stage_declared(&mut files, &apple.files, inputs.app_crate_dir.as_deref())?;
    for path in files.keys() {
        ensure!(
            !matches!(
                path.as_str().to_lowercase().split('/').next(),
                Some(".whisker" | "target" | ".whisker-fingerprint")
            ),
            "reserved macOS staging path: {}",
            path.as_str()
        );
    }
    ensure!(
        !files
            .keys()
            .any(|p| p.as_str().eq_ignore_ascii_case("Resources")),
        "Resources staging root must be a directory"
    );
    let manifest = files
        .get(&rust.manifest)
        .context("macOS Cargo manifest must be staged")?
        .to_bytes()?;
    let cargo: toml::Value = toml::from_str(std::str::from_utf8(&manifest)?)?;
    ensure!(
        cargo
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|v| v.as_str())
            == Some(&rust.package),
        "macOS Cargo package does not match Rust declaration"
    );
    let explicit_bin = cargo
        .get("bin")
        .and_then(|v| v.as_array())
        .is_some_and(|bins| {
            bins.iter()
                .any(|b| b.get("name").and_then(|v| v.as_str()) == Some(&rust.target))
        });
    let auto_bin = cargo
        .get("package")
        .and_then(|p| p.get("autobins"))
        .and_then(|v| v.as_bool())
        != Some(false)
        && rust.target == rust.package
        && files.contains_key(&ProjectPath::new(format!(
            "{}src/main.rs",
            rust.manifest
                .as_str()
                .strip_suffix("Cargo.toml")
                .context("macOS Rust manifest must name Cargo.toml")?
        ))?);
    ensure!(
        explicit_bin || auto_bin,
        "macOS Cargo binary does not match Rust declaration"
    );
    insert(
        &mut files,
        ProjectPath::new("Info.plist")?,
        FileEntry::text(plist_xml(&PropertyListValue::Dict(
            target.info_plist.clone(),
        ))?),
    )?;
    insert(
        &mut files,
        ProjectPath::new("Entitlements.plist")?,
        FileEntry::text(plist_xml(&PropertyListValue::Dict(
            target.entitlements.clone(),
        ))?),
    )?;
    let mut plan = MacosBuildPlan {
        version: 1,
        app_name: target.product_name.clone(),
        rust,
        minimum_system_version: plist_string(target, "LSMinimumSystemVersion")?.into(),
        entitlements: ProjectPath::new("Entitlements.plist")?,
        files: [(
            ProjectPath::new("Contents/Info.plist")?,
            ProjectPath::new("Info.plist")?,
        )]
        .into(),
        icons: vec![],
    };
    for resource in &target.resources {
        match resource {
            AppleResource::Copy { resource } => {
                copy_resource(&files, &mut plan.files, resource, "Contents/Resources/")?
            }
            AppleResource::Process { path } => {
                ensure!(
                    path.as_str().ends_with(".iconset"),
                    "macOS Cargo backend only processes .iconset resources"
                );
                ensure!(
                    files
                        .keys()
                        .any(|p| p.as_str().starts_with(&format!("{}/", path.as_str()))),
                    "missing or empty staged iconset"
                );
                let name = Path::new(path.as_str())
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap();
                plan.icons.push(MacosIcon {
                    source: path.clone(),
                    destination: ProjectPath::new(format!("Contents/Resources/{name}.icns"))?,
                });
            }
        }
    }
    for resource in &target.bundle_files {
        copy_resource(&files, &mut plan.files, resource, "")?;
    }
    for (dest, value) in &target.resource_plists {
        let source = ProjectPath::new(format!(".whisker/plists/{}", dest.as_str()))?;
        insert(
            &mut files,
            source.clone(),
            FileEntry::text(plist_xml(value)?),
        )?;
        add_output(
            &mut plan.files,
            ProjectPath::new(format!("Contents/Resources/{}", dest.as_str()))?,
            source,
        )?;
    }
    plan.validate()?;
    insert(
        &mut files,
        ProjectPath::new(PLAN_PATH)?,
        FileEntry::text(serde_json::to_string_pretty(&plan)?),
    )?;
    crate::project_files::validate(&files)?;
    let mut paths = BTreeSet::new();
    for path in files.keys() {
        ensure!(
            paths.insert(path.as_str().to_lowercase()),
            "case-colliding macOS staging outputs"
        );
    }
    for path in &paths {
        for (i, _) in path.match_indices('/') {
            ensure!(
                !paths.contains(&path[..i]),
                "overlapping macOS staging outputs"
            );
        }
    }
    Ok(files)
}
fn add_output(
    out: &mut BTreeMap<ProjectPath, ProjectPath>,
    dest: ProjectPath,
    source: ProjectPath,
) -> Result<()> {
    ensure!(
        !out.contains_key(&dest),
        "duplicate macOS bundle output: {}",
        dest.as_str()
    );
    out.insert(dest, source);
    Ok(())
}
fn copy_resource(
    files: &BTreeMap<ProjectPath, FileEntry>,
    out: &mut BTreeMap<ProjectPath, ProjectPath>,
    resource: &Resource,
    prefix: &str,
) -> Result<()> {
    let dest = format!("{prefix}{}", resource.destination.as_str());
    match resource.kind {
        ResourceKind::File => {
            ensure!(
                files.contains_key(&resource.source),
                "missing staged macOS resource: {}",
                resource.source.as_str()
            );
            add_output(out, ProjectPath::new(dest)?, resource.source.clone())?;
        }
        ResourceKind::Directory => {
            let source_prefix = format!("{}/", resource.source.as_str());
            let mut found = false;
            for source in files
                .keys()
                .filter(|p| p.as_str().starts_with(&source_prefix))
            {
                add_output(
                    out,
                    ProjectPath::new(format!(
                        "{dest}/{}",
                        &source.as_str()[source_prefix.len()..]
                    ))?,
                    source.clone(),
                )?;
                found = true;
            }
            ensure!(found, "missing or empty staged macOS resource directory");
        }
    }
    Ok(())
}
