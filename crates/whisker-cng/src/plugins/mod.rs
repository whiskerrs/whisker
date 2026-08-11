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
//! Every built-in is opt-in: the engine runs it on every `compose()`
//! call, but a `Config::default()` produces an empty contribution, so
//! nothing lands in the IR until the user writes
//! `app.plugin::<MyPlugin>(|c| …)`.
//!
//! [`app_icon`] is the exception to the "narrow" rule and to where
//! declaration types live: its `AppIcon` / `AppIconConfig` sit in
//! `whisker-config`, because the config probe depends only on that
//! crate and so any type the user names in `app.plugin::<…>` must be
//! reachable from there.
//!
//! A new built-in needs a module here and a registration line in
//! [`crate::Engine::with_builtins`].

pub mod android_application_attributes;
pub mod android_extra_files;
pub mod android_gradle_dependencies;
pub mod android_gradle_plugins;
pub mod android_meta_data;
pub mod android_permissions;
pub mod app_icon;
pub mod info_plist_extra;
pub mod ios_extra_files;
pub mod ios_pbxproj_ops;
