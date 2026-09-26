//! Google Analytics for Firebase for Whisker on Android and iOS (default app).
//!
//! ```ignore
//! use whisker_firebase::analytics::{Analytics, params};
//!
//! let analytics = Analytics::instance()?;
//! analytics.log_event("select_content", params! {
//!     "content_type" => "image",
//!     "item_id" => 42,
//! })?;
//! analytics.log_screen_view("Settings", None)?;
//! analytics.set_user_property("favorite_food", Some("pizza"))?;
//! ```
//!
//! Event, parameter, and user-property names follow Firebase's limits, which are
//! checked before logging because the SDKs drop invalid data silently.
mod plugin;

pub use plugin::{WhiskerFirebaseAnalytics, WhiskerFirebaseAnalyticsConfig};
pub use whisker_firebase_core::{FirebaseError, Result};

use std::collections::BTreeMap;
use std::time::Duration;
use whisker::platform_module::WhiskerValue as Wire;
use whisker_firebase_core::__private::unwrap_response;
use whisker_firebase_core::FirebaseApp;

const SERVICE: &str = "analytics";

fn module() -> whisker::PlatformModule {
    whisker::module!("FirebaseAnalytics")
}

fn invoke(method: &str, args: Vec<Wire>) -> Result<()> {
    match unwrap_response(SERVICE, module().invoke(method, args))? {
        Wire::Null => Ok(()),
        _ => Err(FirebaseError::invalid_response(
            SERVICE,
            "expected an empty result",
        )),
    }
}

fn invalid(message: impl Into<String>) -> FirebaseError {
    FirebaseError::invalid_argument(SERVICE, message)
}

fn optional(value: Option<&str>) -> Wire {
    value.map_or(Wire::Null, |value| Wire::String(value.into()))
}

/// Google Analytics for the default app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Analytics {
    _private: (),
}

impl Analytics {
    /// Initialize the default Firebase app if needed and return its Analytics instance.
    pub fn instance() -> Result<Self> {
        FirebaseApp::initialize()?;
        Ok(Self { _private: () })
    }

    /// Log an event with up to 25 parameters. Prefer the names Google recommends
    /// (`login`, `purchase`, `select_content`, …) so reports recognise them.
    pub fn log_event<K: Into<String>, V: Into<Param>>(
        &self,
        name: &str,
        params: impl IntoIterator<Item = (K, V)>,
    ) -> Result<()> {
        validate_name(name, 40, "event")?;
        invoke(
            "logEvent",
            vec![Wire::String(name.into()), encode_params(params)?],
        )
    }

    /// Log `screen_view`. Whisker has no native screens for automatic tracking, so call
    /// this when your router changes routes.
    pub fn log_screen_view(&self, screen_name: &str, screen_class: Option<&str>) -> Result<()> {
        let mut params = vec![("screen_name", Param::from(screen_name))];
        if let Some(class) = screen_class {
            params.push(("screen_class", class.into()));
        }
        self.log_event("screen_view", params)
    }

    /// `None` clears the user ID.
    pub fn set_user_id(&self, id: Option<&str>) -> Result<()> {
        if id.is_some_and(|id| id.len() > 256) {
            return Err(invalid("user IDs are limited to 256 characters"));
        }
        invoke("setUserId", vec![optional(id)])
    }

    /// Set a user property (name up to 24 characters, value up to 36); `None` clears it.
    pub fn set_user_property(&self, name: &str, value: Option<&str>) -> Result<()> {
        validate_name(name, 24, "user property")?;
        if value.is_some_and(|value| value.chars().count() > 36) {
            return Err(invalid("user property values are limited to 36 characters"));
        }
        invoke(
            "setUserProperty",
            vec![Wire::String(name.into()), optional(value)],
        )
    }

    /// Persisted across launches. Pair with the plugin's `collection_enabled = Some(false)`
    /// to collect nothing until the user consents.
    pub fn set_collection_enabled(&self, enabled: bool) -> Result<()> {
        invoke("setAnalyticsCollectionEnabled", vec![Wire::Bool(enabled)])
    }

    /// Parameters added to every later event; an empty list clears them.
    pub fn set_default_event_parameters<K: Into<String>, V: Into<Param>>(
        &self,
        params: impl IntoIterator<Item = (K, V)>,
    ) -> Result<()> {
        invoke("setDefaultEventParameters", vec![encode_params(params)?])
    }

    /// Clear all analytics data on this device and reset the app instance ID.
    pub fn reset_analytics_data(&self) -> Result<()> {
        invoke("resetAnalyticsData", vec![])
    }

    /// The app instance ID, or `None` while analytics collection is disabled.
    pub async fn app_instance_id(&self) -> Result<Option<String>> {
        match unwrap_response(
            SERVICE,
            module().invoke_async("getAppInstanceId", vec![]).await,
        )? {
            Wire::String(id) => Ok(Some(id)),
            Wire::Null => Ok(None),
            _ => Err(FirebaseError::invalid_response(SERVICE, "expected an ID")),
        }
    }

    /// Update Consent Mode; unset fields keep their current state.
    pub fn set_consent(&self, consent: Consent) -> Result<()> {
        let fields = [
            ("analytics_storage", consent.analytics_storage),
            ("ad_storage", consent.ad_storage),
            ("ad_user_data", consent.ad_user_data),
            ("ad_personalization", consent.ad_personalization),
        ]
        .into_iter()
        .filter_map(|(key, granted)| Some((key.to_owned(), Wire::Bool(granted?))))
        .collect();
        invoke("setConsent", vec![Wire::Map(fields)])
    }

