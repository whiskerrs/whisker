use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;
use whisker_plugin::{PluginConfig, project::*};

/// App-relative Firebase configuration files, downloaded from the Firebase console.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WhiskerFirebaseConfig {
    pub android_config: String,
    pub ios_config: String,
}
impl Default for WhiskerFirebaseConfig {
    fn default() -> Self {
        Self {
            android_config: "google-services.json".into(),
            ios_config: "GoogleService-Info.plist".into(),
        }
    }
}
impl PluginConfig for WhiskerFirebaseConfig {
    const NAME: &'static str = "whisker-firebase";
}

/// Installs shared Firebase application configuration in the main mobile target.
/// Service SDKs belong to optional module crates, not to this plugin.
pub struct WhiskerFirebase;
impl ProjectPlugin for WhiskerFirebase {
    type Config = WhiskerFirebaseConfig;

    fn contribute(&self, context: &ProjectContext, config: &Self::Config) -> Result<ProjectUpdate> {
        let mut project = context.project.clone();
        match &mut project {
            ProjectIr::Android(android) => {
                let source = ProjectPath::new(&config.android_config)?;
                let bytes = input(context, &source)?;
                let json: serde_json::Value = serde_json::from_slice(&bytes)
                    .context("whisker-firebase: invalid google-services.json")?;
                let module = android
                    .modules
                    .get_mut(&android.application)
                    .context("whisker-firebase: main Android application is missing")?;
                let AndroidModuleKind::Application(app) = &mut module.kind else {
                    anyhow::bail!("whisker-firebase requires an Android application module");
                };
                let id = app
                    .application_id
                    .as_deref()
                    .context("Firebase requires an application ID")?;
                ensure!(
                    json["client"]
                        .as_array()
                        .is_some_and(|clients| clients.iter().any(|client| client["client_info"]
                            ["android_client_info"]["package_name"]
                            .as_str()
                            == Some(id))),
                    "google-services.json has no client for Android application ID `{id}`"
                );
                ensure!(
                    json["project_info"]["project_id"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty()),
                    "google-services.json has no project_id"
                );
                if matches!(
                    app.android.sdk.min,
                    None | Some(AndroidApiLevel::Release(0..=22))
                ) {
                    app.android.sdk.min = Some(AndroidApiLevel::Release(23));
                }
                let destination = ProjectPath::new(format!(
                    "{}/google-services.json",
                    module.directory.as_str()
                ))?;
                let id = android.application.clone();
                let mut addition = AndroidProjectIr::default();
                let mut target = module.clone();
                target.build.plugins.push(GradlePlugin {
                    id: "com.google.gms.google-services".into(),
                    version: None,
                    alias: None,
                    apply: true,
                });
                addition.modules.insert(id, target);
                addition
                    .settings
                    .plugin_management
                    .plugins
                    .insert("com.google.gms.google-services".into(), "4.5.0".into());
                addition
                    .files
                    .insert(destination, ProjectFile::AppFile { source });
                android.merge_from(&addition)?;
            }
            ProjectIr::Ios(ios) => {
                let source = ProjectPath::new(&config.ios_config)?;
                let bytes = input(context, &source)?;
                let value = plist::Value::from_reader(std::io::Cursor::new(bytes))
                    .context("whisker-firebase: invalid GoogleService-Info.plist")?;
                let values = value
                    .as_dictionary()
                    .context("Firebase plist must be a dictionary")?;
                for key in ["GOOGLE_APP_ID", "API_KEY", "PROJECT_ID", "BUNDLE_ID"] {
                    ensure!(
                        values
                            .get(key)
                            .and_then(plist::Value::as_string)
                            .is_some_and(|v| !v.is_empty()),
                        "GoogleService-Info.plist is missing `{key}`"
                    );
                }
                let target = ios
                    .apple
                    .targets
                    .get(&ios.apple.application)
                    .context("whisker-firebase: main iOS application is missing")?;
                let id = target.build_settings.get("PRODUCT_BUNDLE_IDENTIFIER");
                let Some(AppleBuildSetting::String(id)) = id else {
                    anyhow::bail!("Firebase requires a literal PRODUCT_BUNDLE_IDENTIFIER");
                };
                ensure!(
                    values["BUNDLE_ID"].as_string() == Some(id.as_str()),
                    "GoogleService-Info.plist BUNDLE_ID does not match `{id}`"
                );
                let destination = ProjectPath::new("firebase/GoogleService-Info.plist")?;
                let mut addition = IosProjectIr::default();
                let mut target = target.clone();
                target.resources.push(AppleResource::Copy {
                    resource: Resource {
                        source: destination.clone(),
                        destination: ProjectPath::new("GoogleService-Info.plist")?,
                        kind: ResourceKind::File,
                    },
                });
                addition
                    .apple
                    .targets
                    .insert(ios.apple.application.clone(), target);
                addition
                    .apple
                    .files
                    .insert(destination, ProjectFile::AppFile { source });
                ios.merge_from(&addition)?;
            }
            _ => return Ok(ProjectUpdate::Keep),
        }
        Ok(ProjectUpdate::Replace {
            project: Box::new(project),
            reason: "Configure the main Firebase app and enforce the Android SDK minimum".into(),
        })
    }
}

fn input(context: &ProjectContext, source: &ProjectPath) -> Result<Vec<u8>> {
    let root = context
        .app_crate_dir
        .as_deref()
        .context("Firebase configuration needs an app crate directory")?;
    let mut path = root.to_path_buf();
    for part in Path::new(source.as_str()).components() {
        path.push(part);
        ensure!(
            !std::fs::symlink_metadata(&path)
                .with_context(|| format!(
                    "Firebase configuration file unavailable: {}",
                    path.display()
                ))?
                .file_type()
                .is_symlink(),
            "Firebase configuration must not contain symlinks"
        );
    }
    std::fs::read(&path).with_context(|| format!("read Firebase configuration {}", path.display()))
}
