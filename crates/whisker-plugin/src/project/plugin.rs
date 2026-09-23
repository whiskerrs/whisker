//! Plugin contributions to a single platform project.

use super::ProjectIr;
use crate::PluginConfig;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Read-only input to one project plugin invocation.
///
/// The initial project may be empty or incomplete. Application plugins supply
/// the main products and defaults before dependent plugins run. Plugins read
/// the current project, not a second copy of the original application settings
/// that could undo earlier contributions. The engine owns this context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectContext {
    /// Initial declarations plus all preceding plugin contributions for one platform.
    pub project: ProjectIr,
    /// Consuming app crate root for resolving user-relative input files.
    pub app_crate_dir: Option<PathBuf>,
}

/// A typed plugin for the declarative project pipeline.
///
/// This contract is separate from the legacy mobile `crate::Plugin`. Register
/// it with CNG's ProjectEngine. Android, iOS, macOS, Windows, Linux, and Web generation discover this contract via
/// metadata protocol = "project". Every registered plugin runs, with default configuration when
/// absent. Plugins unrelated to the selected platform should return Keep.
pub trait ProjectPlugin {
    /// Serializable user configuration; its NAME is the stable plugin identity.
    type Config: PluginConfig;

    /// Registered plugins that must execute first. Missing names are errors.
    fn after(&self) -> &'static [&'static str] {
        &[]
    }

    /// Registered plugins that must execute later. Missing names are errors.
    fn before(&self) -> &'static [&'static str] {
        &[]
    }

    /// Validate configuration immediately before this plugin contributes.
    /// External side effects from earlier invocations cannot be rolled back.
    fn validate(&self, _config: &Self::Config) -> Result<()> {
        Ok(())
    }

    /// Return a contribution without mutating the engine's context.
    /// Use Merge for additive declarations and Replace for intentional edits.
    /// File reads are allowed; generation writes belong to the renderer.
    fn contribute(&self, context: &ProjectContext, config: &Self::Config) -> Result<ProjectUpdate>;
}

/// One plugin's explicit operation on the current project.
///
/// Merge checks platform-specific declaration identities and scalar conflicts.
/// Replace is an escape hatch for edits/deletions, including XML selectors: clone
/// the supplied project, edit the clone, and return it with a nonempty reason.
/// A replacement is the entire project; retaining unrelated declarations is the
/// author's responsibility. The engine records its identity and reason. Neither
/// operation may switch platforms. Structural validation runs after all plugins,
/// so contributions may temporarily reference objects supplied by later plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectUpdate {
    /// Contribute nothing.
    Keep,
    /// Atomically merge declarations using ProjectIr::merge_from.
    Merge {
        /// Complete or partial declarations for the current platform.
        project: Box<ProjectIr>,
    },
    /// Intentionally replace the current project, including removals.
    Replace {
        /// Result derived from the supplied context.
        project: Box<ProjectIr>,
        /// Human-readable explanation, retained in the composition report.
        reason: String,
    },
}

impl ProjectUpdate {
    /// Apply atomically; an error leaves the current project unchanged.
    /// This does not validate graph completeness or native build compatibility.
    pub fn apply_to(&self, current: &mut ProjectIr) -> Result<()> {
        match self {
            Self::Keep => Ok(()),
            Self::Merge { project } => current.merge_from(project),
            Self::Replace { project, reason } => {
                ensure!(
                    !reason.trim().is_empty(),
                    "project replacement requires a reason"
                );
                ensure!(
                    std::mem::discriminant(current) == std::mem::discriminant(project.as_ref()),
                    "plugin cannot change the project platform"
                );
                *current = project.as_ref().clone();
                Ok(())
            }
        }
    }
}
