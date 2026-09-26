//! Firebase Authentication for Whisker on Android and iOS (default app).
//!
//! ```ignore
//! use whisker_firebase::auth::{Auth, AuthCredential};
//!
//! let auth = Auth::instance()?;
//! let credential = auth.sign_in_with_email_and_password("ada@example.com", "secret").await?;
//! let token = credential.user.id_token(false).await?;
//!
//! // Inside a component: re-renders on sign-in/out.
//! let user = auth.user_signal();
//! ```
//!
//! Sign-in UIs for Google, Apple, and other providers are out of scope: obtain their
//! tokens with a native sign-in library and pass them to [`Auth::sign_in_with_credential`].
use std::collections::BTreeMap;
use std::rc::Rc;
use whisker::ReadSignal;
use whisker::platform_module::WhiskerValue as Wire;
use whisker_firebase_core::__private::{Listeners, listen, signal_from_listener, unwrap_response};
use whisker_firebase_core::FirebaseApp;
pub use whisker_firebase_core::{FirebaseError, ListenerRegistration, Result, Timestamp};

const SERVICE: &str = "auth";

fn module() -> whisker::PlatformModule {
    whisker::module!("FirebaseAuth")
}

whisker::runtime_local! {
    static LISTENERS: Rc<Listeners> = Listeners::new(module(), SERVICE, "authState", "removeAllListeners");
}

/// Codes follow the JavaScript SDK (`invalid-credential`, `email-already-in-use`, …);
/// the Android/iOS `ERROR_WRONG_PASSWORD` form is normalized to `wrong-password`.
fn normalize(mut error: FirebaseError) -> FirebaseError {
    if let Some(name) = error.code.strip_prefix("ERROR_") {
        error.code = name.to_ascii_lowercase().replace('_', "-");
    }
    error
}

fn invoke(method: &str, args: Vec<Wire>) -> Result<Wire> {
    unwrap_response(SERVICE, module().invoke(method, args)).map_err(normalize)
}

async fn invoke_async(method: &str, args: Vec<Wire>) -> Result<Wire> {
    unwrap_response(SERVICE, module().invoke_async(method, args).await).map_err(normalize)
}

fn string(value: &str) -> Wire {
    Wire::String(value.into())
}

fn unit(value: Wire) -> Result<()> {
    match value {
        Wire::Null => Ok(()),
        _ => Err(FirebaseError::invalid_response(
            SERVICE,
            "expected an empty result",
        )),
    }
}

/// Firebase Authentication for the default app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Auth {
    _private: (),
}

impl Auth {
    /// Initialize the default Firebase app if needed and return its Auth instance.
    pub fn instance() -> Result<Self> {
        FirebaseApp::initialize()?;
        Ok(Self { _private: () })
    }

    /// Route Auth to the Auth emulator. Call it before any other Auth operation.
    pub fn use_emulator(&self, host: &str, port: u16) -> Result<()> {
        unit(invoke(
            "useEmulator",
            vec![string(host), Wire::Int(port.into())],
        )?)
    }

    /// The signed-in user, if any. Prefer [`on_auth_state_changed`](Self::on_auth_state_changed)
    /// or [`user_signal`](Self::user_signal) at startup, before the persisted session is restored.
    pub fn current_user(&self) -> Result<Option<User>> {
        User::decode_optional(invoke("currentUser", vec![])?)
    }

    pub async fn sign_in_anonymously(&self) -> Result<UserCredential> {
        UserCredential::decode(invoke_async("signInAnonymously", vec![]).await?)
    }

    pub async fn sign_in_with_email_and_password(
        &self,
        email: &str,
        password: &str,
    ) -> Result<UserCredential> {
        UserCredential::decode(
            invoke_async(
                "signInWithEmailAndPassword",
                vec![string(email), string(password)],
            )
            .await?,
        )
    }

    pub async fn create_user_with_email_and_password(
        &self,
        email: &str,
        password: &str,
    ) -> Result<UserCredential> {
        UserCredential::decode(
            invoke_async(
                "createUserWithEmailAndPassword",
                vec![string(email), string(password)],
            )
            .await?,
        )
    }

    /// Sign in with a token minted by your server with the Admin SDK.
    pub async fn sign_in_with_custom_token(&self, token: &str) -> Result<UserCredential> {
        UserCredential::decode(invoke_async("signInWithCustomToken", vec![string(token)]).await?)
    }

