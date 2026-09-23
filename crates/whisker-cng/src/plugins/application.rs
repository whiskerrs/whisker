//! Application configuration policy shared by the active generation adapters.
//!
//! This is a mandatory initialization/resolution pass, not an opt-in Plugin.
//! It preserves user intent as optional fields before mobile plugins run, then
//! resolves remaining defaults from their final IR. Required native structure
//! (targets, SDK integration, entry sources) belongs to each platform. Android's
//! application ProjectPlugin consumes these resolved values and declares its
//! native structure through the regular composition contract.

use crate::compose::EnabledTargets;
use anyhow::{Result, anyhow};
use whisker_config::Config;
use whisker_plugin::{
    AndroidManifest, AndroidProjectIr, AppMeta, GenerateContext, IosProjectIr, MutationJournal,
    PlistValue,
};

pub(crate) fn initial_context(app_config: &Config, enabled: EnabledTargets) -> GenerateContext {
    let app_meta = AppMeta {
        name: app_config.name.clone().unwrap_or_default(),
        version: app_config.version.clone().unwrap_or_default(),
        build_number: app_config.build_number.unwrap_or(1),
        ios_bundle_id: if enabled.ios {
            app_config
                .ios
                .bundle_id
                .clone()
                .or_else(|| app_config.bundle_id.clone())
        } else {
            None
        },
        android_application_id: if enabled.android {
            app_config
                .android
                .application_id
                .clone()
                .or_else(|| app_config.bundle_id.clone())
        } else {
            None
        },
    };

    // Keep optional app intent visible before plugins run. Resolve missing
    // scalar values only from the final IR, so plugin overrides are not undone
    // by reading the original Config again.
    let ios = enabled.ios.then(|| IosProjectIr {
        app_name: app_config.name.clone(),
        version: app_config.version.clone(),
        build_number: app_config.build_number,
        bundle_id: app_meta.ios_bundle_id.clone(),
        scheme: app_config.ios.scheme.clone(),
        deployment_target: app_config.ios.deployment_target.clone(),
        info_plist: seed_orientation_plist(&app_config.ios.orientations),
        ..Default::default()
    });
    let android = enabled.android.then(|| AndroidProjectIr {
        app_name: app_config.name.clone(),
        version: app_config.version.clone(),
        build_number: app_config.build_number,
        application_id: app_meta.android_application_id.clone(),
        min_sdk: app_config.android.min_sdk,
        target_sdk: app_config.android.target_sdk,
        manifest: AndroidManifest {
            main_activity_url_schemes: app_config.url_schemes.clone(),
            ..Default::default()
        },
        ..Default::default()
    });

    GenerateContext {
        app_meta,
        ios,
        android,
        journal: MutationJournal::default(),
        // Stamped by the composition engine, which is where the app crate
        // dir is known.
        app_crate_dir: None,
    }
}

/// Apply the active Host's orientation policy to phone and iPad metadata.
/// Restricted orientations also select the Host's full-screen configuration.
/// These values are seeded before plugins, which can explicitly replace them.
fn seed_orientation_plist(
    orientations: &[whisker_config::Orientation],
) -> std::collections::BTreeMap<String, PlistValue> {
    let restricted = !orientations.is_empty();
    let list = if restricted {
        orientations.to_vec()
    } else {
        whisker_config::Orientation::all()
    };
    let value = PlistValue::Array(
        list.iter()
            .map(|o| PlistValue::String(o.plist_value().to_string()))
            .collect(),
    );

    let mut seeded = std::collections::BTreeMap::new();
    seeded.insert(
        "UISupportedInterfaceOrientations".to_string(),
        value.clone(),
    );
    seeded.insert("UISupportedInterfaceOrientations~ipad".to_string(), value);
    if restricted {
        seeded.insert(
            "UIRequiresFullScreen".to_string(),
            PlistValue::Boolean(true),
        );
    }
    seeded
}

/// Application values resolved after mobile plugins have had their say.
/// Structural project contents are deliberately absent from these records.
pub(crate) struct IosApplication {
    pub app_name: String,
    pub version: String,
    pub build_number: u32,
    pub scheme: String,
    pub bundle_id: String,
    pub deployment_target: String,
}

