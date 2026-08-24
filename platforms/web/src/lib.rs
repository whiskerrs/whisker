//! Browser DOM Host for Whisker.
//!
//! Rust remains authoritative for style resolution and Taffy layout. This Host
//! synchronously measures browser text and applies the resulting semantic frame
//! transaction to DOM nodes using explicit geometry.

#![warn(missing_docs)]

/// Marks and defines a platform implementation contributed by a module.
pub use whisker::WhiskerModule;

/// Browser bindings used by Rust-authored Web Host contributions.
pub use wasm_bindgen;
/// DOM bindings used by Rust-authored Web Host contributions.
pub use web_sys;
/// Shared value used by Web module properties, functions, and events.
pub use whisker_value::WhiskerValue;

mod application;
mod dom;
mod measure;
mod module_api;
mod paint;
mod scene;

pub use application::run;
pub use dom::WebError;
pub(crate) use dom::{js_error, px, set_style};
pub(crate) use module_api::WebElementFactoryKind;
pub use module_api::*;

#[cfg(all(test, target_arch = "wasm32"))]
#[path = "tests/host_conformance.rs"]
mod host_conformance_tests;