    /// Sign in with a provider credential, e.g. tokens from Google or Apple sign-in.
    pub async fn sign_in_with_credential(
        &self,
        credential: &AuthCredential,
    ) -> Result<UserCredential> {
        UserCredential::decode(
            invoke_async("signInWithCredential", vec![credential.encode()]).await?,
        )
    }

    pub async fn send_password_reset_email(&self, email: &str) -> Result<()> {
        unit(invoke_async("sendPasswordResetEmail", vec![string(email)]).await?)
    }

    pub fn sign_out(&self) -> Result<()> {
        unit(invoke("signOut", vec![])?)
    }

    /// Called with the current user immediately, then on every sign-in and sign-out.
    pub fn on_auth_state_changed(
        &self,
        callback: impl Fn(Option<User>) + 'static,
    ) -> Result<ListenerRegistration> {
        self.listen("auth", callback)
    }

    /// Like [`on_auth_state_changed`](Self::on_auth_state_changed), plus ID token refreshes.
    pub fn on_id_token_changed(
        &self,
        callback: impl Fn(Option<User>) + 'static,
    ) -> Result<ListenerRegistration> {
        self.listen("id_token", callback)
    }

    fn listen(
        &self,
        kind: &'static str,
        callback: impl Fn(Option<User>) + 'static,
    ) -> Result<ListenerRegistration> {
        let listeners = LISTENERS.with(Rc::clone);
        listen(
            &listeners,
            "removeListener",
            move |wire| {
                // Malformed events are dropped rather than reported as a sign-out.
                if let Ok(user) = wire.and_then(User::decode_optional) {
                    callback(user);
                }
            },
            |id| unit(invoke("addListener", vec![Wire::Int(id), string(kind)])?),
        )
    }

    /// The signed-in user as a signal, updated on sign-in and sign-out.
    /// Call inside a component: the listener stops when the component is disposed.
    pub fn user_signal(&self) -> ReadSignal<Option<User>> {
        let this = *self;
        signal_from_listener(
            self.current_user().ok().flatten(),
            move |set| this.on_auth_state_changed(set),
            |_| None,
        )
    }
}

/// A provider credential for [`Auth::sign_in_with_credential`] and account linking.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthCredential {
    provider_id: String,
    fields: BTreeMap<&'static str, String>,
}

impl std::fmt::Debug for AuthCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthCredential")
            .field("provider_id", &self.provider_id)
            .finish_non_exhaustive()
    }
}

impl AuthCredential {
    fn new<'a>(
        provider_id: &str,
        fields: impl IntoIterator<Item = (&'static str, Option<&'a str>)>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            fields: fields
                .into_iter()
                .filter_map(|(key, value)| Some((key, value?.to_owned())))
                .collect(),
        }
    }

    /// Email and password, e.g. for reauthentication or linking an anonymous account.
    pub fn email(email: &str, password: &str) -> Self {
        Self::new(
            "password",
            [("email", Some(email)), ("password", Some(password))],
        )
    }

    /// Tokens from Google Sign-In. iOS requires both tokens.
    pub fn google(id_token: &str, access_token: Option<&str>) -> Self {
        Self::new(
            "google.com",
            [("id_token", Some(id_token)), ("access_token", access_token)],
        )
    }

    /// Sign in with Apple. `raw_nonce` is the unhashed nonce used for the request.
    pub fn apple(id_token: &str, raw_nonce: Option<&str>) -> Self {
        Self::new(
            "apple.com",
            [("id_token", Some(id_token)), ("raw_nonce", raw_nonce)],
        )
    }

    /// Another OAuth/OIDC provider configured in the Firebase console, e.g. `microsoft.com`.
    pub fn oauth(provider_id: &str, id_token: Option<&str>, access_token: Option<&str>) -> Self {
        Self::new(
            provider_id,
            [("id_token", id_token), ("access_token", access_token)],
        )
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn encode(&self) -> Wire {
        let mut fields: BTreeMap<String, Wire> = self
            .fields
            .iter()
            .map(|(key, value)| ((*key).to_owned(), string(value)))
            .collect();
        fields.insert("provider".into(), string(&self.provider_id));
        Wire::Map(fields)
    }
}

