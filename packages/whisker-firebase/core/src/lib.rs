//! Firebase application initialization and shared types for Whisker on Android and iOS.
//!
//! Service crates (`whisker-firebase-firestore`, `-auth`, `-storage`) build on the
//! error type, [`Timestamp`], and listener plumbing defined here.
mod error;
mod listener;
mod project;
mod timestamp;

use std::collections::BTreeMap;
use whisker::platform_module::WhiskerValue;

pub use error::{FirebaseError, Result};
pub use listener::ListenerRegistration;
pub use timestamp::Timestamp;

#[doc(hidden)]
pub mod __private {
    pub use crate::listener::{Listeners, listen, listen_event, signal_from_listener};
    pub use crate::project::{android_app, ios_app_target, set_collection_default};
    pub use crate::timestamp::TIMESTAMP_NAME;
    pub use crate::{string_field, unwrap_response};
}

/// The configured default Firebase application.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FirebaseApp {
    pub name: String,
    pub app_id: String,
    pub project_id: String,
    /// The default Cloud Storage bucket, when the configuration file declares one.
    pub storage_bucket: Option<String>,
}
impl FirebaseApp {
    /// Initialize (or retrieve) the default app from the bundled configuration file.
    /// Repeated calls reuse the native app. Service entry points call this for you.
    pub fn initialize() -> Result<Self> {
        if !cfg!(any(target_os = "android", target_os = "ios")) {
            return Err(FirebaseError::new(
                "app",
                "unsupported-platform",
                "Firebase currently supports Android and iOS",
            ));
        }
        let value = unwrap_response(
            "app",
            whisker::module!("FirebaseCore").invoke("initialize", vec![]),
        )?;
        let WhiskerValue::Map(fields) = value else {
            return Err(FirebaseError::invalid_response(
                "app",
                "expected Firebase app information",
            ));
        };
        Ok(Self {
            name: string_field("app", &fields, "name")?,
            app_id: string_field("app", &fields, "app_id")?,
            project_id: string_field("app", &fields, "project_id")?,
            storage_bucket: match fields.get("storage_bucket") {
                Some(WhiskerValue::String(bucket)) if !bucket.is_empty() => Some(bucket.clone()),
                _ => None,
            },
        })
    }
}

#[doc(hidden)]
pub fn string_field(
    service: &'static str,
    fields: &BTreeMap<String, WhiskerValue>,
    key: &str,
) -> Result<String> {
    match fields.get(key) {
        Some(WhiskerValue::String(value)) => Ok(value.clone()),
        _ => Err(FirebaseError::invalid_response(
            service,
            format!("missing {key}"),
        )),
    }
}

/// Decode the shared native result envelope `{value}` / `{error: {code, message}}`.
#[doc(hidden)]
pub fn unwrap_response(service: &'static str, value: WhiskerValue) -> Result<WhiskerValue> {
    match value {
        WhiskerValue::Error(message) => Err(FirebaseError::new(service, "bridge-error", message)),
        WhiskerValue::Map(mut fields) => {
            if let Some(WhiskerValue::Map(error)) = fields.remove("error") {
                return Err(FirebaseError::new(
                    service,
                    string_field(service, &error, "code")?,
                    string_field(service, &error, "message")?,
                ));
            }
            fields
                .remove("value")
                .ok_or_else(|| FirebaseError::invalid_response(service, "missing result value"))
        }
        _ => Err(FirebaseError::invalid_response(
            service,
            "expected result envelope",
        )),
    }
}
