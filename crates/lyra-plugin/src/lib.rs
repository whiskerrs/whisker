//! Plugin API for Lyra CNG (Continuous Native Generation).
//!
//! A plugin is a Rust crate that exposes:
//! ```ignore
//! pub fn lyra_plugin(ctx: &mut lyra_plugin::PrebuildContext) {
//!     // Mutate ctx via typed mods (with_ios_info_plist, with_android_manifest, ...)
//! }
//! ```
//!
//! `lyra prebuild` discovers plugins from `Cargo.toml` `[package.metadata.lyra]`
//! and invokes `lyra_plugin` on each, in dependency declaration order.

/// Carries app config plus per-platform mutators that plugins compose.
pub struct PrebuildContext {
    // typed mod hooks come here:
    //   pub fn with_ios_info_plist(&mut self, f: impl FnOnce(&mut InfoPlist))
    //   pub fn with_android_manifest(&mut self, f: impl FnOnce(&mut AndroidManifest))
    //   pub fn with_main_activity(&mut self, f: impl FnOnce(&mut MainActivityKt))
    //   pub fn with_app_delegate(&mut self, f: impl FnOnce(&mut AppDelegateSwift))
    //   pub fn with_ios_pods(&mut self, f: impl FnOnce(&mut Pods))
    //   pub fn with_android_gradle(&mut self, f: impl FnOnce(&mut Gradle))
    //   ...
}

impl PrebuildContext {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for PrebuildContext {
    fn default() -> Self {
        Self::new()
    }
}
