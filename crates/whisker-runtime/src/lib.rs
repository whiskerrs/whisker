//! Core runtime for Whisker.
//!
//! Public surface, after the Phase 6.5a A3 cleanup:
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
//!
//! Pre-A3 the crate also exposed an `Element` value tree + diff/patch
//! pipeline; that retired when the macro switched to emitting
//! imperative `view::*` calls driven by reactive effects.

pub mod element;
pub mod host_wake;
pub mod main_thread;
pub mod reactive;
pub mod tasks;
pub mod view;
