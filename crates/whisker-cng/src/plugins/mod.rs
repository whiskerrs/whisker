//! Built-in Whisker CNG plugins.
//!
//! Each module here implements one [`whisker_plugin::Plugin`] that
//! the engine registers automatically via
//! [`crate::Engine::with_builtins`]. Plugins are intentionally
//! narrow — one IR field, one mutation, no cross-plugin
//! coordination — so a 3rd-party plugin can rely on a stable set of
//! upstream writers when expressing `after()` / `before()` hints.
//!
//! ## Opt-in semantics
//!
//! Every built-in is opt-in: the engine runs it on every
//! `compose()` call, but it only writes IR entries when the user
//! supplied a non-default Config via
//! `app.plugin::<…Config>(|c| …)`. A built-in's `Config::default()`
//! produces an empty contribution, so apps that don't declare any
//! built-in see the legacy behavior bit-identical.
//!
//! ## Why these three
//!
//! - **[`info_plist_extra`]** — covers "I need an extra Info.plist
//!   key I didn't expect" (privacy strings, capabilities, custom
//!   URL schemes). The most common reason apps end up patching
//!   Xcode-generated plists by hand.
//! - **[`android_permissions`]** — covers the same shape for
//!   Android's `<uses-permission>` list. Single most common manifest
//!   edit.
//! - **[`android_meta_data`]** — covers `<meta-data>` rows inside
//!   `<application>`. Required by Firebase, Google Maps SDK keys,
//!   App Links host declarations, and most other 1st-party Google
//!   SDKs.
//!
//! Future built-ins land here additively. Each new entry needs a
//! brief justification in this list and a registration line in
//! [`crate::Engine::with_builtins`].

pub mod android_extra_files;
pub mod android_gradle_dependencies;
pub mod android_gradle_plugins;
pub mod android_meta_data;
pub mod android_permissions;
pub mod info_plist_extra;
pub mod ios_extra_files;
pub mod ios_pbxproj_ops;
