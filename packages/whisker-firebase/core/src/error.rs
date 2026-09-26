/// A Firebase SDK error or a failure to communicate with the native module.
///
/// `service` and `code` use the same strings as the Firebase JavaScript SDK, so
/// `Display` prints e.g. `firestore/permission-denied: …` or `auth/invalid-credential: …`.
/// Match on them directly:
///
/// ```ignore
/// match error.code.as_str() {
///     "permission-denied" => show_sign_in(),
///     "unavailable" => retry_later(),
///     _ => return Err(error),
/// }
/// ```
///
/// Codes produced by this crate rather than the SDK: `unsupported-platform`,
/// `invalid-argument` (rejected before reaching the SDK), `invalid-response`, and
/// `bridge-error` (the native module could not be reached).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FirebaseError {
    /// `app`, `firestore`, `auth`, `storage`, `messaging`, `analytics`, or `crashlytics`.
    pub service: &'static str,
    /// The service-specific error code, e.g. `not-found` or `email-already-in-use`.
    pub code: String,
    pub message: String,
}

pub type Result<T, E = FirebaseError> = std::result::Result<T, E>;

impl FirebaseError {
    pub fn new(service: &'static str, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            service,
            code: code.into(),
            message: message.into(),
        }
    }

    #[doc(hidden)]
    pub fn invalid_argument(service: &'static str, message: impl Into<String>) -> Self {
        Self::new(service, "invalid-argument", message)
    }

    #[doc(hidden)]
    pub fn invalid_response(service: &'static str, message: impl Into<String>) -> Self {
        Self::new(service, "invalid-response", message)
    }
}

impl std::fmt::Display for FirebaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}: {}", self.service, self.code, self.message)
    }
}

impl std::error::Error for FirebaseError {}
