//! Cargo inputs shared by discovery, generation, and native build drivers.

#[cfg(feature = "generate")]
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
#[cfg(feature = "generate")]
use std::path::{Path, PathBuf};

/// Application features and target used to generate a platform project.
/// An absent target uses the generator's platform default, never Cargo's host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CargoSelection {
    pub features: Vec<String>,
    pub no_default_features: bool,
    pub target: Option<String>,
}

impl CargoSelection {
    pub fn apply(&self, command: &mut std::process::Command) {
        if self.no_default_features {
            command.arg("--no-default-features");
        }
        for feature in &self.features {
            command.arg("--features").arg(feature);
        }
    }

    pub fn for_platform(&self, platform: crate::GenerationTarget) -> Self {
        let mut selection = self.clone();
        selection.features = self
            .features
            .iter()
            .flat_map(|s| s.split(|c: char| c == ',' || c.is_whitespace()))
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        selection.features.sort();
        selection.features.dedup();
        if selection.target.is_none() {
            selection.target = Some(
                match platform {
                    crate::GenerationTarget::Android => "aarch64-linux-android",
                    crate::GenerationTarget::Ios => "aarch64-apple-ios",
                    crate::GenerationTarget::Macos => {
                        if cfg!(target_arch = "x86_64") {
                            "x86_64-apple-darwin"
                        } else {
                            "aarch64-apple-darwin"
                        }
                    }
                    crate::GenerationTarget::Web => "wasm32-unknown-unknown",
                }
                .into(),
            );
        }
        selection
    }

    #[cfg(feature = "generate")]
    pub(crate) fn validate_platform(&self, platform: crate::GenerationTarget) -> Result<()> {
        let Some(target) = &self.target else {
            return Ok(());
        };
        let matches = match platform {
            crate::GenerationTarget::Android => target.contains("-linux-android"),
            crate::GenerationTarget::Ios => target.contains("-apple-ios"),
            crate::GenerationTarget::Macos => target.ends_with("-apple-darwin"),
            crate::GenerationTarget::Web => target == "wasm32-unknown-unknown",
        };
        anyhow::ensure!(
            matches,
            "Cargo target `{target}` does not match the {} project",
            platform.as_str()
        );
        Ok(())
    }

    #[cfg(feature = "generate")]
    pub(crate) fn project_state_dir(
        workspace: &Path,
        package: &str,
        platform: crate::GenerationTarget,
    ) -> Result<PathBuf> {
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(workspace.join("Cargo.toml"))
            .no_deps()
            .exec()?;
        let package = metadata
            .packages
            .iter()
            .find(|p| p.name == package)
            .context("application package not found in workspace")?;
        Ok(package
            .manifest_path
            .parent()
            .context("manifest has no parent")?
            .as_std_path()
            .join("gen")
            .join(platform.as_str())
            .join(".whisker"))
    }

    #[cfg(feature = "generate")]
    fn path(workspace: &Path, package: &str, platform: crate::GenerationTarget) -> Result<PathBuf> {
        Ok(Self::project_state_dir(workspace, package, platform)?.join("cargo-selection.json"))
    }

    /// Read Cargo inputs embedded in a generated project, if present.
    #[cfg(feature = "generate")]
    pub fn load_project(project_dir: &Path) -> Result<Option<Self>> {
        let path = project_dir.join(".whisker/cargo-selection.json");
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .with_context(|| format!("read {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
        }
    }

    /// Read the last generated application's build inputs. Missing files retain
    /// compatibility with native projects generated before selection support.
    #[cfg(feature = "generate")]
    pub fn load(
        workspace: &Path,
        package: &str,
        platform: crate::GenerationTarget,
    ) -> Result<Self> {
        let path = Self::path(workspace, package, platform)?;
        match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).with_context(|| format!("read {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(Self::default().for_platform(platform))
            }
            Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
        }
    }

    #[cfg(feature = "generate")]
    pub(crate) fn save(&self, project_dir: &Path) -> Result<()> {
        write_json(&project_dir.join(".whisker/cargo-selection.json"), self)
    }

    #[cfg(feature = "generate")]
    pub(crate) fn dependency_options(&self) -> String {
        format!(
            ", default-features = {}, features = {}",
            !self.no_default_features,
            toml::Value::Array(
                self.features
                    .iter()
                    .cloned()
                    .map(toml::Value::String)
                    .collect()
            )
        )
    }
}

#[cfg(feature = "generate")]
pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().context("JSON output has no parent")?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".cargo-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        std::fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
        std::fs::rename(&temporary, path).context("publish Cargo state")
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
