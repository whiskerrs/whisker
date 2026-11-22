//! Shared CNG plugin edits for service crates.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::path::Path;
use whisker_plugin::project::*;

/// Write a collection default read by the SDK at startup: an Info.plist key on iOS and
/// an `<application>` meta-data entry on Android.
pub fn set_collection_default(
    project: &mut ProjectIr,
    ios_key: &str,
    android_key: &str,
    enabled: bool,
) -> Result<()> {
    match project {
        ProjectIr::Ios(ios) => {
            ios_app_target(ios)?
                .info_plist
                .insert(ios_key.into(), PropertyListValue::Boolean(enabled));
        }
        ProjectIr::Android(android) => {
            let selector = |name: &str, key: Option<&str>| AndroidManifestSelector {
                name: name.into(),
                attributes: key
                    .map(|key| BTreeMap::from([("android:name".into(), key.into())]))
                    .unwrap_or_default(),
            };
            android_main_source_set(android)?.edit_manifest(&[AndroidManifestEdit::Upsert {
                path: vec![
                    selector("application", None),
                    selector("meta-data", Some(android_key)),
                ],
                attributes: BTreeMap::from([(
                    "android:value".into(),
                    AndroidManifestAttributeEdit::Override(enabled.to_string()),
                )]),
                append: vec![],
            }])?;
        }
        _ => {}
    }
    Ok(())
}

pub fn ios_app_target(ios: &mut IosProjectIr) -> Result<&mut AppleTarget> {
    ios.apple
        .targets
        .get_mut(&ios.apple.application)
        .context("Firebase: main iOS application is missing")
}

pub fn android_app(android: &mut AndroidProjectIr) -> Result<&mut AndroidModule> {
    android
        .modules
        .get_mut(&android.application)
        .context("Firebase: main Android application is missing")
}

fn android_main_source_set(android: &mut AndroidProjectIr) -> Result<&mut AndroidSourceSet> {
    let AndroidModuleKind::Application(app) = &mut android_app(android)?.kind else {
        anyhow::bail!("Firebase requires an Android application module");
    };
    Ok(app.android.source_sets.entry("main".into()).or_default())
}

/// Where `whisker-firebase` places `GoogleService-Info.plist` in the iOS project. Service
/// plugins that run after it find the user's file through this entry.
pub const IOS_CONFIG: &str = "firebase/GoogleService-Info.plist";

/// Read an app-relative input file, rejecting a symlink at any path component.
pub fn read_app_file(context: &ProjectContext, source: &ProjectPath) -> Result<Vec<u8>> {
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
