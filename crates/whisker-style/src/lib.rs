//! Typed inline style semantics shared by Whisker's authoring, scene, and UI
//! layers.
//!
//! This crate owns stable property identities and will own typed declaration
//! storage, composition, inheritance, and computed-style resolution. It has no
//! dependency on a renderer, Host binding, or CSS parser.

#![warn(missing_docs)]

mod property;

pub use property::{PropertyMetadata, PropertyOrigin, StyleProperty, StylePropertyId};
