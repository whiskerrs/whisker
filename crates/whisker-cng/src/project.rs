//! Build and execute the application's generator without compiling its library.

use anyhow::{Context, Result, ensure};
use cargo_metadata::{Metadata, Package};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{GenerationReport, GenerationTarget};

/// Execute `whisker.rs` to generate the selected projects and return their report.
///
/// An empty target list generates all supported platforms. This builds a
/// separate executable containing CNG and discovered plugin dependencies,
/// avoiding compilation of the application library. Registered binaries supply
/// their own `main`; legacy `configure(&mut Config)` files receive a generation
/// entry point. Every invocation runs generation, including fingerprint checks.
/// Arbitrary application dependencies and features are not copied into this
/// executable. Registered binaries use the application's resolved CNG dependency;
/// legacy files use this CNG implementation's generation entry point.
pub fn generate(manifest_path: &Path, targets: &[GenerationTarget]) -> Result<GenerationReport> {
    let manifest = manifest_path
        .canonicalize()
        .context("resolve application manifest")?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest)
        .exec()
        .context("resolve generator dependencies")?;
    let package = metadata
        .packages
        .iter()
        .find(|package| package.manifest_path.as_std_path() == manifest)
        .context("generation requires an application package manifest")?;
    let crate_dir = manifest.parent().context("manifest has no parent")?;
    let source = crate_dir.join("whisker.rs");
    ensure!(
        source.is_file(),
        "no whisker.rs next to {}",
        manifest.display()
    );
    let canonical_source = source.canonicalize()?;
    let has_main = package.targets.iter().any(|target| {
        target.kind.iter().any(|kind| kind == "bin")
            && target.src_path.as_std_path().canonicalize().ok().as_ref() == Some(&canonical_source)
    });
    let plugins = crate::discovery::discover_plugins_from_metadata(&metadata, &package.name)?;
    let cng = if has_main {
        direct_dependency(&metadata, package, "whisker-cng")
    } else {
        None
    };
    let config = cng.and_then(|cng| direct_dependency(&metadata, cng, "whisker-config"));
    let cng_spec = dependency_spec(cng, Path::new(env!("CARGO_MANIFEST_DIR")), true);
    let config_spec = dependency_spec(
        config,
        &Path::new(env!("CARGO_MANIFEST_DIR")).with_file_name("whisker-config"),
        true,
    );
    let generator_dir = crate_dir.join("target/.whisker/generator");
    std::fs::create_dir_all(generator_dir.join("src"))?;
    let mut dependencies = format!("whisker-cng = {cng_spec}\nwhisker-config = {config_spec}\n");
    let mut seen = std::collections::BTreeSet::new();
    for plugin in plugins {
        if seen.insert(plugin.source_crate.clone()) {
            let package = metadata.packages.iter().find(|package| {
                package
                    .manifest_path
                    .parent()
                    .map(|path| path.as_std_path())
                    == Some(plugin.source_manifest_dir.as_path())
            });
            dependencies.push_str(&format!(
                "{} = {}\n",
                plugin.source_crate,
                dependency_spec(package, &plugin.source_manifest_dir, false)
            ));
        }
    }
    let entry = if has_main {
        source.clone()
    } else {
        let entry = generator_dir.join("src/main.rs");
        std::fs::write(
            &entry,
            format!(
                "include!({:?});\nfn main() {{ whisker_cng::run(configure); }}\n",
                source.to_string_lossy()
            ),
        )?;
        entry
    };
    let patches = workspace_patches(metadata.workspace_root.as_std_path())?;
    std::fs::write(
        generator_dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"__whisker_generator_{}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{dependencies}\n[[bin]]\nname = \"__whisker_generator_{}\"\npath = {}\n\n[workspace]\n{patches}",
            package.name.replace('-', "_"),
            package.name.replace('-', "_"),
            toml_path(&entry),
        ),
    )?;

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let report = ReportFile(crate_dir.join(format!(
        "target/.whisker/reports/{}-{}.json",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )));
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command
        .args(["run", "--quiet", "--release", "--manifest-path"])
        .arg(generator_dir.join("Cargo.toml"))
        .arg("--target")
        .arg(crate::generator::host_target()?)
        .arg("--target-dir")
        .arg(
            metadata
                .workspace_root
                .join("target/.whisker/generator-build"),
        )
        .arg("--")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--report-path")
        .arg(&report.0)
        .current_dir(crate_dir);
    for target in targets {
        command.arg("--target").arg(target.as_str());
    }
    let status = command
        .status()
        .context("execute Whisker project generator")?;
    ensure!(
        status.success(),
        "Whisker project generation failed ({status})"
    );
    let generated: GenerationReport = serde_json::from_slice(&std::fs::read(&report.0).context(
        "generator did not write its completion report; whisker.rs must call whisker_cng::run",
    )?)
    .context("read generation report")?;
    ensure!(
        generated.schema_version == 1,
        "unsupported generation report version {}",
        generated.schema_version
    );
    ensure!(
        generated.crate_dir == crate_dir && generated.package == package.name,
        "generator reported a different application"
    );
    let expected = if targets.is_empty() {
        &GenerationTarget::ALL[..]
    } else {
        targets
    };
    ensure!(
        expected
            .iter()
            .all(|target| generated.projects.contains_key(target)),
        "generator did not complete every requested platform"
    );
    Ok(generated)
}