/// Profile information from one sign-in provider linked to a user.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UserInfo {
    pub provider_id: String,
    pub uid: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub photo_url: Option<String>,
    pub phone_number: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UserMetadata {
    pub creation_time: Option<Timestamp>,
    pub last_sign_in_time: Option<Timestamp>,
}

/// A signed-in user at the time it was read. Methods act on the SDK's current user and
/// fail with `no-current-user` or `user-mismatch` if a different user is signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct User {
    pub uid: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub photo_url: Option<String>,
    pub phone_number: Option<String>,
    pub email_verified: bool,
    pub is_anonymous: bool,
    pub provider_data: Vec<UserInfo>,
    pub metadata: UserMetadata,
}

/// The result of signing in, creating an account, or linking a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UserCredential {
    pub user: User,
    pub additional_user_info: Option<AdditionalUserInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AdditionalUserInfo {
    pub is_new_user: bool,
    pub provider_id: Option<String>,
    pub username: Option<String>,
}

/// Changes for [`User::update_profile`]. Unset fields are left unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileUpdate {
    display_name: Option<Option<String>>,
    photo_url: Option<Option<String>>,
}

impl ProfileUpdate {
    pub fn new() -> Self {
        Self::default()
    }

    /// `None` clears the display name.
    pub fn display_name(mut self, name: Option<&str>) -> Self {
        self.display_name = Some(name.map(Into::into));
        self
    }

    /// `None` clears the photo URL.
    pub fn photo_url(mut self, url: Option<&str>) -> Self {
        self.photo_url = Some(url.map(Into::into));
        self
    }
}

impl User {
    fn args(&self, rest: impl IntoIterator<Item = Wire>) -> Vec<Wire> {
        std::iter::once(string(&self.uid)).chain(rest).collect()
    }

    /// A Firebase ID token for your backend; cached until it nears expiry unless forced.
    pub async fn id_token(&self, force_refresh: bool) -> Result<String> {
        match invoke_async("getIdToken", self.args([Wire::Bool(force_refresh)])).await? {
            Wire::String(token) => Ok(token),
            _ => Err(FirebaseError::invalid_response(
                SERVICE,
                "expected an ID token",
            )),
        }
    }

    /// Fetch the latest profile from the server.
    pub async fn reload(&self) -> Result<User> {
        User::decode(invoke_async("reload", self.args([])).await?)
    }

    /// Returns the updated user.
    pub async fn update_profile(&self, update: &ProfileUpdate) -> Result<User> {
        let mut fields = BTreeMap::new();
        for (key, value) in [
            ("display_name", &update.display_name),
            ("photo_url", &update.photo_url),
        ] {
            if let Some(value) = value {
                fields.insert(key.to_owned(), value.as_deref().map_or(Wire::Null, string));
            }
        }
        User::decode(invoke_async("updateProfile", self.args([Wire::Map(fields)])).await?)
    }

    /// May fail with `requires-recent-login`; reauthenticate and retry.
    pub async fn update_password(&self, password: &str) -> Result<()> {
        unit(invoke_async("updatePassword", self.args([string(password)])).await?)
    }

    /// Send a verification link; the email changes once the user follows it.
    pub async fn verify_before_update_email(&self, email: &str) -> Result<()> {
        unit(invoke_async("verifyBeforeUpdateEmail", self.args([string(email)])).await?)
    }

    pub async fn send_email_verification(&self) -> Result<()> {
        unit(invoke_async("sendEmailVerification", self.args([])).await?)
    }

    /// Delete the account and sign out. May fail with `requires-recent-login`.
    pub async fn delete(&self) -> Result<()> {
        unit(invoke_async("deleteUser", self.args([])).await?)
    }

    /// Attach another sign-in method, e.g. upgrade an anonymous account to email/password.
    pub async fn link_with_credential(
        &self,
        credential: &AuthCredential,
    ) -> Result<UserCredential> {
        UserCredential::decode(
            invoke_async("linkWithCredential", self.args([credential.encode()])).await?,
        )
    }

    pub async fn reauthenticate_with_credential(
        &self,
        credential: &AuthCredential,
    ) -> Result<UserCredential> {
        UserCredential::decode(
            invoke_async(
                "reauthenticateWithCredential",
                self.args([credential.encode()]),
            )
            .await?,
        )
    }

    fn decode_optional(wire: Wire) -> Result<Option<User>> {
        match wire {
            Wire::Null => Ok(None),
            wire => User::decode(wire).map(Some),
        }
    }