    /// How long the app may be inactive before a new session starts (default 30 minutes).
    pub fn set_session_timeout(&self, timeout: Duration) -> Result<()> {
        let millis = i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX);
        invoke("setSessionTimeout", vec![Wire::Int(millis)])
    }
}

/// Consent Mode signals: `Some(true)` grants, `Some(false)` denies, `None` leaves unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Consent {
    pub analytics_storage: Option<bool>,
    pub ad_storage: Option<bool>,
    pub ad_user_data: Option<bool>,
    pub ad_personalization: Option<bool>,
}

impl Consent {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn analytics_storage(mut self, granted: bool) -> Self {
        self.analytics_storage = Some(granted);
        self
    }

    pub fn ad_storage(mut self, granted: bool) -> Self {
        self.ad_storage = Some(granted);
        self
    }

    pub fn ad_user_data(mut self, granted: bool) -> Self {
        self.ad_user_data = Some(granted);
        self
    }

    pub fn ad_personalization(mut self, granted: bool) -> Self {
        self.ad_personalization = Some(granted);
        self
    }
}

/// An event parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    /// Up to 100 characters.
    String(String),
    Integer(i64),
    Double(f64),
    /// Ecommerce `items`: each item is its own list of parameters.
    Items(Vec<Vec<(String, Param)>>),
}

macro_rules! param_from {
    ($variant:ident: $($ty:ty),*) => {$(
        impl From<$ty> for Param {
            fn from(value: $ty) -> Self {
                Param::$variant(value.into())
            }
        }
    )*};
}
param_from!(String: String, &str, &String);
param_from!(Integer: i8, i16, i32, i64, u8, u16, u32);
param_from!(Double: f32, f64);

impl From<bool> for Param {
    /// Analytics has no boolean type; booleans are logged as 0 or 1.
    fn from(value: bool) -> Self {
        Param::Integer(value.into())
    }
}

/// Build event parameters from heterogeneous values.
///
/// ```ignore
/// analytics.log_event("purchase", params! { "currency" => "JPY", "value" => 1200 })?;
/// ```
#[macro_export]
macro_rules! params {
    () => { ::std::vec::Vec::<(::std::string::String, $crate::Param)>::new() };
    ($($key:expr => $value:expr),+ $(,)?) => {
        ::std::vec![$((::std::string::String::from($key), $crate::Param::from($value))),+]
    };
}

fn validate_name(name: &str, max: usize, kind: &str) -> Result<()> {
    let valid = name.len() <= max
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !["firebase_", "google_", "ga_"]
            .iter()
            .any(|prefix| name.starts_with(prefix));
    if valid {
        Ok(())
    } else {
        Err(invalid(format!(
            "`{name}` is not a valid {kind} name: use up to {max} letters, digits, and \
             underscores, starting with a letter and without the firebase_, google_, or ga_ prefix"
        )))
    }
}

fn encode_params<K: Into<String>, V: Into<Param>>(
    params: impl IntoIterator<Item = (K, V)>,
) -> Result<Wire> {
    let params: Vec<(String, Param)> = params
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
    if params.len() > 25 {
        return Err(invalid("events are limited to 25 parameters"));
    }
    encode_list(params, true).map(Wire::Map)
}

fn encode_list(params: Vec<(String, Param)>, allow_items: bool) -> Result<BTreeMap<String, Wire>> {
    params
        .into_iter()
        .map(|(key, value)| {
            validate_name(&key, 40, "parameter")?;
            let wire = match value {
                Param::String(value) if value.chars().count() > 100 => {
                    return Err(invalid(format!(
                        "parameter `{key}` exceeds 100 characters"
                    )));
                }
                Param::String(value) => Wire::String(value),
                Param::Integer(value) => Wire::Int(value),
                Param::Double(value) => Wire::Float(value),
                Param::Items(items) if allow_items && key == "items" && items.len() <= 200 => {
                    Wire::Array(
                        items
                            .into_iter()
                            .map(|item| encode_list(item, false).map(Wire::Map))
                            .collect::<Result<_>>()?,
                    )
                }
                Param::Items(_) => {
                    return Err(invalid(
                        "item lists are only allowed as the top-level `items` parameter (up to 200 items)",
                    ));
                }
            };
            Ok((key, wire))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_values_follow_firebase_limits() {
        assert!(validate_name("select_content", 40, "event").is_ok());
        for name in [
            "",
            "1st",
            "has space",
            "firebase_x",
            "ga_x",
            "日本",
            &"a".repeat(41),
        ] {
            assert!(validate_name(name, 40, "event").is_err(), "{name}");
        }
        let params = params! { "currency" => "JPY", "value" => 12.5, "count" => 3, "flag" => true };
        let Wire::Map(wire) = encode_params(params).unwrap() else {
            panic!()
        };
        assert_eq!(wire["flag"], Wire::Int(1));
        assert_eq!(wire["value"], Wire::Float(12.5));
        assert!(encode_params((0..26).map(|i| (format!("p{i}"), 1))).is_err());
        assert!(encode_params(params! { "name" => "x".repeat(101) }).is_err());
    }

    #[test]
    fn items_are_only_allowed_at_the_top_level() {
        let item = params! { "item_id" => "sku-1", "price" => 100 };
        let ok = vec![("items".to_string(), Param::Items(vec![item.clone()]))];
        assert!(encode_params(ok).is_ok());
        let misplaced = vec![("products".to_string(), Param::Items(vec![item.clone()]))];
        assert!(encode_params(misplaced).is_err());
        let nested = vec![(
            "items".to_string(),
            Param::Items(vec![vec![("items".to_string(), Param::Items(vec![item]))]]),
        )];
        assert!(encode_params(nested).is_err());
    }
}
