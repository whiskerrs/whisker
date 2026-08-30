//! Core runtime for Whisker.
//!
//! Public surface:
//!
//! - [`element`] — the [`ElementTag`](element::ElementTag) enum that
//!   the macro emit and the C bridge agree on.
//! - [`reactive`] — Leptos-style fine-grained reactivity: signals,
//!   effects, memos, owner tree, component lifecycle, context.
//! - [`view`] — element-handle + type-erased renderer (`DynRenderer`)
//!   the `render!` macro emits against. Includes `Show` / `For`
//!   control flow.
//! - [`runtime_wake`] — Host's "wake up" callback, pinged by the reactive
//!   scheduler when new work appears.
//! - [`dispatch`] — instance-scoped posting back to the Host-owned runtime
//!   thread from background work.

pub mod anim_hook;
mod background_resources;
mod dispatch;
pub mod element;
mod element_registry;
pub mod event;
pub mod module;
pub mod reactive;
mod runtime_context;
mod runtime_instance;
pub mod runtime_wake;
mod standard_ui;
mod surface_runtime;
pub mod tasks;
mod transform_interpolation;
pub mod value;
pub mod view;

/// Link-time registration hooks used by `#[whisker::module_component]`.
///
/// This is macro plumbing rather than application API.
#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub mod __linked_elements {
    pub use crate::element_registry::LINKED_ELEMENT_PROVIDERS;
    pub use linkme::*;
}

#[doc(hidden)]
pub use dispatch::drain_runtime_dispatches;
pub use dispatch::{RuntimeDispatcher, runtime_dispatcher};
pub use element::ElementTag;
pub use element_registry::{
    ElementAuthoringBinding, ElementModuleDefinition, ElementProviderMetadata, ElementRegistry,
    ElementRegistryBuilder, ElementRegistryError,
};
pub use runtime_context::RuntimeContext;
pub use runtime_instance::{
    RuntimeDrive, RuntimeDriveError, RuntimeEventError, RuntimeInstance, RuntimeLifecycle,
    RuntimeLifecycleError,
};
pub use runtime_wake::{RuntimeWake, RuntimeWakeHandle};
pub use standard_ui::{
    SCROLL_BY_COMMAND, SCROLL_ENABLED_PROPERTY, SCROLL_TO_COMMAND, SCROLL_VIEW_ELEMENT_NAME,
    TEXT_ELEMENT_NAME, VIEW_ELEMENT_NAME, scroll_view_element_binding, text_element_binding,
    view_element_binding,
};
pub use surface_runtime::{
    InputDispatch, ResourceEventApply, RuntimeBindingError, RuntimeFrame, RuntimeFrameError,
    RuntimeInputError, RuntimeLayoutError, RuntimePresentError, RuntimeResourceError,
    SurfaceRuntime, standard_element_registrations,
};
