//! Existing mobile helpers contributing to the main iOS target.
use super::*;
use whisker_plugin::{GenerateContext, Plugin, PluginConfig, project::*};
pub(crate) fn contribution<P: Plugin>(
    plugin: &P,
    ctx: &ProjectContext,
    cfg: &P::Config,
) -> Result<ProjectUpdate> {
    let ProjectIr::Ios(original) = &ctx.project else {
        return Ok(ProjectUpdate::Keep);
    };
    let mut scratch = GenerateContext {
        ios: Some(Default::default()),
        app_crate_dir: ctx.app_crate_dir.clone(),
        ..Default::default()
    };
    Plugin::apply(plugin, &mut scratch, cfg)?;
    if scratch.journal.records.is_empty() {
        return Ok(ProjectUpdate::Keep);
    }
    let delta = scratch.ios.expect("iOS context");
    let mut project = original.clone();
    let target = project
        .apple
        .targets
        .get_mut(&project.apple.application)
        .context("missing primary iOS target")?;
    target.info_plist.extend(
        delta
            .info_plist
            .iter()
            .map(|(k, v)| (k.clone(), application::legacy_value(v))),
    );
    application::apply_ops(target, &delta.pbxproj_ops)?;
    for (path, entry) in delta.extra_files {
        crate::render::validate_extra_file_path(&path)?;
        project.apple.files.insert(
            ProjectPath::new(path.to_str().context("non-UTF-8 plugin path")?)?,
            ProjectFile::Generated { entry },
        );
    }
    if project == *original {
        Ok(ProjectUpdate::Keep)
    } else {
        Ok(ProjectUpdate::Replace {
            project: Box::new(ProjectIr::Ios(project)),
            reason: format!(
                "apply explicit {} configuration to the main iOS target",
                P::Config::NAME
            ),
        })
    }
}
impl crate::ProjectEngine {
    /// Start with the standard iOS application, followed by the existing mobile
    /// configuration helpers. Android-only configurations contribute nothing.
    pub fn with_ios_application(inputs: IosInputs) -> Self {
        let mut engine = Self::with_initializer(application::ApplicationPlugin::new(inputs));
        engine.register_mobile_builtins();
        engine
    }
}
