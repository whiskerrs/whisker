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
//! - [`host_wake`] — host's "wake up" callback, registered by
//!   `whisker-driver::bootstrap` and pinged by the reactive
//!   scheduler when new work appears.
//! - [`main_thread`] — `run_on_main_thread`, the worker-thread →
//!   TASM-thread marshaling primitive used to update signals from
//!   background work (HTTP fetch, channels, etc.).

pub mod anim_hook;
mod dispatch;
pub mod element;
pub mod event;
pub mod host_wake;
pub mod main_thread;
pub mod reactive;
mod runtime_context;
pub mod tasks;
pub mod value;
pub mod view;

#[doc(hidden)]
pub use dispatch::drain_runtime_dispatches;
pub use dispatch::{RuntimeDispatcher, runtime_dispatcher};
pub use host_wake::{RuntimeWake, RuntimeWakeHandle};
pub use runtime_context::RuntimeContext;
