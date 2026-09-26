//! Firebase Cloud Messaging for Whisker on Android and iOS (default app).
//!
//! ```ignore
//! use whisker_firebase::messaging::{Messaging, PermissionOptions};
//!
//! let messaging = Messaging::instance()?;
//! messaging.request_permission(PermissionOptions::default()).await?;
//! let token = messaging.token().await?; // send to your server
//! let _on_message = messaging.on_message(|message| {
//!     println!("{:?}: {:?}", message.notification, message.data);
//! })?;
//! if let Some(message) = messaging.initial_message().await? {
//!     // The app was launched by tapping this notification.
//! }
//! ```
//!
//! Adding this crate installs a CNG plugin that enables push notifications for the app:
//! the `aps-environment` entitlement and the `remote-notification` background mode on
//! iOS, and the messaging service and `POST_NOTIFICATIONS` permission on Android.
mod plugin;

pub use plugin::{WhiskerFirebaseMessaging, WhiskerFirebaseMessagingConfig};
pub use whisker_firebase_core::{FirebaseError, ListenerRegistration, Result, Timestamp};

use std::collections::BTreeMap;
use whisker::platform_module::WhiskerValue as Wire;
use whisker_firebase_core::__private::{listen_event, unwrap_response};
use whisker_firebase_core::FirebaseApp;

const SERVICE: &str = "messaging";

fn module() -> whisker::PlatformModule {
    whisker::module!("FirebaseMessaging")
}

fn invoke(method: &str, args: Vec<Wire>) -> Result<Wire> {
    unwrap_response(SERVICE, module().invoke(method, args))
}

async fn invoke_async(method: &str, args: Vec<Wire>) -> Result<Wire> {
    unwrap_response(SERVICE, module().invoke_async(method, args).await)
}

fn unit(value: Wire) -> Result<()> {
    match value {
        Wire::Null => Ok(()),
        _ => Err(response("expected an empty result")),
    }
}

fn response(message: &str) -> FirebaseError {
    FirebaseError::invalid_response(SERVICE, message)
}

/// Firebase Cloud Messaging for the default app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Messaging {
    _private: (),
}

impl Messaging {
    /// Initialize the default Firebase app if needed and return its Messaging instance.
    pub fn instance() -> Result<Self> {
        FirebaseApp::initialize()?;
        Ok(Self { _private: () })
    }

