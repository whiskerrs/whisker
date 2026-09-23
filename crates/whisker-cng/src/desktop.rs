//! Cargo Host generation and distribution plans for Windows and Linux.
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};
use whisker_config::Config;
use whisker_plugin::project::*;

pub mod application;
mod metadata;
mod project_render;
pub use project_render::{
    DesktopBuildPlan, DesktopProjectInputs, distribution_files, load_build_plan, render_project,
    sync_project,
};

/// Resolved application inputs shared by the two Cargo Host initializers.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DesktopInputs {
    pub platform: crate::GenerationTarget,
    pub app_name: String,
    pub app_id: String,
    pub version: String,
    pub build_number: u32,
    pub background: String,
    pub generated_package: String,
    pub user_package: String,
    pub user_crate_path: PathBuf,
    pub host_dependency: String,
    pub desktop_dependency: String,
    pub element_modules: Vec<crate::RustElementModuleInput>,
    pub cargo_selection: crate::CargoSelection,
    pub app_icon_png: Option<Vec<u8>>,
}
impl DesktopInputs {
    pub fn empty_project(&self) -> Result<ProjectIr> {
        match self.platform {
            crate::GenerationTarget::Windows => Ok(ProjectIr::Windows(Default::default())),
            crate::GenerationTarget::Linux => Ok(ProjectIr::Linux(Default::default())),
            _ => anyhow::bail!("DesktopInputs requires Windows or Linux"),
        }
    }
}
pub fn inputs_from(
    config: &Config,
    platform: crate::GenerationTarget,
    package: String,
    root: PathBuf,
    host_dependency: String,
) -> Result<DesktopInputs> {
    ensure!(
        matches!(
            platform,
            crate::GenerationTarget::Windows | crate::GenerationTarget::Linux
        ),
        "expected Windows or Linux"
    );
    let app_name = config.name.clone().context("app.name is required")?;
    let app_id = config
        .bundle_id
        .clone()
        .context("app.bundle_id is required for desktop metadata")?;
    ensure!(!app_name.trim().is_empty(), "empty application name");
    ensure!(
        app_id.split('.').all(|p| !p.is_empty()
            && p.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')),
        "invalid desktop application ID"
    );
    let app_icon_png = crate::macos::icon::source(config, &root)?;
    Ok(DesktopInputs {
        platform,
        app_name,
        app_id,
        version: config.version.clone().unwrap_or_else(|| "1.0.0".into()),
        build_number: config.build_number.unwrap_or(1),
        background: crate::background::AppBackground::resolve(config)?
            .hex()
            .into(),
        generated_package: format!("{package}-whisker-{}", platform.as_str()),
        user_package: package,
        user_crate_path: root,
        host_dependency,
        desktop_dependency: format!("{:?}", env!("CARGO_PKG_VERSION")),
        element_modules: vec![],
        cargo_selection: crate::CargoSelection::default().for_platform(platform),
        app_icon_png,
    })
}

pub(crate) fn write_entry(path: &Path, entry: &whisker_plugin::FileEntry) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("output needs a parent")?)?;
    std::fs::write(path, entry.to_bytes()?)?;
    #[cfg(unix)]
    if let Some(mode) = entry.mode {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}
