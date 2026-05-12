//! CNG (Continuous Native Generation) codegen.
//!
//! Given an `AppConfig` plus the result of running each plugin's
//! `lyra_plugin(&mut PrebuildContext)`, emit:
//! - `ios/` — Xcode project, Podfile, Info.plist, AppDelegate, etc.
//! - `android/` — Gradle project, AndroidManifest, MainActivity, etc.
//!
//! Inputs are accumulated by `lyra-plugin::PrebuildContext`; this crate
//! is the rendering side.