    /// Ask the user to allow notifications, then register for remote notifications.
    ///
    /// Shows the system prompt the first time on iOS and on Android 13+; later calls
    /// return the current settings. Android 12 and earlier need no prompt.
    pub async fn request_permission(
        &self,
        options: PermissionOptions,
    ) -> Result<NotificationSettings> {
        let options = Wire::Map(
            [
                ("alert", options.alert),
                ("badge", options.badge),
                ("sound", options.sound),
                ("provisional", options.provisional),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), Wire::Bool(value)))
            .collect(),
        );
        NotificationSettings::decode(invoke_async("requestPermission", vec![options]).await?)
    }

    /// The current notification permission, without prompting.
    pub async fn notification_settings(&self) -> Result<NotificationSettings> {
        NotificationSettings::decode(invoke_async("getNotificationSettings", vec![]).await?)
    }

    /// The FCM registration token that your server uses to address this app instance.
    ///
    /// On iOS the token is only usable once APNs registration succeeds, which requires
    /// the `aps-environment` entitlement and [`request_permission`](Self::request_permission).
    pub async fn token(&self) -> Result<String> {
        match invoke_async("getToken", vec![]).await? {
            Wire::String(token) => Ok(token),
            _ => Err(response("expected a token")),
        }
    }

    /// Invalidate the current token. A new one is created on the next [`token`](Self::token)
    /// call, or at the next launch while auto-init is enabled.
    pub async fn delete_token(&self) -> Result<()> {
        unit(invoke_async("deleteToken", vec![]).await?)
    }

    /// The APNs device token as lowercase hex, once iOS has provided it. Always `None` on Android.
    pub fn apns_token(&self) -> Result<Option<String>> {
        match invoke("getApnsToken", vec![])? {
            Wire::String(token) => Ok(Some(token)),
            Wire::Null => Ok(None),
            _ => Err(response("expected an APNs token")),
        }
    }

    /// Receive messages sent to `/topics/{topic}`.
    pub async fn subscribe_to_topic(&self, topic: &str) -> Result<()> {
        validate_topic(topic)?;
        unit(invoke_async("subscribeToTopic", vec![Wire::String(topic.into())]).await?)
    }

    pub async fn unsubscribe_from_topic(&self, topic: &str) -> Result<()> {
        validate_topic(topic)?;
        unit(invoke_async("unsubscribeFromTopic", vec![Wire::String(topic.into())]).await?)
    }

    /// Whether a token is generated automatically at launch.
    pub fn is_auto_init_enabled(&self) -> Result<bool> {
        match invoke("isAutoInitEnabled", vec![])? {
            Wire::Bool(enabled) => Ok(enabled),
            _ => Err(response("expected a boolean")),
        }
    }

    /// Persisted across launches. Disable it to generate tokens only after user consent.
    pub fn set_auto_init_enabled(&self, enabled: bool) -> Result<()> {
        unit(invoke("setAutoInitEnabled", vec![Wire::Bool(enabled)])?)
    }

    /// How iOS presents notifications that arrive while the app is in the foreground.
    /// Android shows no notification for foreground messages; handle them in [`on_message`](Self::on_message).
    pub fn set_foreground_presentation(&self, options: ForegroundPresentation) -> Result<()> {
        let options = Wire::Map(
            [
                ("banner", options.banner),
                ("list", options.list),
                ("sound", options.sound),
                ("badge", options.badge),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), Wire::Bool(value)))
            .collect(),
        );
        unit(invoke("setForegroundPresentation", vec![options])?)
    }

    /// Called with each new token, e.g. after the previous one was invalidated.
    pub fn on_token_refresh(
        &self,
        callback: impl Fn(String) + 'static,
    ) -> Result<ListenerRegistration> {
        listen_event(&module(), SERVICE, "token", move |wire| {
            if let Ok(Wire::String(token)) = wire {
                callback(token);
            }
        })
    }

    /// Called for each message received while the app is in the foreground.
    ///
    /// On Android this includes data-only messages. On iOS it includes notification
    /// messages; data-only (silent) messages are not delivered to Rust yet.
    pub fn on_message(
        &self,
        callback: impl Fn(RemoteMessage) + 'static,
    ) -> Result<ListenerRegistration> {
        self.listen("message", callback)
    }

    /// Called when the user taps a notification while the app keeps running.
    /// Taps that (re)create the app are returned by [`initial_message`](Self::initial_message)
    /// instead; on Android this includes taps while backgrounded, which recreate the activity.
    pub fn on_message_opened_app(
        &self,
        callback: impl Fn(RemoteMessage) + 'static,
    ) -> Result<ListenerRegistration> {
        self.listen("messageOpened", callback)
    }

    fn listen(
        &self,
        event: &str,
        callback: impl Fn(RemoteMessage) + 'static,
    ) -> Result<ListenerRegistration> {
        listen_event(&module(), SERVICE, event, move |wire| {
            if let Ok(message) = wire.and_then(RemoteMessage::decode) {
                callback(message);
            }
        })
    }

    /// The notification whose tap launched the app, returned once.
    pub async fn initial_message(&self) -> Result<Option<RemoteMessage>> {
        match invoke_async("getInitialMessage", vec![]).await? {
            Wire::Null => Ok(None),
            wire => RemoteMessage::decode(wire).map(Some),
        }
    }
}

fn validate_topic(topic: &str) -> Result<()> {
    let topic = topic.strip_prefix("/topics/").unwrap_or(topic);
    let valid = !topic.is_empty()
        && topic.len() <= 900
        && topic
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.~%".contains(c));
    if valid {
        Ok(())
    } else {
        Err(FirebaseError::invalid_argument(
            SERVICE,
            "topic names use letters, digits, and -_.~% (at most 900 characters)",
        ))
    }
}

/// What to ask for in [`Messaging::request_permission`] (iOS; Android asks for all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PermissionOptions {
    pub alert: bool,
    pub badge: bool,
    pub sound: bool,
    /// Deliver quietly to Notification Center without prompting (iOS provisional authorization).
    pub provisional: bool,
}

impl Default for PermissionOptions {
    fn default() -> Self {
        Self {
            alert: true,
            badge: true,
            sound: true,
            provisional: false,
        }
    }
}

impl PermissionOptions {
    pub fn provisional(mut self, provisional: bool) -> Self {
        self.provisional = provisional;
        self
    }
}

/// iOS presentation of foreground notifications. All enabled by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ForegroundPresentation {
    pub banner: bool,
    pub list: bool,
    pub sound: bool,
    pub badge: bool,
}

impl Default for ForegroundPresentation {
    fn default() -> Self {
        Self::all()
    }
}

impl ForegroundPresentation {
    pub fn all() -> Self {
        Self {
            banner: true,
            list: true,
            sound: true,
            badge: true,
        }
    }

