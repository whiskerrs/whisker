//! Asset declarations for the project plugin pipeline.
//!
//! Configure with `app.project_plugin::<WhiskerAsset>(|c| { c.dir("assets"); });`.
//! Paths are relative to the app crate. Directory contents retain their relative
//! paths; individual files use their basename. The plugin enumerates inputs and
//! returns AppFile declarations; renderers read, fingerprint, and copy the bytes.
//! Android uses the application module's main assets/whisker directory, iOS/macOS copy
//! a whisker_assets folder into the application bundle, and Web distributes the
//! logical paths at its configured URL prefix. Windows uses a sibling asset
//! directory; Linux uses `share/<executable>/whisker_assets`.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};
use whisker_plugin::{PluginConfig, project::*};

const IOS_NAMESPACE: &str = "whisker_assets";
const WEB_STAGING: &str = "whisker_assets";
pub(crate) const WEB_BASE_META: &str = "whisker-asset-base";

/// Asset roots relative to the app crate. Symlinks and escaping paths are rejected.
#[derive(Default, Serialize, Deserialize)]
pub struct WhiskerAssetConfig {
    /// Directories to bundle recursively. Every regular file beneath
    /// each entry is included, keyed by its path relative to that
    /// directory.
    #[serde(default)]
    pub dirs: Vec<PathBuf>,
    /// Individual files to bundle. Keyed by the file's basename (its
    /// path relative to its own parent directory).
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

impl WhiskerAssetConfig {
    /// Bundle a directory recursively. Path is relative to the app
    /// crate root, e.g. `c.dir("assets")`.
    pub fn dir(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.dirs.push(path.into());
        self
    }

    /// Bundle a single file. Path is relative to the app crate root,
    /// e.g. `c.file("branding/logo.png")`. The file bundles under its
    /// **basename** — `branding/logo.png` → `<ns>/logo.png` — so
    /// `resolve("logo.png")` finds it. Use [`Self::dir`] to preserve a
    /// subdirectory layout.
    pub fn file(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.files.push(path.into());
        self
    }
}

impl PluginConfig for WhiskerAssetConfig {
    const NAME: &'static str = "whisker-asset";
}

/// Bundles configured app assets through the declarative project pipeline.
pub struct WhiskerAsset;

// Normalize host separators without lossy Unicode conversion. Accept ./assets
// as before, while retaining ProjectPath's portable, app-relative contract.
fn project_path(path: &Path) -> Result<ProjectPath> {
    let mut parts = Vec::new();
    for part in path.components() {
        match part {
            Component::Normal(value) => parts.push(value.to_str().context("non-UTF-8 asset path")?),
            Component::CurDir => {}
            _ => anyhow::bail!(
                "whisker-asset: path `{}` must be relative to the app crate and contain no ..",
                path.display()
            ),
        }
    }
    ProjectPath::new(parts.join("/"))
}

// logical path -> app-relative source. Keeping sources in the IR lets the common
// staging layer preserve file modes and notice changes in bytes on regeneration.
fn collect(cfg: &WhiskerAssetConfig, root: &Path) -> Result<BTreeMap<ProjectPath, ProjectPath>> {
    let mut assets = BTreeMap::new();
    for (path, directory) in cfg
        .dirs
        .iter()
        .map(|p| (p, true))
        .chain(cfg.files.iter().map(|p| (p, false)))
    {
        let source = project_path(path)?;
        let mut abs = root.to_path_buf();
        for part in source.as_str().split('/') {
            abs.push(part);
            let metadata = std::fs::symlink_metadata(&abs).with_context(|| {
                format!(
                    "whisker-asset: declared input does not exist or cannot be read: {}",
                    abs.display()
                )
            })?;
            ensure!(
                !metadata.file_type().is_symlink(),
                "whisker-asset: symlink input is not supported: {}",
                abs.display()
            );
        }
        ensure!(
            if directory {
                abs.is_dir()
            } else {
                abs.is_file()
            },
            "whisker-asset: input kind mismatch: {}",
            abs.display()
        );
        let logical_root = if directory {
            abs.clone()
        } else {
            abs.parent().unwrap().to_path_buf()
        };
        collect_path(root, &logical_root, &abs, &mut assets)?;
    }
    Ok(assets)
}
fn collect_path(
    root: &Path,
    logical_root: &Path,
    path: &Path,
    assets: &mut BTreeMap<ProjectPath, ProjectPath>,
) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "whisker-asset: symlink input is not supported: {}",
        path.display()
    );
    if metadata.is_dir() {
        let mut entries = std::fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            collect_path(root, logical_root, &entry.path(), assets)?;
        }
    } else {
        ensure!(
            metadata.is_file(),
            "whisker-asset: input is not a regular file: {}",
            path.display()
        );
        let rel = project_path(path.strip_prefix(logical_root)?)?;
        let source = project_path(path.strip_prefix(root)?)?;
        if let Some(prior) = assets.get(&rel) {
            anyhow::bail!(
                "whisker-asset: two assets collide at `{}`: `{}` and `{}`",
                rel.as_str(),
                prior.as_str(),
                source.as_str()
            );
        }
        assets.insert(rel, source);
    }
    Ok(())
}
fn staged(assets: &BTreeMap<ProjectPath, ProjectPath>, prefix: &str) -> Result<ProjectFiles> {
    assets
        .iter()
        .map(|(rel, source)| {
            Ok((
                ProjectPath::new(format!("{prefix}/{}", rel.as_str()))?,
                ProjectFile::AppFile {
                    source: source.clone(),
                },
            ))
        })
        .collect()
}

