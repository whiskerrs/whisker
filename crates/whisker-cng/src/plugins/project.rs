//! Declarative adapters for the shared mobile configuration helpers.
use super::*;
use anyhow::Result;
use whisker_plugin::{Plugin, project::*};

fn contribution<P: Plugin>(
    plugin: &P,
    context: &ProjectContext,
    config: &P::Config,
) -> Result<ProjectUpdate> {
    match &context.project {
        ProjectIr::Android(_) => crate::android::builtins::contribution(plugin, context, config),
        ProjectIr::Ios(_) => crate::ios::builtins::contribution(plugin, context, config),
        _ => Ok(ProjectUpdate::Keep),
    }
}

macro_rules! project_builtin {
    ($($ty:ty),+ $(,)?) => {$(
        impl ProjectPlugin for $ty {
            type Config = <Self as Plugin>::Config;
            fn after(&self) -> &'static [&'static str] { Plugin::after(self) }
            fn before(&self) -> &'static [&'static str] { Plugin::before(self) }
            fn validate(&self, config: &<Self as ProjectPlugin>::Config) -> Result<()> { Plugin::validate(self,config) }
            fn contribute(&self, context: &ProjectContext, config: &<Self as ProjectPlugin>::Config) -> Result<ProjectUpdate> { contribution(self,context,config) }
        }
    )+};
}
project_builtin!(
    android_permissions::AndroidPermissions,
    android_meta_data::AndroidMetaData,
    android_application_attributes::AndroidApplicationAttributes,
    android_gradle_plugins::GradlePlugins,
    android_gradle_dependencies::GradleDependencies,
    android_extra_files::AndroidExtraFiles,
    app_icon::AppIcon,
    info_plist_extra::InfoPlistExtra,
    ios_extra_files::IosExtraFiles,
    ios_pbxproj_ops::IosPbxprojOps
);

impl crate::ProjectEngine {
    pub(crate) fn register_mobile_builtins(&mut self) {
        self.register(android_permissions::AndroidPermissions)
            .register(android_meta_data::AndroidMetaData)
            .register(android_application_attributes::AndroidApplicationAttributes)
            .register(android_gradle_plugins::GradlePlugins)
            .register(android_gradle_dependencies::GradleDependencies)
            .register(android_extra_files::AndroidExtraFiles)
            .register(app_icon::AppIcon)
            .register(info_plist_extra::InfoPlistExtra)
            .register(ios_extra_files::IosExtraFiles)
            .register(ios_pbxproj_ops::IosPbxprojOps);
    }
}
