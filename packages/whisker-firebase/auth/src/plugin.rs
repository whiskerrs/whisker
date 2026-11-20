use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use whisker_plugin::{PluginConfig, project::*};

/// Phone sign-in setup for the main iOS app. Android needs no project changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WhiskerFirebaseAuthConfig {
    /// App-relative `GoogleService-Info.plist`, the same file as `whisker-firebase`'s `ios_config`.
    pub ios_config: String,
    /// Register the app's encoded Firebase app ID as a URL scheme. Without APNs, phone
    /// sign-in returns from its reCAPTCHA through it, and the SDK stops the app if it is missing.
    pub phone_url_scheme: bool,
}

impl Default for WhiskerFirebaseAuthConfig {
    fn default() -> Self {
        Self {
            ios_config: "GoogleService-Info.plist".into(),
            phone_url_scheme: true,
        }
    }
}

impl PluginConfig for WhiskerFirebaseAuthConfig {
    const NAME: &'static str = "whisker-firebase-auth";
}

pub struct WhiskerFirebaseAuth;

impl ProjectPlugin for WhiskerFirebaseAuth {
    type Config = WhiskerFirebaseAuthConfig;

    fn contribute(&self, context: &ProjectContext, config: &Self::Config) -> Result<ProjectUpdate> {
        let ProjectIr::Ios(ios) = &context.project else {
            return Ok(ProjectUpdate::Keep);
        };
        if !config.phone_url_scheme {
            return Ok(ProjectUpdate::Keep);
        }
        let scheme =
            PropertyListValue::String(encoded_app_id(&read(context, &config.ios_config)?)?);
        let mut ios = ios.clone();
        let target = ios
            .apple
            .targets
            .get_mut(&ios.apple.application)
            .context("whisker-firebase-auth: main iOS application is missing")?;
        let PropertyListValue::Array(url_types) = target
            .info_plist
            .entry("CFBundleURLTypes".into())
            .or_insert_with(|| PropertyListValue::Array(Vec::new()))
        else {
            anyhow::bail!("CFBundleURLTypes must be an array");
        };
        let registered = url_types.iter().any(|url_type| {
            matches!(url_type, PropertyListValue::Dict(fields)
                if matches!(fields.get("CFBundleURLSchemes"), Some(PropertyListValue::Array(schemes)) if schemes.contains(&scheme)))
        });
        if !registered {
            url_types.push(PropertyListValue::Dict(
                [(
                    "CFBundleURLSchemes".into(),
                    PropertyListValue::Array(vec![scheme]),
                )]
                .into(),
            ));
        }
        Ok(ProjectUpdate::Replace {
            project: Box::new(ProjectIr::Ios(ios)),
            reason: "Register the URL scheme for Firebase phone sign-in".into(),
        })
    }
}

/// `1:123:ios:abc` becomes `app-1-123-ios-abc`, the scheme the Firebase console lists.
fn encoded_app_id(plist: &[u8]) -> Result<String> {
    let value = plist::Value::from_reader(std::io::Cursor::new(plist))
        .context("whisker-firebase-auth: invalid GoogleService-Info.plist")?;
    let id = value
        .as_dictionary()
        .and_then(|values| values.get("GOOGLE_APP_ID"))
        .and_then(plist::Value::as_string)
        .filter(|id| !id.is_empty())
        .context("GoogleService-Info.plist is missing `GOOGLE_APP_ID`")?;
    ensure!(
        id.chars().all(|c| c.is_ascii_alphanumeric() || c == ':'),
        "GoogleService-Info.plist has a malformed GOOGLE_APP_ID"
    );
    Ok(format!("app-{}", id.replace(':', "-")))
}

fn read(context: &ProjectContext, source: &str) -> Result<Vec<u8>> {
    let source = ProjectPath::new(source)?;
    let path = context
        .app_crate_dir
        .as_deref()
        .context("Firebase configuration needs an app crate directory")?
        .join(source.as_str());
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
    std::fs::read(&path).with_context(|| format!("read Firebase configuration {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist(app_id: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>GOOGLE_APP_ID</key><string>{app_id}</string></dict></plist>"#
        )
        .into_bytes()
    }

    #[test]
    fn app_ids_encode_as_the_console_url_scheme() {
        assert_eq!(
            encoded_app_id(&plist("1:1234567890:ios:0123abcd")).unwrap(),
            "app-1-1234567890-ios-0123abcd"
        );
        assert!(encoded_app_id(&plist("")).is_err());
        assert!(encoded_app_id(&plist("1:2:ios:a/b")).is_err());
        assert!(encoded_app_id(b"not a plist").is_err());
    }
}
