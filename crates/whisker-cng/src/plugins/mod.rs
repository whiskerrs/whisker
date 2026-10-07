//! Built-in contributions and application policy for CNG.
//!
//! `application` owns mandatory app-config initialization and final fallback
//! values. It is separate from the opt-in native configuration helpers and the
//! [`app_icon`] feature plugin. The engine owns scheduling, config decoding and
//! conflict checks; mobile application plugins own native scaffolding.
//!
//! Optional plugins use an empty default config and retain their stable names
//! and public module paths. Their single registration list lives here and is
//! used by [`crate::Engine::with_builtins`]. Registration order is not execution
//! order: the engine applies each plugin's before/after constraints.
//!
//! AppIcon's declaration/config types live in whisker-config so the config
//! probe can name them without depending on the generation engine. Its plugin
//! implementation and image generation live in [`app_icon`].
//!
//! These plugins retain the crate-root mobile GenerateContext contract. Android and iOS
//! also implement the declarative ProjectPlugin contract using shared config
//! and image processing.

pub(crate) mod application;
mod project;

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

/// Register the optional built-ins without exposing native policy in the engine.
pub(crate) fn register_builtins(engine: &mut crate::Engine) {
    engine
        .register(info_plist_extra::InfoPlistExtra)
        .register(android_permissions::AndroidPermissions)
        .register(android_meta_data::AndroidMetaData)
        .register(android_application_attributes::AndroidApplicationAttributes)
        .register(android_gradle_plugins::GradlePlugins)
        .register(android_gradle_dependencies::GradleDependencies)
        .register(ios_extra_files::IosExtraFiles)
        .register(android_extra_files::AndroidExtraFiles)
        .register(ios_pbxproj_ops::IosPbxprojOps)
        .register(app_icon::AppIcon);
}
