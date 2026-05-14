//! Core runtime for Whisker.
//!
//! Layered roughly as:
//!
//! - [`renderer`]: low-level abstraction for *something* that can build
//!   an element tree (the production impl talks to the C++ bridge; tests
//!   use [`renderer::MockRenderer`] for host-side cargo tests).
//! - [`element`]: the [`Element`] data structure produced by user code.
//! - [`build`]: ergonomic constructors and the builder pattern.
//! - [`render`]: walks an [`Element`] tree, issuing renderer ops.
//! - [`patch`] / [`diff`]: Phase 6 — diff two [`Element`] trees into
//!   incremental [`patch::Patch`] ops.
//! - [`signal`]: Phase 8 — reactive primitives.
//! - [`runtime`]: Phase 8 — `run_app` ties everything together.

pub mod build;
pub mod diff;
pub mod element;
pub mod patch;
pub mod render;
pub mod renderer;
pub mod runtime;
pub mod signal;

pub mod prelude {
    pub use crate::build::{image, page, raw_text, scroll_view, text, view};
    pub use crate::element::{Element, ElementTag};
    pub use crate::renderer::Renderer;
    pub use crate::signal::{use_signal, Signal};
}
