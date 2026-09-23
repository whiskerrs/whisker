//! Declarative project composition, independent of the legacy renderer pipeline.
//!
//! The caller supplies a platform composition input, which may be empty.
//! An initializer can declare the application through the same plugin contract.
//! ProjectEngine runs typed or versioned subprocess plugins, owns
//! merging/replacement, and validates the final graph. It never writes projects
//! itself. Android, iOS, macOS, Windows, Linux, and Web generation register their application plugins and consume the result.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use whisker_config::Config;
use whisker_plugin::project::protocol::{self, Descriptor, Request, Response};
use whisker_plugin::project::{ProjectContext, ProjectIr, ProjectPlugin, ProjectUpdate};

/// Engine-owned account of each plugin operation, in execution order.
/// This is not a field-level ownership journal or a renderer fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectStep {
    /// Plugin returned no contribution.
    Keep {
        /// Stable plugin identity.
        plugin: String,
    },
    /// Declarations were merged with conflict checks.
    Merge {
        /// Stable plugin identity.
        plugin: String,
    },
    /// Plugin deliberately replaced the project.
    Replace {
        /// Stable plugin identity.
        plugin: String,
        /// Explanation supplied by the plugin.
        reason: String,
    },
}

/// A structurally valid project and the operations used to produce it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectComposition {
    /// Final declarations. Native schema/build validation belongs to a backend.
    pub project: ProjectIr,
    /// Engine-owned trace; subprocess responses cannot rewrite earlier steps.
    pub steps: Vec<ProjectStep>,
}

/// Registry for the declarative project plugin contract.
///
/// All subprocesses are preflighted before any plugin contribution executes.
/// Ordering is shared with the legacy engine: duplicate names, missing ordering
/// references, and cycles are errors. Registered plugins run with default config
/// if absent; empty configuration is not an implicit enable/disable flag.
#[derive(Default)]
pub struct ProjectEngine {
    plugins: Vec<Box<dyn Runner>>,
    app_crate_dir: Option<PathBuf>,
    initializer: bool,
}

impl ProjectEngine {
    /// Create an empty registry, without any implicit application declarations.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry whose first plugin must precede every other plugin.
    ///
    /// The initializer uses the normal configuration, contribution, conflict
    /// checking, and reporting contract. It does not bypass preflight or final
    /// validation. Contradictory before/after constraints are ordering errors.
    /// No application or platform-specific defaults are supplied by the engine.
    pub fn with_initializer<P: ProjectPlugin + 'static>(plugin: P) -> Self {
        let mut engine = Self::new();
        engine.register(plugin);
        engine.initializer = true;
        engine
    }

    /// Set the app root passed as read-only input to every plugin.
    pub fn with_app_crate_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.app_crate_dir = Some(dir.into());
        self
    }

    /// Register a typed declarative project plugin.
    pub fn register<P: ProjectPlugin + 'static>(&mut self, plugin: P) -> &mut Self {
        self.plugins.push(Box::new(Typed(plugin)));
        self
    }

    /// Register a binary implementing the versioned project protocol.
    /// Its declared name must match `name`. Ordering comes from the binary's
    /// preflight descriptor, not a second caller-maintained set of constraints.
    pub fn register_subprocess(
        &mut self,
        name: impl Into<String>,
        binary: impl Into<PathBuf>,
    ) -> &mut Self {
        self.plugins.push(Box::new(Subprocess {
            name: name.into(),
            binary: binary.into(),
        }));
        self
    }

    /// Compose one platform, leaving the caller's input unchanged on error.
    ///
    /// `initial` may be incomplete, including an empty Android or iOS IR. An application
    /// plugin can supply defaults and main products. Only Config.plugins is
    /// consumed here; original app values are never reapplied after plugins.
    /// No renderer-specific defaults
    /// are invented. Graph references may be incomplete until the final check.
    ///
    /// Merge uses the IR's transactional declaration rules, including conflicts
    /// with initial values. Replace acknowledges an intentional edit of the
    /// whole current project. Plugin file/network side effects cannot be undone.
    pub fn compose(&self, config: &Config, initial: &ProjectIr) -> Result<ProjectComposition> {
        let descriptors = self
            .plugins
            .iter()
            .map(|p| p.describe())
            .collect::<Result<Vec<_>>>()
            .context("preflight project plugins")?;
        let order = crate::plugin_order::sort(
            &descriptors
                .iter()
                .enumerate()
                .map(|(index, p)| crate::plugin_order::Order {
                    name: &p.name,
                    after: p
                        .after
                        .iter()
                        .map(String::as_str)
                        .chain(
                            (self.initializer && index != 0).then(|| descriptors[0].name.as_str()),
                        )
                        .collect(),
                    before: p.before.iter().map(String::as_str).collect(),
                })
                .collect::<Vec<_>>(),
        )?;
        for name in config.plugins.keys() {
            ensure!(
                descriptors.iter().any(|p| &p.name == name),
                "project plugin `{name}` is configured but not registered"
            );
        }
        let mut context = ProjectContext {
            project: initial.clone(),
            app_crate_dir: self.app_crate_dir.clone(),
        };
        let mut steps = Vec::new();
        for index in order {
            let descriptor = &descriptors[index];
            let config = config.plugins.get(&descriptor.name).unwrap_or(&Value::Null);
            let update = self.plugins[index]
                .contribute(descriptor, &context, config)
                .with_context(|| format!("project plugin `{}` failed", descriptor.name))?;
            update.apply_to(&mut context.project).with_context(|| {
                format!(
                    "apply project plugin `{}` after initial input and plugins {:?}",
                    descriptor.name,
                    steps
                        .iter()
                        .map(|s: &ProjectStep| match s {
                            ProjectStep::Keep { plugin }
                            | ProjectStep::Merge { plugin }
                            | ProjectStep::Replace { plugin, .. } => plugin.as_str(),
                        })
                        .collect::<Vec<_>>()
                )
            })?;
            steps.push(match update {
                ProjectUpdate::Keep => ProjectStep::Keep {
                    plugin: descriptor.name.clone(),
                },
                ProjectUpdate::Merge { .. } => ProjectStep::Merge {
                    plugin: descriptor.name.clone(),
                },
                ProjectUpdate::Replace { reason, .. } => ProjectStep::Replace {
                    plugin: descriptor.name.clone(),
                    reason,
                },
            });
        }
        context
            .project
            .validate_structure()
            .context("validate composed project structure")?;
        Ok(ProjectComposition {
            project: context.project,
            steps,
        })
    }
}

