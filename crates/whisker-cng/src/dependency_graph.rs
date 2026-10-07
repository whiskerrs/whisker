//! Canonical Cargo dependency graph used by every CNG target.

use std::path::Path;

use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;

use crate::discovery::{DiscoveredPlugin, discover_plugins_from_metadata};
use crate::modules::{ResolvedModule, discover_from_metadata};

/// All Whisker-specific contributions resolved from one Cargo graph snapshot.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectDependencyGraph {
    /// Runtime/native modules selected by the application dependency graph.
    pub modules: Vec<ResolvedModule>,
    /// Generation-time plugins activated by those same dependencies.
    pub cng_plugins: Vec<DiscoveredPlugin>,
}

impl ProjectDependencyGraph {
    fn snapshot_path(
        workspace: &Path,
        package: &str,
        platform: crate::GenerationTarget,
    ) -> Result<std::path::PathBuf> {
        Ok(
            crate::CargoSelection::project_state_dir(workspace, package, platform)?
                .join("native-graph.json"),
        )
    }

    pub(crate) fn save(&self, project_dir: &Path) -> Result<()> {
        crate::selection::write_json(&project_dir.join(".whisker/native-graph.json"), self)
    }

    /// Check the actual native slice against the generated project. iOS
    /// device/simulator and Android ABIs may share a project only when their
    /// module and plugin contributions agree. Also catches stale IDE projects.
    pub fn native_build_selection(
        workspace: &Path,
        package: &str,
        platform: crate::GenerationTarget,
        triple: &str,
        extra_features: &[String],
    ) -> Result<crate::CargoSelection> {
        let selection = crate::CargoSelection::load(workspace, package, platform)?;
        let path = Self::snapshot_path(workspace, package, platform)?;
        let snapshot = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(selection),
            Err(error) => return Err(error.into()),
        };
        let mut actual = selection.clone();
        actual.target = Some(triple.into());
        actual.features.extend_from_slice(extra_features);
        let graph = Self::resolve_with_selection(&workspace.join("Cargo.toml"), package, &actual)?;
        anyhow::ensure!(
            serde_json::from_slice::<serde_json::Value>(&snapshot)?
                == serde_json::to_value(&graph)?,
            "native dependencies for `{package}` ({triple}) differ from the generated {} project; regenerate with the same Cargo target and features before building",
            platform.as_str()
        );
        Ok(selection)
    }

    /// Resolve native modules and plugin declarations from the same selected
    /// runtime graph. Without explicit selection, Cargo uses its default target.
    pub fn resolve(manifest_path: &Path, app_package: &str) -> Result<Self> {
        Self::resolve_with_selection(
            manifest_path,
            app_package,
            &crate::CargoSelection::default(),
        )
    }

    pub fn resolve_with_selection(
        manifest_path: &Path,
        app_package: &str,
        selection: &crate::CargoSelection,
    ) -> Result<Self> {
        let metadata = selected_metadata(manifest_path, app_package, selection)?;
        Ok(Self {
            modules: discover_from_metadata(&metadata, app_package)
                .context("resolve Whisker modules")?,
            cng_plugins: discover_plugins_from_metadata(&metadata, app_package)
                .context("resolve Whisker CNG plugins")?,
        })
    }
}