impl ProjectPlugin for WhiskerAsset {
    type Config = WhiskerAssetConfig;
    fn validate(&self, cfg: &Self::Config) -> Result<()> {
        for path in cfg.dirs.iter().chain(&cfg.files) {
            project_path(path)?;
        }
        Ok(())
    }
    fn contribute(&self, ctx: &ProjectContext, cfg: &Self::Config) -> Result<ProjectUpdate> {
        if cfg.dirs.is_empty() && cfg.files.is_empty() {
            return Ok(ProjectUpdate::Keep);
        }
        self.validate(cfg)?;
        let root = ctx
            .app_crate_dir
            .as_deref()
            .context("whisker-asset: missing app crate dir")?;
        let assets = collect(cfg, root)?;
        if assets.is_empty() {
            return Ok(ProjectUpdate::Keep);
        }
        let project = match &ctx.project {
            ProjectIr::Android(current) => {
                let mut module = current
                    .modules
                    .get(&current.application)
                    .context("whisker-asset: missing Android application module")?
                    .clone();
                let assets_root =
                    ProjectPath::new(format!("{}/src/main/assets", module.directory.as_str()))?;
                let AndroidModuleKind::Application(app) = &mut module.kind else {
                    anyhow::bail!("whisker-asset: primary Android module is not an application");
                };
                let source_set = app.android.source_sets.entry("main".into()).or_default();
                if !source_set.assets.contains(&assets_root) {
                    source_set.assets.push(assets_root.clone());
                }
                ProjectIr::Android(Box::new(AndroidProjectIr {
                    modules: [(current.application.clone(), module)].into(),
                    files: staged(&assets, &format!("{}/whisker", assets_root.as_str()))?,
                    ..Default::default()
                }))
            }
            ProjectIr::Ios(current) => ProjectIr::Ios(IosProjectIr {
                apple: apple_assets(&current.apple, &assets)?,
            }),
            ProjectIr::Macos(current) => ProjectIr::Macos(MacosProjectIr {
                apple: apple_assets(&current.apple, &assets)?,
            }),
            ProjectIr::Web(current) => {
                ensure!(
                    !current.base_path.is_empty(),
                    "whisker-asset: Web application must declare base_path first"
                );
                let resources = assets
                    .keys()
                    .map(|rel| {
                        Ok(Resource {
                            source: ProjectPath::new(format!("{WEB_STAGING}/{}", rel.as_str()))?,
                            destination: rel.clone(),
                            kind: ResourceKind::File,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                ProjectIr::Web(Box::new(WebProjectIr {
                    files: staged(&assets, WEB_STAGING)?,
                    resources,
                    head: vec![WebHtmlContribution {
                        id: WEB_BASE_META.into(),
                        element: HtmlElement {
                            name: "meta".into(),
                            attributes: [
                                ("name".into(), Some(WEB_BASE_META.into())),
                                ("content".into(), Some(current.base_path.clone())),
                            ]
                            .into(),
                            children: vec![],
                        },
                    }],
                    ..Default::default()
                }))
            }
            ProjectIr::Windows(current) => {
                let exe = &current
                    .executables
                    .get(&current.application)
                    .context("whisker-asset: missing Windows application")?
                    .executable;
                let directory = exe
                    .destination
                    .as_str()
                    .rsplit_once('/')
                    .map(|(dir, _)| format!("{dir}/"))
                    .unwrap_or_default();
                ProjectIr::Windows(WindowsProjectIr {
                    files: staged(&assets, IOS_NAMESPACE)?,
                    resources: vec![Resource {
                        source: ProjectPath::new(IOS_NAMESPACE)?,
                        destination: ProjectPath::new(format!("{directory}{IOS_NAMESPACE}"))?,
                        kind: ResourceKind::Directory,
                    }],
                    ..Default::default()
                })
            }
            ProjectIr::Linux(current) => {
                let exe = current
                    .executables
                    .get(&current.application)
                    .context("whisker-asset: missing Linux application")?;
                let binary = exe
                    .destination
                    .as_str()
                    .strip_prefix("bin/")
                    .filter(|s| !s.contains('/'))
                    .context(
                        "whisker-asset: Linux application must be installed directly in bin/",
                    )?;
                ProjectIr::Linux(LinuxProjectIr {
                    files: staged(&assets, IOS_NAMESPACE)?,
                    resources: vec![Resource {
                        source: ProjectPath::new(IOS_NAMESPACE)?,
                        destination: ProjectPath::new(format!("share/{binary}/{IOS_NAMESPACE}"))?,
                        kind: ResourceKind::Directory,
                    }],
                    ..Default::default()
                })
            }
        };
        Ok(ProjectUpdate::Merge {
            project: Box::new(project),
        })
    }
}

fn apple_assets(
    current: &AppleProjectIr,
    assets: &BTreeMap<ProjectPath, ProjectPath>,
) -> Result<AppleProjectIr> {
    let mut target = current
        .targets
        .get(&current.application)
        .context("whisker-asset: missing Apple application target")?
        .clone();
    let folder = AppleResource::Copy {
        resource: Resource {
            source: ProjectPath::new(IOS_NAMESPACE)?,
            destination: ProjectPath::new(IOS_NAMESPACE)?,
            kind: ResourceKind::Directory,
        },
    };
    if !target.resources.contains(&folder) {
        target.resources.push(folder);
    }
    Ok(AppleProjectIr {
        targets: [(current.application.clone(), target)].into(),
        files: staged(assets, IOS_NAMESPACE)?,
        ..Default::default()
    })
}
