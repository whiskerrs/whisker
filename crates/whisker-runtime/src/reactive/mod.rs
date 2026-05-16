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

pub mod effect;
pub mod memo;
pub mod owner;
pub mod runtime;
pub mod scheduler;
pub mod signal;

#[cfg(test)]
mod tests;

pub use effect::effect;
pub use memo::{memo, Memo};
pub use owner::{create_owner, dispose_owner, on_cleanup, with_owner};
pub use runtime::{NodeId, OwnerId};
pub use scheduler::flush;
pub use signal::{signal, ReadSignal, RwSignal, WriteSignal};

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
pub fn with_runtime<R>(f: impl FnOnce(&mut ReactiveRuntime) -> R) -> R {
    RUNTIME.with_borrow_mut(f)
}

/// (Test only) reset the thread-local runtime to an empty state. Used
/// between unit tests to keep the arena clean.
#[doc(hidden)]
pub fn __reset_for_tests() {
    RUNTIME.with(|r| *r.borrow_mut() = ReactiveRuntime::new());
}
