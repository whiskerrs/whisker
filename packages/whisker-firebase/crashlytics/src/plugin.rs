use anyhow::Result;
use serde::{Deserialize, Serialize};
use whisker_firebase_core::__private::{android_app, ios_app_target, set_collection_default};
use whisker_plugin::{PluginConfig, project::*};

const GRADLE_PLUGIN: &str = "com.google.firebase.crashlytics";
const GRADLE_PLUGIN_VERSION: &str = "3.0.8";

/// Crashlytics project settings. Adding the crate enables it with the defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WhiskerFirebaseCrashlyticsConfig {
    /// Automatic report sending at first launch, before any `set_collection_enabled`
    /// call. `Some(false)` keeps reports on the device until the app opts in;
    /// `None` keeps the SDK default (enabled).
    pub collection_enabled: Option<bool>,
    /// Upload the iOS app's dSYMs after Release builds so crashes are symbolicated.
    pub upload_symbols: bool,
}

impl Default for WhiskerFirebaseCrashlyticsConfig {
    fn default() -> Self {
        Self {
            collection_enabled: None,
            upload_symbols: true,
        }
    }
}

impl PluginConfig for WhiskerFirebaseCrashlyticsConfig {
    const NAME: &'static str = "whisker-firebase-crashlytics";
}

pub struct WhiskerFirebaseCrashlytics;

impl ProjectPlugin for WhiskerFirebaseCrashlytics {
    type Config = WhiskerFirebaseCrashlyticsConfig;

    /// The Crashlytics Gradle plugin must be applied after Google Services.
    fn after(&self) -> &'static [&'static str] {
        &["whisker-firebase"]
    }

    fn contribute(&self, context: &ProjectContext, config: &Self::Config) -> Result<ProjectUpdate> {
        let mut project = context.project.clone();
        match &mut project {
            ProjectIr::Android(android) => {
                // The SDK throws at startup without the build ID this plugin generates.
                android
                    .settings
                    .plugin_management
                    .plugins
                    .insert(GRADLE_PLUGIN.into(), GRADLE_PLUGIN_VERSION.into());
                let plugins = &mut android_app(android)?.build.plugins;
                if !plugins.iter().any(|plugin| plugin.id == GRADLE_PLUGIN) {
                    plugins.push(GradlePlugin {
                        id: GRADLE_PLUGIN.into(),
                        version: None,
                        alias: None,
                        apply: true,
                    });
                }
            }
            ProjectIr::Ios(ios) => {
                if config.upload_symbols {
                    ios_app_target(ios)?.scripts.push(upload_symbols_script());
                }
            }
            _ => return Ok(ProjectUpdate::Keep),
        }
        if let Some(enabled) = config.collection_enabled {
            set_collection_default(
                &mut project,
                "FirebaseCrashlyticsCollectionEnabled",
                "firebase_crashlytics_collection_enabled",
                enabled,
            )?;
        }
        Ok(ProjectUpdate::Replace {
            project: Box::new(project),
            reason: "Configure Firebase Crashlytics".into(),
        })
    }
}

fn upload_symbols_script() -> AppleBuildScript {
    let expression = |expression: &str| AppleBuildPath::Expression {
        expression: expression.into(),
    };
    AppleBuildScript {
        name: "Upload Crashlytics symbols".into(),
        position: AppleScriptPosition::AfterResources,
        shell: "/bin/sh".into(),
        script: r#"[ "$CONFIGURATION" = "Release" ] || exit 0
run="${BUILD_DIR%/Build/*}/SourcePackages/checkouts/firebase-ios-sdk/Crashlytics/run"
if [ ! -x "$run" ]; then
  echo "warning: Crashlytics upload script not found at $run"
  exit 0
fi
"$run"
"#
        .into(),
        inputs: vec![
            expression("${DWARF_DSYM_FOLDER_PATH}/${DWARF_DSYM_FILE_NAME}"),
            expression(
                "${DWARF_DSYM_FOLDER_PATH}/${DWARF_DSYM_FILE_NAME}/Contents/Resources/DWARF/${PRODUCT_NAME}",
            ),
            expression("${DWARF_DSYM_FOLDER_PATH}/${DWARF_DSYM_FILE_NAME}/Contents/Info.plist"),
            expression(
                "$(TARGET_BUILD_DIR)/$(UNLOCALIZED_RESOURCES_FOLDER_PATH)/GoogleService-Info.plist",
            ),
            expression("$(TARGET_BUILD_DIR)/$(EXECUTABLE_PATH)"),
        ],
        outputs: vec![],
        input_file_lists: vec![],
        output_file_lists: vec![],
        based_on_dependency_analysis: Some(false),
    }
}