pub(crate) fn ios(ir: &IosProjectIr) -> Result<IosApplication> {
    let app_name = ir
        .app_name
        .clone()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required"))?;
    Ok(IosApplication {
        scheme: ir.scheme.clone().unwrap_or_else(|| app_name.clone()),
        app_name,
        version: version(ir.version.as_deref()),
        build_number: ir.build_number.unwrap_or(1),
        bundle_id: ir.bundle_id.clone().ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.bundle_id(\"…\")) (or app.bundle_id) is required for iOS"
            )
        })?,
        deployment_target: ir
            .deployment_target
            .clone()
            .unwrap_or_else(|| "13.0".into()),
    })
}

pub(crate) struct AndroidApplication {
    pub app_name: String,
    pub version: String,
    pub build_number: u32,
    pub application_id: String,
    pub min_sdk: u32,
    pub target_sdk: u32,
}

pub(crate) fn android(ir: &AndroidProjectIr) -> Result<AndroidApplication> {
    Ok(AndroidApplication {
        app_name: ir.app_name.clone()
            .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required"))?,
        version: version(ir.version.as_deref()),
        build_number: ir.build_number.unwrap_or(1),
        application_id: ir.application_id.clone().ok_or_else(|| anyhow!(
            "whisker.rs: app.android(|a| a.application_id(\"…\")) (or app.bundle_id) is required for Android"
        ))?,
        min_sdk: ir.min_sdk.unwrap_or(24),
        target_sdk: ir.target_sdk.unwrap_or(34),
    })
}

pub(crate) struct MacosApplication {
    pub app_name: String,
    pub bundle_id: String,
    pub version: String,
    pub build_number: u32,
    pub minimum_system_version: String,
}

pub(crate) fn macos(config: &Config) -> Result<MacosApplication> {
    Ok(MacosApplication {
        app_name: config
            .name
            .clone()
            .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required for macOS"))?,
        bundle_id: config
            .bundle_id
            .clone()
            .ok_or_else(|| anyhow!("whisker.rs: app.bundle_id(\"…\") is required for macOS"))?,
        version: version(config.version.as_deref()),
        build_number: config.build_number.unwrap_or(1),
        minimum_system_version: "12.0".into(),
    })
}

pub(crate) struct WebApplication {
    pub app_name: String,
    pub base_path: String,
}

pub(crate) fn web(config: &Config) -> Result<WebApplication> {
    Ok(WebApplication {
        app_name: config
            .name
            .clone()
            .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required for Web"))?,
        base_path: normalize_web_base_path(config.web.base_path.as_deref().unwrap_or("/"))?,
    })
}

fn version(value: Option<&str>) -> String {
    value.unwrap_or("0.1.0").into()
}

/// The active Web adapter accepts a directory prefix with an optional final slash.
/// Its resolved value matches the declarative Web IR's trailing-slash contract.
pub(crate) fn normalize_web_base_path(path: &str) -> Result<String> {
    if !path.starts_with('/')
        || path.contains("//")
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-._~".contains(&b))
        || path.split('/').any(|part| matches!(part, "." | ".."))
    {
        anyhow::bail!(
            "Web base path must be an absolute URL path without query, fragment, or dot segments: {path}"
        );
    }
    Ok(format!("{}/", path.trim_end_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_orientation_policy_covers_phone_and_ipad() {
        let seeded = seed_orientation_plist(&[]);
        let PlistValue::Array(phone) = &seeded["UISupportedInterfaceOrientations"] else {
            panic!("expected an array");
        };
        assert_eq!(phone.len(), 4);
        assert_eq!(
            seeded["UISupportedInterfaceOrientations"],
            seeded["UISupportedInterfaceOrientations~ipad"]
        );
        assert!(!seeded.contains_key("UIRequiresFullScreen"));
    }

    #[test]
    fn restricted_orientation_policy_sets_fullscreen() {
        let seeded = seed_orientation_plist(&[whisker_config::Orientation::Portrait]);
        assert_eq!(
            seeded["UISupportedInterfaceOrientations"],
            PlistValue::Array(vec![PlistValue::String(
                "UIInterfaceOrientationPortrait".to_string()
            )])
        );
        assert_eq!(
            seeded["UIRequiresFullScreen"],
            PlistValue::Boolean(true),
            "the active Host policy pairs restricted orientations with fullscreen"
        );
    }
}
