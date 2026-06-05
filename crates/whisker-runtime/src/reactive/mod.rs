//! Leptos-style fine-grained reactivity.
//!
//! Design: see `docs/reactivity-design.md`.
//!
//! Quick map:
//!
//! - [`runtime`] — internal data structures: [`ReactiveRuntime`],
//!   [`runtime::Scope`], [`ReactiveNode`], [`NodeData`].
//! - [`owner`] — public owner API: the [`Owner`] handle and its
//!   methods ([`Owner::new`], [`Owner::with`], [`Owner::dispose`],
//!   [`Owner::pause`], [`Owner::resume`], [`Owner::is_paused`])
//!   plus the free function [`on_cleanup`].
//! - [`signal`] — [`signal`] / [`RwSignal`] / [`ReadSignal`] /
//!   [`WriteSignal`].
//! - [`effect`] — [`effect`] + dependency tracking.
//! - [`computed`] — [`computed`] (returns [`ReadSignal<T>`]).
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

pub mod arc_signal;
pub mod component;
pub mod computed;
pub mod context;
pub mod effect;
pub mod owner;
pub mod prop;
pub mod resource;
pub mod runtime;
pub mod scheduler;
pub mod signal;
pub mod stored;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_resource;

pub use arc_signal::{arc_signal, ArcReadSignal, ArcRwSignal, ArcWriteSignal};
#[doc(hidden)]
pub use component::__reset_pending_mount_for_tests;
pub use component::{
    flush_mounts, mount_component, mount_component_remountable, on_component_root_attached,
    on_mount, owners_for_fn, remount_components_for, unmount_component, MountId,
};
pub use computed::computed;
pub use context::{provide_context, use_context, with_context};
pub use effect::effect;
// `on_cleanup` lives at the module top-level because it acts on
// whichever owner is currently on the runtime stack — the caller
// can't name it. Everything else owner-related lives behind the
// `owner` module path (re-exported below) so users write
// `whisker::owner::Owner::new(None)` / `owner.with(...)` / etc.
pub use owner::on_cleanup;
pub use prop::Signal;
pub use resource::{resource, resource_sync, Resource, ResourceState};
pub use runtime::{NodeId, Owner};
pub use scheduler::flush;
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
/// / `computed` / `Owner::dispose` etc. functions. Exposing the raw
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

/// Run `f` with the runtime's `current_tracker` temporarily cleared,
/// then restore it.
///
/// Use this whenever a reactive primitive needs to invoke a user
/// closure that performs signal reads **but those reads must not
/// register dependencies against whatever outer effect / computed
/// happens to be running**. The canonical case is `computed`'s
/// construction-time seed run: the seed is just for cache initial
/// value, the real dependency edges are registered by the
/// scheduler-driven run that happens immediately afterwards.
///
/// The restore runs from a `Drop` guard, so a panic in `f` doesn't
/// leave the runtime in an "untracked" state. Re-entrant safe — if
/// `f` itself calls `untrack`, the nested guard restores `None`
/// (the value the outer guard already pushed), which is what the
/// outer guard would have done.
pub(crate) fn untrack<R>(f: impl FnOnce() -> R) -> R {
    use runtime::NodeId;

    struct Restore(Option<NodeId>);
    impl Drop for Restore {
        fn drop(&mut self) {
            // `with_runtime` reborrows the thread-local; the previous
            // borrow opened in `untrack` was already released before
            // `f` started, so this is a fresh borrow — no
            // double-borrow risk.
            with_runtime(|rt| rt.current_tracker = self.0);
        }
    }

    let prev = with_runtime(|rt| rt.current_tracker.take());
    let _guard = Restore(prev);
    f()
}

/// (Test only) reset the thread-local runtime to an empty state. Used
/// between unit tests to keep the arena clean.
#[doc(hidden)]
pub fn __reset_for_tests() {
    RUNTIME.with(|r| *r.borrow_mut() = ReactiveRuntime::new());
}
