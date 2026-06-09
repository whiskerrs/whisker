//! Whisker CNG (Continuous Native Generation).
//!
//! Renders the Android / iOS host projects under `gen/{android,ios}/`
//! from the user's `whisker.rs` (= [`whisker_config::Config`]).
//! Drift between the in-tree files and the current config is detected
//! via a content-hashed fingerprint stored alongside each generated
//! tree (`gen/<platform>/.whisker-fingerprint`).
//!
//! Modelled on Expo's CNG: the source of truth is the declarative
//! config, the platforms directory are build artifacts (never
//! committed). Unlike Expo, regeneration is *implicit* — there's no
//! separate `whisker generate` command. Whichever command needs the
//! native tree (today: `whisker run`) calls
//! [`sync_android`] / [`sync_ios`] first; the fast path (fingerprint
//! match) is a single file read and returns instantly.
//!
//! ## Public entry points
//!
//! - [`sync_android`] / [`sync_ios`] — render-or-skip for one
//!   platform. Returns whether files were actually rewritten.
//! - [`AndroidInputs`] / [`IosInputs`] — the renderer's input bundle.
//!   Build them yourself for full control, or use
//!   [`android::inputs_from`] / [`ios::inputs_from`] for the
//!   "extract from Config + defaults" path.
//!
//! The crate has no CLI surface and shells out to nothing —
//! `whisker-cli` is responsible for running `xcodegen`, `gradle`, etc.
//! after a sync completes. Keeping side-effects out of the renderer
//! makes it cheap to unit-test against tempdirs.

pub mod android;
pub mod compose;
pub mod discovery;
mod fingerprint;
pub mod ios;
pub mod plugins;
mod render;

pub use android::{sync as sync_android, AndroidInputs};
pub use compose::{EnabledTargets, Engine, SubprocessPlugin};
pub use discovery::{discover_plugins, DiscoveredPlugin};
pub use ios::{sync as sync_ios, IosInputs};
pub use whisker_config::Config;
