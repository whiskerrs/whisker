//! Whisker CNG (Continuous Native Generation).
//!
//! Renders platform Host projects under `gen/<platform>/`
//! from the user's `whisker.rs` (= [`whisker_config::Config`]).
//! Drift between the in-tree files and the current config is detected
//! via a content-hashed fingerprint stored alongside each generated
//! tree (`gen/<platform>/.whisker-fingerprint`).
//!
//! Modelled on Expo's CNG: the declarative config is the source of
//! truth and `gen/` is a build artifact, never committed. Unlike Expo
//! there is no separate `whisker generate` command — every command
//! that needs the native tree (`whisker run`, `whisker build`) calls
//! [`sync_android`] / [`sync_ios`] first, and the fingerprint-match
//! fast path is a single file read.
//!
//! ## Public entry points
//!
//! - [`sync_android`] / [`sync_ios`] / [`sync_macos`] — render-or-skip for one
//!   platform. Returns whether files were actually rewritten.
//! - [`AndroidInputs`] / [`IosInputs`] — the renderer's input bundle.
//!   Build them yourself for full control, or use
//!   [`android::inputs_from`] / [`ios::inputs_from`] for the
//!   "extract from Config + defaults" path.
//!
//! The crate has no CLI surface and shells out to nothing —
//! `whisker-cli` runs `xcodegen`, `gradle`, etc. after a sync
//! completes, which keeps the renderer unit-testable against tempdirs.

pub mod android;
pub mod compose;
pub mod discovery;
mod fingerprint;
pub mod ios;
pub mod macos;
pub mod plugins;
mod render;
pub mod web;

pub use android::{AndroidInputs, sync as sync_android};
pub use compose::{EnabledTargets, Engine, SubprocessPlugin};
pub use discovery::{DiscoveredPlugin, discover_plugins};
pub use ios::{IosInputs, sync as sync_ios};
pub use macos::{MacosInputs, sync as sync_macos};
pub use web::{WebInputs, sync as sync_web};
pub use whisker_config::Config;
