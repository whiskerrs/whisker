//! Leptos-style fine-grained reactivity.
//!
//! Design: see `docs/reactivity-design.md`.
//!
//! Quick map:
//!
//! - [`runtime`] — internal data structures: [`ReactiveRuntime`],
//!   [`Owner`], [`ReactiveNode`], [`NodeData`].
//! - [`owner`] — owner lifecycle: [`create_owner`], [`dispose_owner`],
//!   [`with_owner`], [`on_cleanup`].
//! - [`signal`] — [`signal`] / [`RwSignal`] / [`ReadSignal`] /
//!   [`WriteSignal`].
//! - [`effect`] — [`effect`] + dependency tracking.
//! - [`memo`] — [`memo`] / [`Memo`].
//! - [`scheduler`] — batching / flush.
//!
//! All operations are single-threaded — reactive UI runs on the Lynx
//! TASM thread. The runtime lives in a `thread_local!`. Operations
//! that need a runtime borrow go through [`with_runtime`], which gives
//! a `&mut ReactiveRuntime` for the duration of the closure.
//!
//! ## Why a single thread-local
//!
//! Lynx renders UI on its TASM thread; Whisker's bridge schedules all
//! reactive work onto that thread (see
//! `whisker-driver-sys/bridge/src/whisker_bridge_common.cc`). A single
//! thread-local instance keeps the implementation borrow-checker-clean
//! (no `Arc`, no locks) while matching how the runtime actually
//! executes.

pub mod component;
pub mod context;
pub mod effect;
pub mod memo;
pub mod owner;
pub mod runtime;
pub mod scheduler;
pub mod signal;
pub mod stored;

#[cfg(test)]
mod tests;

pub use component::{
    flush_mounts, mount_component, mount_component_remountable, on_component_root_attached,
    on_mount, owners_for_fn, remount_components_for, unmount_component, MountId,
};
#[doc(hidden)]
pub use component::__reset_pending_mount_for_tests;
pub use context::{provide_context, use_context, with_context};
pub use effect::effect;
pub use memo::{memo, Memo};
pub use owner::{create_owner, dispose_owner, on_cleanup, with_owner};
pub use runtime::{NodeId, OwnerId};
pub use scheduler::{flush, mark_all_dirty};
pub use signal::{signal, ReadSignal, RwSignal, WriteSignal};
pub use stored::StoredValue;

use std::cell::RefCell;

use runtime::ReactiveRuntime;

thread_local! {
    /// The per-thread reactive runtime. Created lazily on first access.
    ///
    /// All public primitives funnel through [`with_runtime`]; nothing
    /// else should touch this directly.
    static RUNTIME: RefCell<ReactiveRuntime> = RefCell::new(ReactiveRuntime::new());
}

/// Open a mutable borrow on the thread-local runtime and run `f`.
///
/// **The borrow is held only for the duration of `f`.** User code that
/// needs to re-enter the runtime (e.g. reading a signal from inside an
/// effect closure) MUST drop this borrow first — the implementations
/// in this module take care to copy out whatever data they need
/// (Rc handles, NodeIds, …) in a short borrow before invoking user
/// closures.
///
/// Crate-internal; the public surface is the typed `signal` / `effect`
/// / `memo` / `dispose_owner` etc. functions. Exposing the raw
/// runtime would let callers violate borrow-window invariants.
pub(crate) fn with_runtime<R>(f: impl FnOnce(&mut ReactiveRuntime) -> R) -> R {
    RUNTIME.with_borrow_mut(f)
}

/// Warn (debug only) when a reactive primitive is allocated outside
/// any owner. The fallback path creates a detached owner that's never
/// disposed, so this is mostly OK for one-offs (tests, app bootstrap)
/// but should not happen inside steady-state component code.
#[cfg(debug_assertions)]
pub(crate) fn warn_no_owner(context: &'static str) {
    eprintln!(
        "whisker-reactive: {context} called outside any owner scope; \
         allocating in a detached owner. The node will leak until \
         `__reset_for_tests` or manual disposal."
    );
}
#[cfg(not(debug_assertions))]
pub(crate) fn warn_no_owner(_context: &'static str) {}

/// (Test only) reset the thread-local runtime to an empty state. Used
/// between unit tests to keep the arena clean.
#[doc(hidden)]
pub fn __reset_for_tests() {
    RUNTIME.with(|r| *r.borrow_mut() = ReactiveRuntime::new());
}
