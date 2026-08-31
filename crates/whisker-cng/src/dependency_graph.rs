//! Canonical Cargo dependency graph used by every CNG target.

use std::path::Path;

use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;

use crate::discovery::{DiscoveredPlugin, discover_plugins_from_metadata};
use crate::modules::{ResolvedModule, discover_from_metadata};

/// All Whisker-specific contributions resolved from one Cargo graph snapshot.
#[derive(Debug, Clone)]
pub struct ProjectDependencyGraph {
    /// Runtime/native modules selected by the application dependency graph.
    pub modules: Vec<ResolvedModule>,
    /// Generation-time plugins activated by those same dependencies.
    pub cng_plugins: Vec<DiscoveredPlugin>,
}

impl ProjectDependencyGraph {
    /// Resolve both native modules and CNG plugins with one `cargo metadata`
    /// invocation so every target observes the same activated packages.
    pub fn resolve(manifest_path: &Path, app_package: &str) -> Result<Self> {
        let metadata = MetadataCommand::new()
            .manifest_path(manifest_path)
            .exec()
            .with_context(|| {
                format!(
                    "cargo metadata failed for {} (package: {app_package})",
                    manifest_path.display(),
                )
            })?;
        Ok(Self {
            modules: discover_from_metadata(&metadata, app_package)
                .context("resolve Whisker modules")?,
            cng_plugins: discover_plugins_from_metadata(&metadata, app_package)
                .context("resolve Whisker CNG plugins")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_project_snapshot_contains_runtime_modules_and_cng_plugins() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("Cargo.toml");
        let graph = ProjectDependencyGraph::resolve(&workspace, "host-smoke").unwrap();
        assert!(
            graph
                .modules
                .iter()
                .any(|module| module.package == "whisker-toggle")
        );
        assert_eq!(graph.cng_plugins.len(), 0);
    }
}
