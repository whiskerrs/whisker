use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use whisker_plugin::{
    PluginConfig,
    project::{ProjectContext, ProjectPlugin, ProjectUpdate},
};

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureConfig {
    pub update: Option<ProjectUpdate>,
    pub expected: Option<ProjectContext>,
    #[serde(default)]
    pub reject: bool,
}
impl PluginConfig for FixtureConfig {
    const NAME: &'static str = "fixture-project";
}
pub struct Fixture;
impl ProjectPlugin for Fixture {
    type Config = FixtureConfig;
    fn validate(&self, config: &FixtureConfig) -> Result<()> {
        ensure!(!config.reject, "fixture configuration rejected");
        Ok(())
    }
    fn contribute(
        &self,
        context: &ProjectContext,
        config: &FixtureConfig,
    ) -> Result<ProjectUpdate> {
        if let Some(expected) = &config.expected {
            ensure!(
                context == expected,
                "preceding declarations or app root were lost"
            );
        }
        Ok(config.update.clone().unwrap_or(ProjectUpdate::Keep))
    }
}
