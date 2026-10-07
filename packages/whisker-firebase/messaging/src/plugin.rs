use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use whisker_plugin::{PluginConfig, project::*};

/// Push notification setup for the main iOS app. Android needs no project changes:
/// the messaging module's library manifest declares its service and permission.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WhiskerFirebaseMessagingConfig {
    /// The `aps-environment` entitlement: `development` or `production`. Xcode switches
    /// to production when exporting for the App Store, so development suits most apps.
    pub aps_environment: String,
    /// Add the `remote-notification` background mode so iOS can deliver messages while
    /// the app is suspended.
    pub background_remote_notifications: bool,
}

impl Default for WhiskerFirebaseMessagingConfig {
    fn default() -> Self {
        Self {
            aps_environment: "development".into(),
            background_remote_notifications: true,
        }
    }
}

impl PluginConfig for WhiskerFirebaseMessagingConfig {
    const NAME: &'static str = "whisker-firebase-messaging";
}

pub struct WhiskerFirebaseMessaging;

impl ProjectPlugin for WhiskerFirebaseMessaging {
    type Config = WhiskerFirebaseMessagingConfig;

    fn validate(&self, config: &Self::Config) -> Result<()> {
        ensure!(
            matches!(
                config.aps_environment.as_str(),
                "development" | "production"
            ),
            "whisker-firebase-messaging: aps_environment must be `development` or `production`"
        );
        Ok(())
    }

    fn contribute(&self, context: &ProjectContext, config: &Self::Config) -> Result<ProjectUpdate> {
        let ProjectIr::Ios(ios) = &context.project else {
            return Ok(ProjectUpdate::Keep);
        };
        let mut ios = ios.clone();
        let target = ios
            .apple
            .targets
            .get_mut(&ios.apple.application)
            .context("whisker-firebase-messaging: main iOS application is missing")?;
        target.entitlements.insert(
            "aps-environment".into(),
            PropertyListValue::String(config.aps_environment.clone()),
        );
        if config.background_remote_notifications {
            let mode = PropertyListValue::String("remote-notification".into());
            match target
                .info_plist
                .entry("UIBackgroundModes".into())
                .or_insert_with(|| PropertyListValue::Array(Vec::new()))
            {
                PropertyListValue::Array(modes) => {
                    if !modes.contains(&mode) {
                        modes.push(mode);
                    }
                }
                _ => anyhow::bail!("UIBackgroundModes must be an array"),
            }
        }
        Ok(ProjectUpdate::Replace {
            project: Box::new(ProjectIr::Ios(ios)),
            reason: "Enable push notifications for Firebase Cloud Messaging".into(),
        })
    }
}