    fn decode(wire: Wire) -> Result<User> {
        let fields = map(wire)?;
        let provider_data = match fields.get("provider_data") {
            Some(Wire::Array(items)) => items
                .iter()
                .cloned()
                .map(|item| {
                    let item = map(item)?;
                    Ok(UserInfo {
                        provider_id: required(&item, "provider_id")?,
                        uid: required(&item, "uid")?,
                        email: optional(&item, "email"),
                        display_name: optional(&item, "display_name"),
                        photo_url: optional(&item, "photo_url"),
                        phone_number: optional(&item, "phone_number"),
                    })
                })
                .collect::<Result<_>>()?,
            _ => Vec::new(),
        };
        let time = |key| match fields.get(key) {
            Some(Wire::Int(millis)) => Timestamp::from_millis(*millis).ok(),
            _ => None,
        };
        Ok(User {
            uid: required(&fields, "uid")?,
            email: optional(&fields, "email"),
            display_name: optional(&fields, "display_name"),
            photo_url: optional(&fields, "photo_url"),
            phone_number: optional(&fields, "phone_number"),
            email_verified: matches!(fields.get("email_verified"), Some(Wire::Bool(true))),
            is_anonymous: matches!(fields.get("is_anonymous"), Some(Wire::Bool(true))),
            provider_data,
            metadata: UserMetadata {
                creation_time: time("creation_time"),
                last_sign_in_time: time("last_sign_in_time"),
            },
        })
    }
}

impl UserCredential {
    fn decode(wire: Wire) -> Result<Self> {
        let mut fields = map(wire)?;
        let user = User::decode(fields.remove("user").unwrap_or(Wire::Null))?;
        let additional_user_info = match fields.remove("additional_user_info") {
            Some(Wire::Map(info)) => Some(AdditionalUserInfo {
                is_new_user: matches!(info.get("is_new_user"), Some(Wire::Bool(true))),
                provider_id: optional(&info, "provider_id"),
                username: optional(&info, "username"),
            }),
            _ => None,
        };
        Ok(Self {
            user,
            additional_user_info,
        })
    }
}

fn map(wire: Wire) -> Result<BTreeMap<String, Wire>> {
    match wire {
        Wire::Map(fields) => Ok(fields),
        _ => Err(FirebaseError::invalid_response(SERVICE, "expected a map")),
    }
}

fn required(fields: &BTreeMap<String, Wire>, key: &str) -> Result<String> {
    optional(fields, key)
        .ok_or_else(|| FirebaseError::invalid_response(SERVICE, format!("missing {key}")))
}

fn optional(fields: &BTreeMap<String, Wire>, key: &str) -> Option<String> {
    match fields.get(key) {
        Some(Wire::String(value)) => Some(value.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_error_names_use_web_codes() {
        let error = normalize(FirebaseError::new(
            SERVICE,
            "ERROR_EMAIL_ALREADY_IN_USE",
            "taken",
        ));
        assert_eq!(error.code, "email-already-in-use");
        assert_eq!(error.to_string(), "auth/email-already-in-use: taken");
        assert_eq!(
            normalize(FirebaseError::new(SERVICE, "no-current-user", "")).code,
            "no-current-user"
        );
    }

    #[test]
    fn users_decode_with_optional_fields() {
        let wire = Wire::Map(
            [
                ("uid".into(), string("u1")),
                ("is_anonymous".into(), Wire::Bool(true)),
                ("creation_time".into(), Wire::Int(1_700_000_000_000)),
                (
                    "provider_data".into(),
                    Wire::Array(vec![Wire::Map(
                        [
                            ("provider_id".into(), string("password")),
                            ("uid".into(), string("a@b.c")),
                        ]
                        .into(),
                    )]),
                ),
            ]
            .into(),
        );
        let user = User::decode(wire).unwrap();
        assert!(user.is_anonymous && user.email.is_none());
        assert_eq!(user.provider_data[0].provider_id, "password");
        assert_eq!(
            user.metadata.creation_time.unwrap().seconds(),
            1_700_000_000
        );
        assert!(User::decode_optional(Wire::Null).unwrap().is_none());
        let credential = AuthCredential::apple("token", None);
        assert!(!format!("{credential:?}").contains("token"));
    }
}
