//! `whisker-secure-store` — hardware-backed secure key-value store.
//!
//! **API shape — 5 (Static methods).** See
//! [`docs/module-api-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/module-api-design.md)
//! §"Shape 5". Stateless one-shot operations, namespaced under the
//! unit struct [`WhiskerSecureStore`] — `save` / `load` / `remove`,
//! each returning a `Result`. The same shape as
//! [`whisker-local-store`](https://docs.rs/whisker-local-store), but
//! the value never touches plaintext storage.
//!
//! Backed by the platform's **secure credential storage**:
//!   - **iOS**: Keychain (`kSecClassGenericPassword`), accessibility
//!     `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` — readable
//!     after first unlock, not iCloud-synced, not migrated to a new
//!     device / unencrypted backup.
//!   - **Android**: AES-256-GCM via **Google Tink**, with the Tink
//!     keyset wrapped (envelope-encrypted) by an Android-Keystore master
//!     key — hardware-backed where the device has a secure element.
//!     Ciphertext is stored in an app-private `SharedPreferences`.
//!     (Tink is Google's recommended replacement for the now-deprecated
//!     `EncryptedSharedPreferences`.)
//!
//! ## When to use this vs `whisker-local-store`
//!
//! Use **`whisker-secure-store`** for secrets: OAuth access / refresh
//! tokens, DPoP / signing private keys, API keys, anything an attacker
//! with filesystem access (rooted / jailbroken device, or an
//! unencrypted backup) must not read. Use **`whisker-local-store`**
//! (plaintext UserDefaults / SharedPreferences) for everything else —
//! UI preferences, cached non-secret profile fields, feature flags.
//!
//! Keep values small (secrets, not blobs). Both backends are credential
//! stores, not databases.
//!
//! ## Errors
//!
//! Unlike `whisker-local-store` (whose backends never fail), a secure
//! store genuinely can: a Keychain `OSStatus`, a Keystore exception, or
//! a Tink decrypt failure (e.g. the keyset was invalidated by a factory
//! reset / credential change) all surface as
//! `Err(WhiskerModuleError)`. Treat a `load` error the same as a miss
//! that requires re-authentication rather than a fatal condition.
//!
//! ## Usage
//!
//! ```ignore
//! use whisker_secure_store::WhiskerSecureStore;
//!
//! WhiskerSecureStore::save("session".into(), session_json)?;
//! let restored = WhiskerSecureStore::load("session".into())?; // Option<String>
//! WhiskerSecureStore::remove("session".into())?;
//! ```
//!
//! ## Native source
//!
//! Contributors: the matching platform module lives at
//!
//! - iOS: `packages/whisker-secure-store/ios/Sources/WhiskerSecureStore/SecureStoreModule.swift`
//!   (storage: `SecureStore.swift`)
//! - Android: `packages/whisker-secure-store/android/src/main/kotlin/rs/whisker/modules/securestore/SecureStoreModule.kt`
//!   (storage: `SecureStore.kt`)

use whisker::platform_module::{WhiskerModuleError, WhiskerValue};

/// Typed Rust API for the `WhiskerSecureStore` platform module.
///
/// Hand-written wrapper over the framework primitive: each method
/// builds the raw `Vec<WhiskerValue>` arg list, dispatches via
/// `whisker::module!("WhiskerSecureStore").invoke(method, args)`, and
/// lifts the returned `WhiskerValue` into the matching typed result.
/// `module!` prepends this crate's name (→ `<crate>:WhiskerSecureStore`)
/// so module names never collide across crates.
pub struct WhiskerSecureStore;

impl WhiskerSecureStore {
    /// Encrypt and persist `value` under `key`. Returns `true` on
    /// success.
    ///
    /// Overwrites any previously-stored value. A subsequent
    /// [`load`](WhiskerSecureStore::load) with the same key resolves to
    /// `Some(value)` across app launches. Returns
    /// `Err(WhiskerModuleError)` if the platform secure store rejects
    /// the write (Keychain `OSStatus` / Keystore / Tink failure).
    pub fn save(key: String, value: String) -> Result<bool, WhiskerModuleError> {
        let result = whisker::module!("WhiskerSecureStore").invoke(
            "save",
            vec![WhiskerValue::String(key), WhiskerValue::String(value)],
        );
        match result {
            WhiskerValue::Bool(b) => Ok(b),
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            other => Err(WhiskerModuleError(format!(
                "WhiskerSecureStore::save expected Bool, got {other:?}"
            ))),
        }
    }

    /// Decrypt and read the value previously stored under `key`.
    ///
    /// Returns `None` when no entry exists. A decryption failure (e.g.
    /// the keyset was invalidated) surfaces as `Err` — callers should
    /// generally treat that like a miss and re-authenticate.
    pub fn load(key: String) -> Result<Option<String>, WhiskerModuleError> {
        let result =
            whisker::module!("WhiskerSecureStore").invoke("load", vec![WhiskerValue::String(key)]);
        match result {
            WhiskerValue::Null => Ok(None),
            WhiskerValue::String(s) => Ok(Some(s)),
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            other => Err(WhiskerModuleError(format!(
                "WhiskerSecureStore::load expected String or Null, got {other:?}"
            ))),
        }
    }

    /// Drop `key`'s entry (if any). No-op when the key isn't present.
    pub fn remove(key: String) -> Result<(), WhiskerModuleError> {
        let result = whisker::module!("WhiskerSecureStore")
            .invoke("remove", vec![WhiskerValue::String(key)]);
        match result {
            WhiskerValue::Null => Ok(()),
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            // Permissive: `remove` is documented side-effect-only
            // (returns null), but a stale return shape shouldn't fail.
            _ => Ok(()),
        }
    }
}