struct ReportFile(PathBuf);
impl Drop for ReportFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn direct_dependency<'a>(
    metadata: &'a Metadata,
    package: &Package,
    name: &str,
) -> Option<&'a Package> {
    let node = metadata
        .resolve
        .as_ref()?
        .nodes
        .iter()
        .find(|node| node.id == package.id)?;
    node.deps
        .iter()
        .filter(|dep| {
            dep.dep_kinds
                .iter()
                .any(|kind| kind.kind == cargo_metadata::DependencyKind::Normal)
        })
        .filter_map(|dep| {
            metadata
                .packages
                .iter()
                .find(|package| package.id == dep.pkg)
        })
        .find(|package| package.name == name)
}

fn dependency_spec(package: Option<&Package>, fallback: &Path, default_features: bool) -> String {
    let features = if default_features {
        ""
    } else {
        ", default-features = false"
    };
    if let Some(package) = package.filter(|package| {
        package
            .source
            .as_ref()
            .is_some_and(|source| source.is_crates_io())
    }) {
        return format!(
            "{{ version = {:?}{features} }}",
            format!("={}", package.version)
        );
    }
    let path = package
        .and_then(|package| package.manifest_path.parent())
        .map(|path| path.as_std_path())
        .unwrap_or(fallback);
    if path.join("Cargo.toml").is_file() {
        format!("{{ path = {}{features} }}", toml_path(path))
    } else {
        format!(
            "{{ version = {:?}{features} }}",
            format!("={}", env!("CARGO_PKG_VERSION"))
        )
    }
}

fn toml_path(path: &Path) -> String {
    toml::Value::String(path.to_string_lossy().into_owned()).to_string()
}

fn workspace_patches(workspace: &Path) -> Result<String> {
    let document: toml::Value = std::fs::read_to_string(workspace.join("Cargo.toml"))?.parse()?;
    let Some(mut patches) = document.get("patch").cloned() else {
        return Ok(String::new());
    };
    if let Some(registries) = patches.as_table_mut() {
        for registry in registries
            .iter_mut()
            .map(|(_, value)| value)
            .filter_map(toml::Value::as_table_mut)
        {
            for dependency in registry
                .iter_mut()
                .map(|(_, value)| value)
                .filter_map(toml::Value::as_table_mut)
            {
                if let Some(path) = dependency.get("path").and_then(toml::Value::as_str) {
                    dependency.insert(
                        "path".into(),
                        toml::Value::String(workspace.join(path).to_string_lossy().into_owned()),
                    );
                }
            }
        }
    }
    let mut table = toml::value::Table::new();
    table.insert("patch".into(), patches);
    Ok(toml::to_string(&table)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_dependencies_keep_their_source_identity_and_resolved_version() {
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .no_deps()
            .exec()
            .unwrap();
        let mut package = metadata
            .packages
            .into_iter()
            .find(|package| package.name == "whisker-cng")
            .unwrap();
        package.source = Some(cargo_metadata::Source {
            repr: "registry+https://github.com/rust-lang/crates.io-index".into(),
        });
        package.version = "99.1.2".parse().unwrap();
        let spec = dependency_spec(Some(&package), Path::new("/unused"), false);
        let manifest: toml::Value = format!("[dependencies]\nwhisker-cng = {spec}")
            .parse()
            .unwrap();
        let dependency = &manifest["dependencies"]["whisker-cng"];
        assert_eq!(dependency["version"].as_str(), Some("=99.1.2"));
        assert_eq!(dependency["default-features"].as_bool(), Some(false));
        assert!(dependency.get("path").is_none());
    }
}