    /// Deliver foreground messages only to [`Messaging::on_message`].
    pub fn none() -> Self {
        Self {
            banner: false,
            list: false,
            sound: false,
            badge: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthorizationStatus {
    /// The user has not been asked yet (iOS).
    NotDetermined,
    Denied,
    Authorized,
    /// Notifications are delivered quietly (iOS).
    Provisional,
    /// Temporarily authorized for an App Clip (iOS).
    Ephemeral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct NotificationSettings {
    pub authorization_status: AuthorizationStatus,
}

impl NotificationSettings {
    /// Whether notifications can be shown, including quietly.
    pub fn is_allowed(&self) -> bool {
        matches!(
            self.authorization_status,
            AuthorizationStatus::Authorized
                | AuthorizationStatus::Provisional
                | AuthorizationStatus::Ephemeral
        )
    }

    fn decode(wire: Wire) -> Result<Self> {
        let fields = map(wire)?;
        let authorization_status = match fields.get("authorization_status") {
            Some(Wire::String(status)) => match status.as_str() {
                "not_determined" => AuthorizationStatus::NotDetermined,
                "denied" => AuthorizationStatus::Denied,
                "authorized" => AuthorizationStatus::Authorized,
                "provisional" => AuthorizationStatus::Provisional,
                "ephemeral" => AuthorizationStatus::Ephemeral,
                _ => return Err(response("unknown authorization status")),
            },
            _ => return Err(response("missing authorization status")),
        };
        Ok(Self {
            authorization_status,
        })
    }
}

/// The notification part of a message, shown by the system when the app is in the background.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Notification {
    pub title: Option<String>,
    pub body: Option<String>,
    pub image_url: Option<String>,
}

/// A message received from FCM.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RemoteMessage {
    pub message_id: Option<String>,
    /// The sender ID or `/topics/{topic}`.
    pub from: Option<String>,
    pub collapse_key: Option<String>,
    pub sent_time: Option<Timestamp>,
    /// Custom key/value pairs from the message's `data` field.
    pub data: BTreeMap<String, String>,
    pub notification: Option<Notification>,
}

impl RemoteMessage {
    fn decode(wire: Wire) -> Result<Self> {
        let fields = map(wire)?;
        let data = match fields.get("data") {
            Some(Wire::Map(data)) => data
                .iter()
                .filter_map(|(key, value)| match value {
                    Wire::String(value) => Some((key.clone(), value.clone())),
                    _ => None,
                })
                .collect(),
            _ => BTreeMap::new(),
        };
        let notification = match fields.get("notification") {
            Some(Wire::Map(notification)) => Some(Notification {
                title: text(notification, "title"),
                body: text(notification, "body"),
                image_url: text(notification, "image_url"),
            }),
            _ => None,
        };
        Ok(Self {
            message_id: text(&fields, "message_id"),
            from: text(&fields, "from"),
            collapse_key: text(&fields, "collapse_key"),
            sent_time: match fields.get("sent_time") {
                Some(Wire::Int(millis)) if *millis > 0 => Timestamp::from_millis(*millis).ok(),
                _ => None,
            },
            data,
            notification,
        })
    }
}

fn map(wire: Wire) -> Result<BTreeMap<String, Wire>> {
    match wire {
        Wire::Map(fields) => Ok(fields),
        _ => Err(response("expected a map")),
    }
}

fn text(fields: &BTreeMap<String, Wire>, key: &str) -> Option<String> {
    match fields.get(key) {
        Some(Wire::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_are_validated_before_native_dispatch() {
        for topic in ["news", "/topics/news", "a-b_c.d~e%f"] {
            assert!(validate_topic(topic).is_ok(), "{topic}");
        }
        for topic in ["", "has space", "/topics/", "日本語", &"x".repeat(901)] {
            assert!(validate_topic(topic).is_err(), "{topic}");
        }
    }

    #[test]
    fn messages_decode_with_optional_parts() {
        let wire = Wire::Map(
            [
                ("message_id".into(), Wire::String("m1".into())),
                ("sent_time".into(), Wire::Int(1_700_000_000_000)),
                (
                    "data".into(),
                    Wire::Map([("kind".into(), Wire::String("chat".into()))].into()),
                ),
                (
                    "notification".into(),
                    Wire::Map([("title".into(), Wire::String("Hi".into()))].into()),
                ),
            ]
            .into(),
        );
        let message = RemoteMessage::decode(wire).unwrap();
        assert_eq!(message.message_id.as_deref(), Some("m1"));
        assert_eq!(message.data["kind"], "chat");
        assert_eq!(message.notification.unwrap().title.as_deref(), Some("Hi"));
        assert_eq!(message.sent_time.unwrap().seconds(), 1_700_000_000);
        assert!(message.from.is_none());
        let settings = NotificationSettings::decode(Wire::Map(
            [(
                "authorization_status".into(),
                Wire::String("provisional".into()),
            )]
            .into(),
        ))
        .unwrap();
        assert!(settings.is_allowed());
    }
}
