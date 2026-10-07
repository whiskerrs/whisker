use anyhow::Result;
use serde::{Deserialize, Serialize};
use whisker_firebase_core::__private::{ios_app_target, set_collection_default};
use whisker_plugin::{PluginConfig, project::*};

/// Analytics project settings. Adding the crate enables it with the defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WhiskerFirebaseAnalyticsConfig {
    /// Collection at first launch, before any `set_collection_enabled` call.
    /// `Some(false)` collects nothing until the app enables it (e.g. after consent);
    /// `None` keeps the SDK default (enabled).
    pub collection_enabled: Option<bool>,
}

impl PluginConfig for WhiskerFirebaseAnalyticsConfig {
    const NAME: &'static str = "whisker-firebase-analytics";
}

pub struct WhiskerFirebaseAnalytics;

impl ProjectPlugin for WhiskerFirebaseAnalytics {
    type Config = WhiskerFirebaseAnalyticsConfig;

    fn contribute(&self, context: &ProjectContext, config: &Self::Config) -> Result<ProjectUpdate> {
        let mut project = context.project.clone();
        match &mut project {
            ProjectIr::Ios(ios) => {
                // GoogleAppMeasurement is a static binary target whose Objective-C categories
                // are dropped without -ObjC, silently disabling Analytics.
                let target = ios_app_target(ios)?;
                let flags = target
                    .build_settings
                    .entry("OTHER_LDFLAGS".into())
                    .or_insert_with(|| AppleBuildSetting::String("$(inherited)".into()));
                match flags {
                    AppleBuildSetting::String(flags) => {
                        if !flags.split_whitespace().any(|flag| flag == "-ObjC") {
                            flags.push_str(" -ObjC");
                        }
                    }
                    AppleBuildSetting::List(flags) => {
                        if !flags.iter().any(|flag| flag == "-ObjC") {
                            flags.push("-ObjC".into());
                        }
                    }
                }
            }
            ProjectIr::Android(_) if config.collection_enabled.is_some() => {}
            _ => return Ok(ProjectUpdate::Keep),
        }
        if let Some(enabled) = config.collection_enabled {
            set_collection_default(
                &mut project,
                "FIREBASE_ANALYTICS_COLLECTION_ENABLED",
                "firebase_analytics_collection_enabled",
                enabled,
            )?;
        }
        Ok(ProjectUpdate::Replace {
            project: Box::new(project),
            reason: "Configure Google Analytics for Firebase".into(),
        })
    }
}
