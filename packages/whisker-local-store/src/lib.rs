//! `whisker-local-store` — reference Whisker platform module.
//!
//! Persistent string-keyed key-value store backed by:
//!   - **iOS**: `UserDefaults.standard`
//!   - **Android**: `SharedPreferences` (named `"WhiskerLocalStore"`)
//!
//! Both platforms persist across app launches but don't sync across
//! devices. For app-level preferences, small state, and similar
//! data — not for large blobs (use a real file API or database for
//! anything > ~1 MB per entry).
//!
//! The module is also the documented template for first-party
//! Whisker function-only modules. Shape:
//!
//!   - A hand-written typed wrapper — `pub struct WhiskerLocalStore`
//!     — whose methods (`save` / `load` / `remove`) build the raw
//!     `Vec<WhiskerValue>` arg list, dispatch via
//!     `whisker::module!("WhiskerLocalStore").invoke(method, args)`,
//!     and lift the returned `WhiskerValue` into a typed `Result`.
//!     This is where validation / defaults / ergonomic types live;
//!     the framework primitive (`PlatformModule::invoke`) stays a
//!     `WhiskerValue`-only pass-through.
//!
//! The platform side declares the matching `@WhiskerModule` DSL
//! (`Name("WhiskerLocalStore")` + module-level `Function`s) — the
//! per-platform codegen registers a dispatch shim under the
//! crate-namespaced `<crate>:WhiskerLocalStore` key, which is what
//! `module!` resolves to.
//!
//! ## Usage
//!
//! Add the crate to your app's `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! whisker-local-store = { path = "../whisker/packages/whisker-local-store" }
//! ```
//!
//! Call from anywhere reactive runs (component bodies, event
//! handlers, effects):
//!
//! ```ignore
//! use whisker_local_store::WhiskerLocalStore;
//!
//! let _ = WhiskerLocalStore::save("user_id".into(), "abc".into())?;
//! let loaded = WhiskerLocalStore::load("user_id".into())?;
//! // -> Some("abc".to_string())
//! ```
//!
//! Each method returns `Result<T, WhiskerModuleError>` — the
//! platform side can't reasonably fail under normal conditions,
//! but Whisker's bridge reports any class-lookup / dispatch
//! issue through the same channel so callers handle every error
//! uniformly.

use whisker::platform_module::{WhiskerModuleError, WhiskerValue};

/// Typed Rust API for the `WhiskerLocalStore` platform module.
///
/// Hand-written wrapper over the framework primitive: each method
/// builds the raw `Vec<WhiskerValue>` arg list, dispatches via
/// `whisker::module!("WhiskerLocalStore").invoke(method, args)`, and
/// lifts the returned `WhiskerValue` into the matching typed result.
///
/// `module!` prepends this crate's name (→ `<crate>:WhiskerLocalStore`)
/// so two crates can ship same-named modules without colliding. The
/// wrapper is the recommended shape for first-party + community
/// modules — the bridge wire stays uniform (`WhiskerValue`-only)
/// while the public API is ergonomic Rust.
pub struct WhiskerLocalStore;

impl WhiskerLocalStore {
    /// Persist `value` under `key`. Returns `true` on success.
    ///
    /// Subsequent [`load`](WhiskerLocalStore::load) calls with
    /// the same key resolve to `Some(value)` even across app
    /// launches. Overwrites any previously-stored value.
    pub fn save(key: String, value: String) -> Result<bool, WhiskerModuleError> {
        let result = whisker::module!("WhiskerLocalStore").invoke(
            "save",
            vec![WhiskerValue::String(key), WhiskerValue::String(value)],
        );
        match result {
            WhiskerValue::Bool(b) => Ok(b),
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            other => Err(WhiskerModuleError(format!(
                "WhiskerLocalStore::save expected Bool, got {other:?}"
            ))),
        }
    }

    /// Look up the value previously stored under `key`.
    ///
    /// Returns `None` when no entry exists. Doesn't distinguish
    /// "never saved" from "saved then removed" — both surface as
    /// `None`. Use [`save`](WhiskerLocalStore::save) to set a
    /// sentinel value if you need that distinction.
    pub fn load(key: String) -> Result<Option<String>, WhiskerModuleError> {
        let result =
            whisker::module!("WhiskerLocalStore").invoke("load", vec![WhiskerValue::String(key)]);
        match result {
            WhiskerValue::Null => Ok(None),
            WhiskerValue::String(s) => Ok(Some(s)),
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            other => Err(WhiskerModuleError(format!(
                "WhiskerLocalStore::load expected String or Null, got {other:?}"
            ))),
        }
    }

    /// Drop `key`'s entry (if any). No-op when the key isn't
    /// present.
    pub fn remove(key: String) -> Result<(), WhiskerModuleError> {
        let result =
            whisker::module!("WhiskerLocalStore").invoke("remove", vec![WhiskerValue::String(key)]);
        match result {
            WhiskerValue::Null => Ok(()),
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            // Be permissive: any non-error return is treated as
            // success. The platform-side contract for `remove`
            // is "side effect only, return null", but a stale
            // return shape shouldn't fail the call.
            _ => Ok(()),
        }
    }
}
