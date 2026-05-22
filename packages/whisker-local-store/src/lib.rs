//! `whisker-local-store` — reference Whisker native module.
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
//! Whisker native modules: cargo-discoverable manifest + Rust trait
//! marked `#[whisker::native_module(name = "...")]` + a
//! `@WhiskerModule(...)`-annotated class on each platform. Phase
//! 7-Φ.E follow-up modules (`whisker-network`, `whisker-clipboard`,
//! …) follow the same shape.
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
//! Each method returns `Result<T, whisker::native_module::WhiskerModuleError>` —
//! the platform side (UserDefaults / SharedPreferences) can't
//! reasonably fail under normal conditions, but Whisker's bridge
//! reports any class-lookup / dispatch issue through the same
//! channel so callers handle every error uniformly.

// `#![deny(missing_docs)]` would fire on the unit struct +
// methods the `#[whisker::native_module]` proc macro emits — the
// macro doesn't propagate the trait method docs onto the emitted
// proxy methods. Doc comments on the trait itself + the
// public-facing module are what users see in IDE hovers; the
// generated struct is an implementation detail.

/// Typed Rust proxy for the `WhiskerLocalStore` native module.
///
/// The `#[whisker::native_module]` proc macro (Phase 7-Φ.E.5)
/// generates a unit struct + associated `pub fn save / load /
/// remove` methods that marshal args + return through
/// `whisker::native_module::invoke`. The platform-side class
/// (`WhiskerLocalStoreImpl` — `src/ios/WhiskerLocalStoreImpl.swift`
/// / `src/android/WhiskerLocalStoreImpl.kt`) provides the actual
/// UserDefaults / SharedPreferences storage.
#[whisker::native_module(name = "WhiskerLocalStore")]
pub trait WhiskerLocalStore {
    /// Persist `value` under `key`. Returns `true` on success.
    ///
    /// Subsequent [`load`](WhiskerLocalStore::load) calls with the
    /// same key resolve to `Some(value)` even across app launches.
    /// Overwrites any previously-stored value for that key.
    fn save(key: String, value: String) -> bool;

    /// Look up the value previously stored under `key`.
    ///
    /// Returns `None` when no entry exists. Doesn't distinguish
    /// "never saved" from "saved then removed" — both surface as
    /// `None`. Use [`save`](WhiskerLocalStore::save) to set a
    /// sentinel value if you need that distinction.
    fn load(key: String) -> Option<String>;

    /// Drop `key`'s entry (if any). No-op when the key isn't
    /// present.
    fn remove(key: String) -> ();
}
