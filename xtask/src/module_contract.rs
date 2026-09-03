//! Static contract checks for first-party Whisker modules.
//!
//! Host implementations are intentionally hand-written and loosely coupled
//! from their Rust package. That keeps normal Xcode/Gradle/Cargo builds
//! autonomous, but it also means an identifier typo otherwise survives until
//! runtime bootstrap. These tests keep the checked-in first-party packages in
//! sync without introducing generated Host bindings.

use std::fs;
use std::path::{Path, PathBuf};

const HOSTS: [&str; 4] = ["android", "ios", "web", "desktop"];

#[test]
fn first_party_element_names_match_every_declared_host() {
    let root = super::workspace_root().expect("resolve workspace root");
    let mut failures = Vec::new();

    for package_dir in module_package_dirs(&root) {
        let manifest = read(&package_dir.join("Cargo.toml"));
        let package_name = package_name(&manifest)
            .unwrap_or_else(|| panic!("{} has no [package] name", package_dir.display()));
        let rust_source = source_tree(&package_dir.join("src"), "rs");
        let element_names = module_element_names(&rust_source);

        for element_name in &element_names {
            if !element_name.starts_with(&format!("{package_name}:")) {
                failures.push(format!(
                    "{package_name}: Rust element {element_name:?} must use `<crate>:<Element>`"
                ));
            }
        }

        for host in HOSTS {
            let Some(host_manifest) = platform_manifest(&manifest, host) else {
                continue;
            };
            let host_root = package_dir
                .join(host_manifest)
                .parent()
                .unwrap()
                .to_path_buf();
            let source_root = match host {
                "android" => host_root.join("android/src/main"),
                "ios" => host_root.join("ios/Sources"),
                "web" | "desktop" => host_root.join("src"),
                _ => unreachable!(),
            };
            let extension = match host {
                "android" => "kt",
                "ios" => "swift",
                "web" | "desktop" => "rs",
                _ => unreachable!(),
            };
            let host_source = source_tree(&source_root, extension);
            for element_name in &element_names {
                let declared = match host {
                    "android" | "ios" => string_values_after(&host_source, "View(\"")
                        .any(|name| name == *element_name),
                    "web" | "desktop" => rust_host_declares_element(&host_source, element_name),
                    _ => unreachable!(),
                };
                if !declared {
                    failures.push(format!(
                        "{package_name}: {host} Host does not declare Rust element {element_name:?}"
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "first-party element contracts are inconsistent:\n{}",
        failures.join("\n")
    );
}

#[test]
fn rust_host_module_names_are_package_qualified() {
    let root = super::workspace_root().expect("resolve workspace root");
    let mut failures = Vec::new();

    for package_dir in module_package_dirs(&root) {
        let manifest = read(&package_dir.join("Cargo.toml"));
        let package_name = package_name(&manifest)
            .unwrap_or_else(|| panic!("{} has no [package] name", package_dir.display()));
        let expected_prefix = format!("{package_name}:");
        let rust_source = source_tree(&package_dir.join("src"), "rs");
        let mut expected_names = module_element_names(&rust_source)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        expected_names.extend(
            string_values_after(&rust_source, "module!(\"")
                .map(|name| format!("{package_name}:{name}")),
        );

        for host in ["web", "desktop"] {
            let Some(host_manifest) = platform_manifest(&manifest, host) else {
                continue;
            };
            let host_root = package_dir
                .join(host_manifest)
                .parent()
                .unwrap()
                .to_path_buf();
            let source = source_tree(&host_root.join("src"), "rs");
            let compact = compact_source(&source);
            let constant_names =
                string_values_after(&source, "const MODULE_NAME: &str = \"").collect::<Vec<_>>();
            let direct_names = string_values_after(&compact, "ModuleDefinition::new().name(\"")
                .collect::<Vec<_>>();
            let names_module = !direct_names.is_empty()
                || !constant_names.is_empty()
                    && compact.contains("ModuleDefinition::new().name(MODULE_NAME)");
            if source.contains("impl WhiskerModule") && !names_module {
                failures.push(format!(
                    "{package_name}: {host} WhiskerModule has no ModuleDefinition name"
                ));
            }
            for module_name in constant_names.into_iter().chain(direct_names) {
                if !module_name.starts_with(&expected_prefix) {
                    failures.push(format!(
                        "{package_name}: {host} module name {module_name:?} must start with {expected_prefix:?}"
                    ));
                } else if !expected_names
                    .iter()
                    .any(|expected| expected == module_name)
                {
                    failures.push(format!(
                        "{package_name}: {host} registers {module_name:?}, which has no matching Rust element or module handle"
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "first-party Rust Host module names are inconsistent:\n{}",
        failures.join("\n")
    );
}

fn module_package_dirs(root: &Path) -> Vec<PathBuf> {
    let mut directories = fs::read_dir(root.join("packages"))
        .expect("read packages directory")
        .map(|entry| entry.expect("read package entry").path())
        .filter(|path| {
            path.join("Cargo.toml").is_file()
                && read(&path.join("Cargo.toml"))
                    .contains("[package.metadata.whisker.module.platforms]")
        })
        .collect::<Vec<_>>();
    directories.sort();
    directories
}

fn package_name(manifest: &str) -> Option<&str> {
    let package = manifest.split_once("[package]")?.1;
    let package = package.split("\n[").next().unwrap_or(package);
    string_values_after(package, "name = \"").next()
}

fn platform_manifest(manifest: &str, host: &str) -> Option<String> {
    let platforms = manifest
        .split_once("[package.metadata.whisker.module.platforms]")?
        .1
        .split("\n[")
        .next()
        .unwrap_or_default();
    let line = platforms
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{host} =")))?;
    string_values_after(line, "manifest = \"")
        .next()
        .map(str::to_owned)
}

fn module_element_names(source: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = source;
    const START: &str = "#[whisker::module_element(";
    while let Some((_, after_start)) = rest.split_once(START) {
        let Some((attribute, after_attribute)) = after_start.split_once(")]") else {
            break;
        };
        if let Some(name) = string_values_after(attribute, "name = \"").next() {
            names.push(name);
        }
        rest = after_attribute;
    }
    names
}

fn rust_host_declares_element(source: &str, element_name: &str) -> bool {
    let compact = compact_source(source);
    let direct_web = format!("WebViewDefinition::new(\"{element_name}\"");
    let direct_desktop = format!("DesktopViewDefinition::new(\"{element_name}\"");
    let constant = format!("constMODULE_NAME:&str=\"{element_name}\";");
    let constant_view = compact.contains("WebViewDefinition::new(MODULE_NAME,")
        || compact.contains("DesktopViewDefinition::new(MODULE_NAME,");

    compact.contains(&direct_web)
        || compact.contains(&direct_desktop)
        || compact.contains(&constant) && constant_view
}

fn compact_source(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect()
}

fn string_values_after<'a>(source: &'a str, marker: &'a str) -> impl Iterator<Item = &'a str> {
    source.match_indices(marker).filter_map(move |(offset, _)| {
        let value = &source[offset + marker.len()..];
        value.split_once('"').map(|(value, _)| value)
    })
}

fn source_tree(root: &Path, extension: &str) -> String {
    let mut files = Vec::new();
    collect_sources(root, extension, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| read(&path))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_sources(root: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("read source entry").path();
        if path.is_dir() {
            collect_sources(&path, extension, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            files.push(path);
        }
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}
