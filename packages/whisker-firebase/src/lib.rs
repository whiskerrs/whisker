//! Firebase for Whisker on Android and iOS.
//!
//! Enable services with Cargo features; each links only its own native SDK.
mod plugin;
pub use plugin::{WhiskerFirebase, WhiskerFirebaseConfig};
#[cfg(feature = "auth")]
pub use whisker_firebase_auth as auth;
pub use whisker_firebase_core::{
    FirebaseApp, FirebaseError, ListenerRegistration, Result, Timestamp,
};
#[cfg(feature = "firestore")]
pub use whisker_firebase_firestore as firestore;
#[cfg(feature = "storage")]
pub use whisker_firebase_storage as storage;
