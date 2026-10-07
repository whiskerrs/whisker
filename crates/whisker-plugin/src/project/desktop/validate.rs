use super::*;
use crate::project::RustArtifactKind;
use crate::project::validate::{files, ids, reference, unique_paths};
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

pub(in crate::project) fn validate_windows(w: &WindowsProjectIr) -> Result<()> {
    ids(w.executables.keys())?;
    ids(w.packages.keys())?;
    reference(&w.executables, &w.application, "Windows application")?;
    windows_paths(w.files.keys(), "Windows staged files")?;
    windows_paths(
        w.executables
            .values()
            .map(|e| &e.executable.destination)
            .chain(w.resources.iter().map(|r| &r.destination)),
        "Windows distribution",
    )?;
    for exe in w.executables.values() {
        executable(&exe.executable)?;
        if let Some(manifest) = &exe.manifest {
            root(manifest, &["assembly"], "Win32 manifest")?;
        }
        if let Some(version) = &exe.version_info {
            let mut languages = BTreeSet::new();
            for lang in version.strings.keys() {
                ensure!(
                    lang.len() == 8 && lang.bytes().all(|b| b.is_ascii_hexdigit()),
                    "invalid VERSIONINFO translation ID"
                );
                ensure!(
                    languages.insert(lang.to_ascii_lowercase()),
                    "duplicate VERSIONINFO translation ID"
                );
            }
        }
    }
    if let Some(id) = &w.application_package {
        reference(&w.packages, id, "Windows application package")?;
        ensure!(
            w.packages[id].executables.contains(&w.application),
            "MSIX application package must include the Windows application executable"
        );
    }
    let manifest_path = ProjectPath::new("AppxManifest.xml")?;
    for package in w.packages.values() {
        root(&package.manifest, &["Package"], "MSIX manifest")?;
        let mut destinations = vec![&manifest_path];
        for id in &package.executables {
            reference(&w.executables, id, "MSIX executable")?;
            destinations.push(&w.executables[id].executable.destination);
        }
        destinations.extend(package.resources.iter().map(|r| &r.destination));
        windows_paths(destinations, "MSIX package")?;
    }
    Ok(())
}
pub(in crate::project) fn validate_linux(l: &LinuxProjectIr) -> Result<()> {
    ids([&l.app_id])?;
    ids(l.executables.keys())?;
    ids(l.packages.keys())?;
    reference(&l.executables, &l.application, "Linux application")?;
    files(&l.files)?;
    for exe in l.executables.values() {
        executable(exe)?;
    }
    unique_paths(
        l.executables
            .values()
            .map(|e| &e.destination)
            .chain(l.resources.iter().map(|r| &r.destination))
            .chain(l.desktop_entries.keys())
            .chain(l.mime_packages.keys())
            .chain(l.metainfo.keys())
            .chain(l.dbus_services.keys()),
        "Linux installation",
    )?;
    for entry in l.desktop_entries.values() {
        desktop_values(&entry.entries)?;
        for action in entry.actions.values() {
            desktop_values(action)?;
        }
        ids(entry.actions.keys())?;
        for id in entry.actions.keys() {
            ensure!(
                !id.contains(['[', ']', '\n', '\r']),
                "invalid desktop action ID"
            );
        }
        if let Some(actions) = entry.entries.get("Actions") {
            let DesktopEntryValue::List(actions) = actions else {
                anyhow::bail!("Desktop Actions must be a logical list");
            };
            ensure!(
                actions.iter().collect::<BTreeSet<_>>().len() == actions.len(),
                "duplicate desktop action reference"
            );
            for action in actions {
                reference(&entry.actions, action, "desktop action")?;
            }
        }
    }
    for xml in l.mime_packages.values() {
        root(xml, &["mime-info"], "MIME package")?;
    }
    for xml in l.metainfo.values() {
        root(xml, &["component", "components"], "AppStream metadata")?;
    }
    for service in l.dbus_services.values() {
        for key in service.entries.keys() {
            key_name(key)?;
        }
    }
    for package in l.packages.values() {
        if let LinuxPackage::Flatpak { manifest } = package {
            if let FlatpakCommand::Executable { id } = &manifest.command {
                reference(&l.executables, id, "Flatpak command")?;
            }
            let reserved = [
                "id",
                "app-id",
                "runtime",
                "runtime-version",
                "sdk",
                "command",
                "finish-args",
                "modules",
            ];
            for key in manifest.properties.keys() {
                ensure!(
                    !reserved.contains(&key.as_str()),
                    "reserved Flatpak property {key}"
                );
            }
            let mut names = BTreeSet::new();
            for module in &manifest.modules {
                if let FlatpakModule::Inline(value) = module {
                    ensure!(
                        value
                            .get("name")
                            .and_then(|v| v.as_str())
                            .is_some_and(|s| !s.trim().is_empty()),
                        "Flatpak module needs name"
                    );
                }
                ensure!(
                    names.insert(super::compose::module_key(module)),
                    "duplicate Flatpak module"
                );
            }
            ensure!(
                !manifest.runtime.trim().is_empty()
                    && !manifest.runtime_version.trim().is_empty()
                    && !manifest.sdk.trim().is_empty(),
                "Flatpak runtime/version/sdk must be specified"
            );
        }
    }
    Ok(())
}
fn key_name(key: &str) -> Result<()> {
    ensure!(
        !key.is_empty() && !key.chars().any(|c| c.is_control() || c == '='),
        "invalid desktop metadata key"
    );
    Ok(())
}
fn desktop_values(values: &BTreeMap<String, DesktopEntryValue>) -> Result<()> {
    for (key, value) in values {
        key_name(key)?;
        if let DesktopEntryValue::Number(n) = value {
            ensure!(n.is_finite(), "desktop numeric value must be finite");
        }
    }
    Ok(())
}
fn executable(e: &DesktopExecutable) -> Result<()> {
    if let ExecutableSource::Cargo(build) = &e.source {
        ensure!(
            build.kind == RustArtifactKind::Bin,
            "desktop executable Cargo artifact must be a bin"
        );
    }
    Ok(())
}
fn root(xml: &XmlElement, names: &[&str], owner: &str) -> Result<()> {
    ensure!(
        names.contains(&xml.name.rsplit(':').next().unwrap_or("")),
        "invalid {owner} root"
    );
    Ok(())
}
fn windows_paths<'a>(paths: impl IntoIterator<Item = &'a ProjectPath>, owner: &str) -> Result<()> {
    let mut folded = Vec::new();
    for path in paths {
        for component in path.as_str().split('/') {
            ensure!(
                !component.ends_with([' ', '.'])
                    && !component
                        .chars()
                        .any(|c| c.is_control() || "<>\"|?*".contains(c)),
                "invalid Windows destination: {}",
                path.as_str()
            );
            let stem = component.split('.').next().unwrap().to_ascii_uppercase();
            let numbered = stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"));
            ensure!(
                !["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
                    && !numbered.is_some_and(|n| [
                        "1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"
                    ]
                    .contains(&n)),
                "reserved Windows destination: {}",
                path.as_str()
            );
        }
        folded.push(ProjectPath::new(path.as_str().to_ascii_lowercase())?);
    }
    unique_paths(&folded, owner)
}