/// Metadata supplies declarations, while Cargo's package-scoped tree supplies
/// membership. Metadata alone unifies workspace, build, and dev features even
/// with --filter-platform, so pruning its edges is insufficient (resolver 2).
pub(crate) fn selected_metadata(
    manifest: &Path,
    package: &str,
    selection: &crate::CargoSelection,
) -> Result<cargo_metadata::Metadata> {
    use std::collections::HashSet;
    // Selected dependency features (e.g. `whisker/hot-reload`) can pull in
    // packages that none of the application's own features reach, and the
    // tree below lists them, so metadata must resolve with them too.
    let mut metadata = MetadataCommand::new()
        .manifest_path(manifest)
        .features(cargo_metadata::CargoOpt::AllFeatures)
        .other_options(
            selection
                .features
                .iter()
                .flat_map(|feature| ["--features".to_owned(), feature.clone()])
                .collect::<Vec<_>>(),
        )
        .exec()
        .context("resolve package declarations")?;
    let mut command =
        std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command
        .args([
            "tree",
            "--color",
            "never",
            "--prefix",
            "none",
            "--format",
            "{p}",
            "--edges",
            "normal,no-proc-macro",
            "--manifest-path",
        ])
        .arg(manifest)
        .args(["--package", package]);
    selection.apply(&mut command);
    if let Some(target) = &selection.target {
        command.arg("--target").arg(target);
    }
    let output = command
        .output()
        .context("resolve application runtime dependencies")?;
    anyhow::ensure!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut selected = HashSet::new();
    for line in std::str::from_utf8(&output.stdout)?.lines() {
        let display = line.strip_suffix(" (*)").unwrap_or(line);
        // {p} is Cargo's documented package display format. Do not guess if
        // two sources publish the same name and version.
        let matches: Vec<_> = metadata
            .packages
            .iter()
            .filter(|p| {
                let base = format!("{} v{}", p.name, p.version);
                if display == base {
                    return p.source.as_ref().is_some_and(|s| s.is_crates_io());
                }
                let Some(source) = display
                    .strip_prefix(&format!("{base} ("))
                    .and_then(|s| s.strip_suffix(')'))
                else {
                    return false;
                };
                if p.source.is_none() {
                    return p.manifest_path.parent().is_some_and(|dir| {
                        std::fs::canonicalize(dir).ok() == std::fs::canonicalize(source).ok()
                    });
                }
                let repr = &p.source.as_ref().unwrap().repr;
                repr.strip_prefix("git+")
                    .is_some_and(|git| git.starts_with(source))
                    || repr.strip_prefix("registry+") == Some(source)
            })
            .collect();
        anyhow::ensure!(
            matches.len() == 1,
            "cannot identify Cargo tree package `{display}` in metadata ({} matches)",
            matches.len()
        );
        selected.insert(matches[0].id.clone());
    }
    let resolve = metadata
        .resolve
        .as_mut()
        .context("missing Cargo resolution")?;
    resolve.nodes.retain(|node| selected.contains(&node.id));
    for node in &mut resolve.nodes {
        node.deps.retain(|dep| selected.contains(&dep.pkg));
        node.dependencies.retain(|id| selected.contains(id));
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_project_snapshot_contains_fixture_modules_and_no_plugins() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("tests/cng-module-fixture/Cargo.toml");
        let graph = ProjectDependencyGraph::resolve(&workspace, "cng-module-fixture").unwrap();
        let mut packages: Vec<_> = graph
            .modules
            .iter()
            .map(|module| module.package.as_str())
            .collect();
        packages.sort_unstable();
        assert_eq!(packages, ["cng-test-service", "cng-test-widget"]);
        assert_eq!(graph.cng_plugins.len(), 0);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::CargoSelection;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    impl Fixture {
        fn new() -> Self {
            static SEQUENCE: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "cng-selection-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("Cargo.toml"), "[workspace]\nresolver = '2'\nmembers = ['app', 'other', 'facade', 'auth', 'store', 'dev', 'build', 'windows', 'host', 'macro', 'macro-leaf']\n").unwrap();
            let f = Self(dir);
            for name in [
                "auth",
                "store",
                "dev",
                "build",
                "windows",
                "host",
                "macro-leaf",
            ] {
                f.package(name, "[package.metadata.whisker.module.platforms]\nandroid = { kind = 'common' }\nios = { kind = 'common' }\n");
            }
            f.package("macro", "[lib]\nproc-macro = true\n[dependencies]\nmacro-leaf = { path = '../macro-leaf' }\n");
            let auth_manifest = f.0.join("auth/Cargo.toml");
            let mut auth = std::fs::read_to_string(&auth_manifest).unwrap();
            auth.push_str("\n[package.metadata.whisker.plugins.auth-cng]\nbin = 'auth-cng'\n");
            std::fs::write(auth_manifest, auth).unwrap();
            f.package("facade", "[features]\ndefault = ['auth']\nauth = ['dep:auth']\nstore = ['dep:store']\nhost = ['dep:host']\n[dependencies]\nauth = { path = '../auth', optional = true }\nstore = { path = '../store', optional = true }\nhost = { path = '../host', optional = true }\n");
            f.package("app", "[features]\ndefault = ['auth']\nauth = ['renamed/auth']\nstore = ['renamed/store']\n[dependencies]\nmacro = { path = '../macro' }\nrenamed = { package = 'facade', path = '../facade', default-features = false }\n[dev-dependencies]\ndev = { path = '../dev' }\n[build-dependencies]\nbuild = { path = '../build' }\nrenamed = { package = 'facade', path = '../facade', features = ['host'] }\n[target.'cfg(windows)'.dependencies]\nwindows = { path = '../windows' }\n");
            std::fs::write(f.0.join("app/build.rs"), "fn main() {}\n").unwrap();
            f.package(
                "other",
                "[dependencies]\nfacade = { path = '../facade', features = ['store'] }\n",
            );
            f
        }
        fn package(&self, name: &str, extra: &str) {
            let dir = self.0.join(name);
            std::fs::create_dir_all(dir.join("src")).unwrap();
            std::fs::write(dir.join("src/lib.rs"), "").unwrap();
            std::fs::write(
                dir.join("Cargo.toml"),
                format!("[package]\nname = '{name}'\nversion = '0.1.0'\nedition = '2021'\n{extra}"),
            )
            .unwrap();
        }
        fn modules(
            &self,
            features: &[&str],
            no_default_features: bool,
            target: &str,
        ) -> Vec<String> {
            ProjectDependencyGraph::resolve_with_selection(
                &self.0.join("Cargo.toml"),
                "app",
                &CargoSelection {
                    features: features.iter().map(|s| s.to_string()).collect(),
                    no_default_features,
                    target: Some(target.into()),
                },
            )
            .unwrap()
            .modules
            .into_iter()
            .map(|m| m.package)
            .collect()
        }
    }

    #[test]
    fn dependency_features_can_reach_packages_outside_the_application_features() {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = Fixture(std::env::temp_dir().join(format!(
            "cng-dependency-feature-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        )));
        let package = |name: &str, manifest: &str| {
            let dir = root.0.join(name);
            std::fs::create_dir_all(dir.join("src")).unwrap();
            std::fs::write(dir.join("src/lib.rs"), "").unwrap();
            std::fs::write(
                dir.join("Cargo.toml"),
                format!(
                    "[package]\nname = '{name}'\nversion = '0.1.0'\nedition = '2021'\n{manifest}"
                ),
            )
            .unwrap();
        };
        package(
            "extra",
            "[package.metadata.whisker.module.platforms]\nios = { kind = 'common' }\n",
        );
        package(
            "runtime",
            "[features]\ndev = ['dep:extra']\n[dependencies]\nextra = { path = '../extra', optional = true }\n",
        );
        // A standalone app workspace, as `whisker new` generates, so `extra`
        // is not a workspace member that metadata would list regardless.
        package(
            "app",
            "[workspace]\n[dependencies]\nruntime = { path = '../runtime' }\n",
        );

        let modules = ProjectDependencyGraph::resolve_with_selection(
            &root.0.join("app/Cargo.toml"),
            "app",
            &CargoSelection {
                features: vec!["runtime/dev".into()],
                no_default_features: false,
                target: Some("aarch64-apple-ios".into()),
            },
        )
        .unwrap()
        .modules;
        assert_eq!(
            modules.into_iter().map(|m| m.package).collect::<Vec<_>>(),
            ["extra"]
        );
    }

    #[test]
    fn follows_app_runtime_features_without_workspace_dev_build_or_target_leaks() {
        let f = Fixture::new();
        assert_eq!(f.modules(&[], false, "aarch64-apple-ios"), ["auth"]);
        let selected = CargoSelection::default().for_platform(crate::GenerationTarget::Ios);
        let graph = ProjectDependencyGraph::resolve_with_selection(
            &f.0.join("Cargo.toml"),
            "app",
            &selected,
        )
        .unwrap();
        assert_eq!(
            graph
                .cng_plugins
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["auth-cng"]
        );
        let disabled = CargoSelection {
            no_default_features: true,
            ..selected
        };
        assert!(
            ProjectDependencyGraph::resolve_with_selection(
                &f.0.join("Cargo.toml"),
                "app",
                &disabled
            )
            .unwrap()
            .cng_plugins
            .is_empty()
        );
        let lock = std::fs::read(f.0.join("Cargo.lock")).unwrap();
        assert_eq!(
            f.modules(&[], true, "aarch64-apple-ios"),
            Vec::<String>::new()
        );
        assert_eq!(f.modules(&["store"], true, "aarch64-apple-ios"), ["store"]);
        assert_eq!(
            f.modules(&["store"], false, "aarch64-apple-ios"),
            ["auth", "store"]
        );
        assert_eq!(
            f.modules(&[], false, "x86_64-pc-windows-msvc"),
            ["auth", "windows"]
        );
        assert_eq!(std::fs::read(f.0.join("Cargo.lock")).unwrap(), lock);
    }
    #[test]
    fn native_build_rejects_stale_features_and_platform_specific_modules() {
        let f = Fixture::new();
        let platform = crate::GenerationTarget::Ios;
        let selection = CargoSelection::default().for_platform(platform);
        selection.save(&f.0.join("app/gen/ios")).unwrap();
        let graph = ProjectDependencyGraph::resolve_with_selection(
            &f.0.join("Cargo.toml"),
            "app",
            &selection,
        )
        .unwrap();
        graph.save(&f.0.join("app/gen/ios")).unwrap();
        assert!(
            ProjectDependencyGraph::native_build_selection(
                &f.0,
                "app",
                platform,
                "aarch64-apple-ios-sim",
                &[]
            )
            .is_ok()
        );
        assert!(
            ProjectDependencyGraph::native_build_selection(
                &f.0,
                "app",
                platform,
                "x86_64-pc-windows-msvc",
                &[]
            )
            .is_err()
        );
        assert!(
            ProjectDependencyGraph::native_build_selection(
                &f.0,
                "app",
                platform,
                "aarch64-apple-ios",
                &["store".into()]
            )
            .is_err()
        );
        let mut disabled = selection.clone();
        disabled.no_default_features = true;
        disabled.save(&f.0.join("app/gen/ios")).unwrap();
        assert!(
            ProjectDependencyGraph::native_build_selection(
                &f.0,
                "app",
                platform,
                "aarch64-apple-ios",
                &[]
            )
            .is_err()
        );
        ProjectDependencyGraph::resolve_with_selection(&f.0.join("Cargo.toml"), "app", &disabled)
            .unwrap()
            .save(&f.0.join("app/gen/ios"))
            .unwrap();
        assert_eq!(
            ProjectDependencyGraph::native_build_selection(
                &f.0,
                "app",
                platform,
                "aarch64-apple-ios",
                &[]
            )
            .unwrap(),
            disabled
        );
    }
}
