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
/// Available-space constraint exposed to Web module measurers.
pub type WhiskerAvailableSpace = whisker_protocol::AvailableSpace;
/// Intrinsic measurement request exposed to Web module authors.
pub type WhiskerMeasureRequest = whisker_protocol::ModuleMeasureRequest;
/// Logical size returned by a Web module measurer.
pub type WhiskerMeasuredSize = whisker_protocol::MeasuredSize;
/// Resolved inherited text style delivered to Web module content.
pub type WhiskerTextStyle = whisker_protocol::TextStyleSnapshot;

mod application;
mod dom;
mod input;
mod measure;
mod module_api;
mod paint;
mod scene;

pub use application::{handle_resource_command, register_resource_url, run};
pub use dom::WebError;
pub(crate) use dom::{js_error, px, set_style};
pub(crate) use module_api::WebElementFactoryKind;
pub use module_api::*;
pub use scene::resource_service::{WebResourceService, WebResourceState};
pub use scene::resource_store::WebResourceStore;

#[cfg(all(test, target_arch = "wasm32"))]
#[path = "tests/host_conformance.rs"]
mod host_conformance_tests;