trait Runner {
    fn describe(&self) -> Result<Descriptor>;
    fn contribute(
        &self,
        descriptor: &Descriptor,
        context: &ProjectContext,
        config: &Value,
    ) -> Result<ProjectUpdate>;
}

struct Typed<P>(P);
impl<P: ProjectPlugin> Runner for Typed<P> {
    fn describe(&self) -> Result<Descriptor> {
        Ok(Descriptor::of(&self.0))
    }
    fn contribute(
        &self,
        _: &Descriptor,
        context: &ProjectContext,
        config: &Value,
    ) -> Result<ProjectUpdate> {
        protocol::contribute(&self.0, context, config)
    }
}

struct Subprocess {
    name: String,
    binary: PathBuf,
}
impl Runner for Subprocess {
    fn describe(&self) -> Result<Descriptor> {
        match exchange(&self.binary, &self.name, &Request::Describe)? {
            Response::Describe { descriptor } => {
                ensure!(
                    descriptor.name == self.name,
                    "project plugin identity mismatch: expected `{}`, received `{}`",
                    self.name,
                    descriptor.name
                );
                Ok(descriptor)
            }
            _ => bail!(
                "project plugin `{}` returned a contribution during preflight",
                self.name
            ),
        }
    }
    fn contribute(
        &self,
        descriptor: &Descriptor,
        context: &ProjectContext,
        config: &Value,
    ) -> Result<ProjectUpdate> {
        let request = Request::Contribute {
            descriptor: descriptor.clone(),
            config: config.clone(),
            context: Box::new(context.clone()),
        };
        match exchange(&self.binary, &self.name, &request)? {
            Response::Contribute {
                descriptor: actual,
                update,
            } => {
                ensure!(
                    &actual == descriptor,
                    "project plugin `{}` changed its descriptor after preflight",
                    self.name
                );
                Ok(update)
            }
            _ => bail!(
                "project plugin `{}` returned a descriptor instead of a contribution",
                self.name
            ),
        }
    }
}

fn exchange(binary: &Path, name: &str, request: &Request) -> Result<Response> {
    let bytes = protocol::encode(request)?;
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn project plugin `{name}` ({})", binary.display()))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    // Drain stdout concurrently with writing stdin. A rejecting child may reply
    // before consuming a large request; a valid response may also exceed pipe size.
    let output = std::thread::scope(|scope| -> Result<_> {
        let writer = scope.spawn(move || stdin.write_all(&bytes));
        let output = child
            .wait_with_output()
            .with_context(|| format!("wait for project plugin `{name}`"))?;
        let written = writer
            .join()
            .map_err(|_| anyhow::anyhow!("project plugin writer panicked"))?;
        ensure!(
            output.status.success(),
            "project plugin `{name}` exited with {}; check stderr",
            output.status
        );
        written.with_context(|| format!("write request to project plugin `{name}`"))?;
        Ok(output)
    })?;
    protocol::decode(&output.stdout)
        .with_context(|| format!("response from project plugin `{name}`"))
}
