//! Shared CNG plugin edits for service crates.
use anyhow::{Context, Result};
use std::collections::BTreeMap;
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
