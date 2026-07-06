//! Age-encrypted credential store, committed to the user's repo.
//!
//! ## Model
//!
//! Signing credentials (Android upload keystores, App Store Connect
//! API keys) live **encrypted inside the app repository** under
//! `<project>/credentials/`, so the repo itself is the backup and
//! the team-share mechanism. One asymmetric age key pair protects
//! the whole store:
//!
//! - `credentials/recipients.txt` — the **public** key(s). Committed
//!   in plaintext. Anyone with a clone can *add* credentials
//!   (encryption needs only this file).
//! - The **secret** key (`AGE-SECRET-KEY-1…`) is never written to
//!   disk by whisker. It is shown exactly once at bootstrap (the
//!   user stores it in a password manager) and afterwards arrives
//!   via [`IDENTITY_ENV`] or an interactive prompt at build time.
//!
//! ## Layout
//!
//! ```text
//! credentials/
//!   recipients.txt                                  (plaintext, committed)
//!   ios/default/asc.json.age                        (team-wide ASC API key)
//!   ios/<bundle_id>/asc.json.age                    (per-bundle override)
//!   android/<application_id>/keystore.jks.age       (upload keystore)
//!   android/<application_id>/keystore.json.age      (passwords + alias)
//! ```
//!
//! iOS resolution falls back `<bundle_id>` → `default` because an
//! ASC key is team-scoped (one key signs every bundle id in the
//! team); the per-bundle entry exists for the rare "dev builds ship
//! under a different Apple team" setup. Android is exact-match only:
//! an upload keystore is a per-app asset.
//!
//! ## Boundary
//!
//! This crate owns encrypt/decrypt, layout, and plaintext staging
//! ([`MaterializedDir`], deleted on drop). It deliberately has **no
//! TTY or UI dependencies** — prompting for the secret key and
//! running wizards is the CLI's job (`whisker credential …`), which
//! keeps `whisker build`'s pre-step a pure library call.

mod entries;
mod materialize;
mod store;

pub use entries::{
    AndroidSigning, AscKey, IosSigning, KeystoreMeta, android_keystore_rel, android_meta_rel,
    ios_asc_rel,
};
pub use materialize::MaterializedDir;
pub use store::{Identity, Store};

/// Environment variable carrying the age secret key
/// (`AGE-SECRET-KEY-1…`). Checked before any interactive prompt; the
/// only credential-related secret a CI environment needs.
pub const IDENTITY_ENV: &str = "WHISKER_CREDENTIALS_KEY";

/// Store directory name under the project root. Committed to git —
/// everything inside except `recipients.txt` is age ciphertext.
pub const DIR_NAME: &str = "credentials";

/// Public-key list file inside [`DIR_NAME`].
pub const RECIPIENTS_FILE: &str = "recipients.txt";
