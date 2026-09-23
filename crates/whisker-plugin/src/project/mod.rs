//! Declarative project models for iOS, Android, macOS, Windows, Linux, and Web
//!
//! These types describe the desired project, including build products and OS
//! metadata. They do not contain Xcode object IDs or requests to edit templates.
//! Plugin authors and generator implementors can construct and inspect them
//! without a platform SDK. [`ProjectIr::validate_structure`] checks references
//! before a backend interprets platform-specific settings.
//!
//! [`ProjectPlugin`] and [`protocol`] compose these models through CNG's
//! ProjectEngine. Android, iOS, macOS, Windows, Linux, and Web generators and renderers consume this model.
//! Other platform renderers still use their existing contracts; constructing
//! those models alone does not change their generated projects. The legacy
//! [`crate::GenerateContext`] is retained for legacy mobile plugin compatibility.
//!
//! Maps use stable, caller-chosen IDs; ordered lists preserve build or document
//! order. File references are relative to the generated project. External app
//! files must be staged through [`ProjectFile`]. No implicit target, file, or
//! dependency is inserted by these types. Platform assemblers supply defaults.
//! Unknown fields fail deserialization rather than silently losing declarations.
//! The separate [`protocol`] requires exact wire/schema agreement before any
//! project data is sent to a subprocess.

#![warn(missing_docs)]

mod android;
mod apple;
mod desktop;
mod files;
mod merge;
mod plugin;
pub mod protocol;
mod validate;
mod web;
mod xml;

pub use android::*;
pub use apple::*;
pub use desktop::*;
pub use files::*;
pub use plugin::*;
pub use web::*;
pub use xml::*;

use serde::{Deserialize, Serialize};

/// One platform's complete project declaration
///
/// The platform discriminator separates OS behavior even where the underlying
/// build structures are shared, as with iOS and macOS. Package formats belong
/// to their respective platform models and are optional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "platform",
    content = "project",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProjectIr {
    /// An iOS application and its additional targets.
    Ios(IosProjectIr),
    /// An Android application and its Gradle modules.
    Android(Box<AndroidProjectIr>),
    /// A macOS application bundle and its additional targets.
    Macos(MacosProjectIr),
    /// A Windows application with optional MSIX packaging.
    Windows(WindowsProjectIr),
    /// A Linux desktop application with optional package recipes.
    Linux(LinuxProjectIr),
    /// A Web document, static resources, and optional PWA metadata.
    Web(Box<WebProjectIr>),
}

impl ProjectIr {
    /// Merge declarations for the same platform atomically.
    ///
    /// Equal declarations are idempotent; conflicting scalar or named values
    /// are errors. No defaults are injected. Platform-specific merge contracts
    /// apply, and graph completeness is checked separately after composition.
    pub fn merge_from(&mut self, contribution: &Self) -> anyhow::Result<()> {
        match (self, contribution) {
            (Self::Ios(a), Self::Ios(b)) => a.merge_from(b),
            (Self::Android(a), Self::Android(b)) => a.merge_from(b),
            (Self::Macos(a), Self::Macos(b)) => a.merge_from(b),
            (Self::Windows(a), Self::Windows(b)) => a.merge_from(b),
            (Self::Linux(a), Self::Linux(b)) => a.merge_from(b),
            (Self::Web(a), Self::Web(b)) => a.merge_from(b),
            _ => anyhow::bail!("plugin cannot change the project platform"),
        }
    }

    /// Check project IDs, entry points, graph references, and dependency cycles
    ///
    /// Returns an error for a missing main product, unknown target/module/package
    /// reference, cyclic per-platform Apple build graph, invalid staged file tree, or undeclared
    /// executable in a desktop package. This does not read the filesystem or
    /// validate native XML schemas, build-setting names, signing credentials,
    /// resource existence, or support in a particular renderer. Android dependency
    /// cycles are delegated to Gradle because configurations select different graphs.
    pub fn validate_structure(&self) -> anyhow::Result<()> {
        validate::project(self)
    }
}

#[cfg(test)]
mod tests;
