//! Refcount-backed signal primitives — the `Arc*` family.
//!
//! Unlike the arena-backed [`signal`](super::signal) primitives,
//! whose lifetime is bounded by the [`Owner`](super::runtime::Owner)
//! that allocated them, an Arc signal owns its value through an
//! [`Rc`] and survives for as long as any handle on it remains.
//! That makes the Arc family the right tool for:
//!
//! - Process-global state stashed in a `OnceLock` / `static` slot —
//!   the canonical example is `whisker_safe_area`'s `SafeAreaInsets`
//!   signal, which has to outlive every route that happens to
//!   trigger the lazy init.
//! - State shared across components whose lifecycles don't nest
//!   (e.g. routes in a stack navigator, tabs that mount and unmount
//!   independently, modals rendered through a portal).
//! - Collections like `HashMap<K, ArcRwSignal<V>>` where each value's
//!   lifetime is governed by whoever holds it rather than by the
//!   component that put it in.
//!
//! The Copy [`RwSignal`](super::signal::RwSignal) family is still
//! the right default for component-local state: an arena handle is a
//! single integer, can move freely into `'static` closures, and gets
//! cleaned up automatically when the component unmounts. Reach for
//! the Arc family only when those owner-tied semantics don't fit.
//!
//! ## Lifecycle model
//!
//! ```text
//!  Strong: refcount on Rc<ArcSignalInner>
//!  Weak:   Vec<NodeId> of subscribers (effects / computeds in the arena)
//!
//!  ┌──────────────────────────────┐
//!  │ ArcRwSignal<T>               │ ← Rc handle; live while
//!  │   inner: Rc<ArcSignalInner>  │   refcount ≥ 1
//!  └──────────────┬───────────────┘
//!                 │ Rc::clone for every subscriber's `arc_sources`
//!                 ▼
//!  ┌──────────────────────────────┐
//!  │ ArcSignalInner<T>            │
//!  │   value: RefCell<T>          │
//!  │   subscribers: Vec<NodeId> ──┼──► arena effect / computed nodes
//!  └──────────────────────────────┘    (back-link by NodeId — Weak in
//!                                       effect: the subscriber can die
//!                                       freely; we prune stale NodeIds
//!                                       on next `set` and on owner
//!                                       disposal).
//! ```
//!
//! The strong/weak split makes the failure mode of the Copy variants
//! — reading a `NodeId` after its arena slot is freed — structurally
//! impossible: the value is owned by `Rc`, not by an arena slot, so
//! it stays alive whenever a handle stays alive.
//!
//! ## Conversions
//!
//! For now this module only provides the Arc-native primitives. A
//! follow-up may add `From<ArcRwSignal<T>> for RwSignal<T>` so module
//! authors can stash `ArcRwSignal` at the storage boundary and hand
//! out arena-backed Copy handles at the API surface (the Leptos
//! pattern). The Arc-only API already covers the bug class that
//! motivated this change.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use super::runtime::{ArcSubscription, NodeId};
use super::with_runtime;

/// Shared inner state of every Arc-backed signal.
///
/// `value` is the actual data; `subscribers` is the list of reactive
/// nodes (effects, computeds) that read this signal and want to be
/// re-scheduled when it changes. Subscribers are stored as `NodeId`s
/// — the signal is the "value owner", the subscriber arena nodes
/// own themselves; we don't keep the subscriber alive by listing it
/// here. Stale `NodeId`s (whose arena slot got freed) are pruned
/// lazily on the next `notify_subscribers` call and eagerly during
/// `dispose_owner` (see `crates/whisker-runtime/src/reactive/owner.rs`).
pub(crate) struct ArcSignalInner<T> {
    value: RefCell<T>,
    subscribers: RefCell<Vec<NodeId>>,
}

impl<T: 'static> ArcSubscription for ArcSignalInner<T> {
    fn unsubscribe(&self, subscriber: NodeId) {
        self.subscribers.borrow_mut().retain(|n| *n != subscriber);
    }
}

/// Register the running tracker (effect / computed) as a subscriber of
/// `inner`. Bidirectional: `inner.subscribers` gains the tracker's
/// `NodeId`, and the tracker's `arc_sources` gains an
/// `Rc<dyn ArcSubscription>` pointing at `inner` so the scheduler
/// can detach the back-link on the next re-run (see
/// `scheduler::run_node_if_alive`).
fn track<T: 'static>(inner: &Rc<ArcSignalInner<T>>) {
    let added = {
        let mut subs = inner.subscribers.borrow_mut();
        with_runtime(|rt| {
            if let Some(tracker) = rt.current_tracker {
                if !subs.contains(&tracker) {
                    subs.push(tracker);
                    return Some(tracker);
                }
            }
            None
        })
    };
    if let Some(tracker) = added {
        // Borrow released; safe to re-enter runtime.
        let cleanup: Rc<dyn ArcSubscription> = inner.clone();
        with_runtime(|rt| {
            if let Some(node) = rt.nodes.get_mut(tracker) {
                node.arc_sources.push(cleanup);
            }
        });
    }
}

/// Schedule every live subscriber for re-run; prune any whose arena
/// slot has been freed. Mirrors `signal::write_and_notify` but reads
/// the subscriber list from the Arc signal's own storage instead of
/// from an arena node.
///
/// Subscribers go through [`super::scheduler::schedule`] (not a raw
/// `rt.pending.push`) so the host wake on the empty→non-empty edge
/// fires. Without that, writes from a bridge callback (e.g.
/// `module.on_event(...)` updating a global `ArcRwSignal`) queued
/// dirty effects but never flushed them until some other source
/// happened to trigger a reactive cycle — manifesting as a UI
/// bound to the signal that simply never re-rendered. Stale
/// subscriber pruning is independent of the wake.
fn notify_subscribers<T: 'static>(inner: &Rc<ArcSignalInner<T>>) {
    let subscribers: Vec<NodeId> = inner.subscribers.borrow().clone();
    let mut stale: Vec<NodeId> = Vec::new();
    // First pass: identify stale subscribers so the dedupe walk
    // inside `schedule()` doesn't end up with freed slots.
    with_runtime(|rt| {
        for sub in &subscribers {
            if !rt.nodes.contains_key(*sub) {
                stale.push(*sub);
            }
        }
    });
    for sub in &subscribers {
        if !stale.contains(sub) {
            super::scheduler::schedule(*sub);
        }
    }
    if !stale.is_empty() {
        inner
            .subscribers
            .borrow_mut()
            .retain(|n| !stale.contains(n));
    }
}

// ---------------------------------------------------------------------------
// ArcRwSignal — combined read/write Arc handle
// ---------------------------------------------------------------------------

/// Refcount-backed combined read/write signal.
///
/// `Clone` (refcount bump), **not** `Copy`. To pass into multiple
/// closures, clone explicitly — same `Rc`/`Arc` ergonomics rules.
/// Most app code shouldn't reach for `ArcRwSignal` directly; the
/// arena-backed [`RwSignal`](super::signal::RwSignal) is more
/// ergonomic. Use `ArcRwSignal` when the signal needs to outlive
/// its declaring scope (see the module doc for typical patterns).
pub struct ArcRwSignal<T: 'static> {
    pub(crate) inner: Rc<ArcSignalInner<T>>,
}

impl<T: 'static> Clone for ArcRwSignal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: 'static> ArcRwSignal<T> {
    /// Create a fresh Arc-backed signal initialised to `value`. The
    /// signal lives for as long as any handle on it (the returned
    /// value, plus any clone) remains alive — disposal of the owner
    /// that called `new` has no effect.
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(ArcSignalInner {
                value: RefCell::new(value),
                subscribers: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Project to a read-only handle pointing at the same underlying
    /// signal. Mirrors [`RwSignal::read_only`](super::signal::RwSignal::read_only).
    pub fn read_only(&self) -> ArcReadSignal<T> {
        ArcReadSignal {
            inner: self.inner.clone(),
            _ty: PhantomData,
        }
    }

    /// Project to a write-only handle.
    pub fn write_only(&self) -> ArcWriteSignal<T> {
        ArcWriteSignal {
            inner: self.inner.clone(),
            _ty: PhantomData,
        }
    }

    /// Split into separate read and write halves backed by the same
    /// underlying signal.
    pub fn split(self) -> (ArcReadSignal<T>, ArcWriteSignal<T>) {
        (
            ArcReadSignal {
                inner: self.inner.clone(),
                _ty: PhantomData,
            },
            ArcWriteSignal {
                inner: self.inner,
                _ty: PhantomData,
            },
        )
    }

    /// Read with dependency tracking — registers the currently-running
    /// effect / computed as a subscriber.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        track(&self.inner);
        f(&self.inner.value.borrow())
    }

    /// Read without registering a dependency.
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.inner.value.borrow())
    }

    /// Replace the value and notify subscribers.
    pub fn set(&self, value: T) {
        self.update(move |slot| *slot = value);
    }

    /// Mutate the value in place and notify subscribers.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.inner.value.borrow_mut());
        notify_subscribers(&self.inner);
    }

    /// Mutate without notifying subscribers — escape hatch for
    /// updates that shouldn't propagate.
    pub fn update_untracked(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.inner.value.borrow_mut());
    }
}

impl<T: 'static + Clone> ArcRwSignal<T> {
    /// Read the current value, registering this signal as a
    /// dependency of the currently-running effect / computed.
    pub fn get(&self) -> T {
        self.with(|v| v.clone())
    }

    /// Read without registering a dependency.
    pub fn get_untracked(&self) -> T {
        self.with_untracked(|v| v.clone())
    }
}

// ---------------------------------------------------------------------------
// ArcReadSignal — read-only Arc handle
// ---------------------------------------------------------------------------

/// Refcount-backed read-only handle. Clone-cheap (`Rc` bump),
/// `!Copy`. The companion to [`ArcRwSignal::read_only`].
pub struct ArcReadSignal<T: 'static> {
    pub(crate) inner: Rc<ArcSignalInner<T>>,
    pub(crate) _ty: PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for ArcReadSignal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _ty: PhantomData,
        }
    }
}

impl<T: 'static> ArcReadSignal<T> {
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        track(&self.inner);
        f(&self.inner.value.borrow())
    }

    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.inner.value.borrow())
    }
}

impl<T: 'static + Clone> ArcReadSignal<T> {
    pub fn get(&self) -> T {
        self.with(|v| v.clone())
    }

    pub fn get_untracked(&self) -> T {
        self.with_untracked(|v| v.clone())
    }
}

// ---------------------------------------------------------------------------
// ArcWriteSignal — write-only Arc handle
// ---------------------------------------------------------------------------

/// Refcount-backed write-only handle. Clone-cheap (`Rc` bump),
/// `!Copy`. The companion to [`ArcRwSignal::write_only`].
pub struct ArcWriteSignal<T: 'static> {
    pub(crate) inner: Rc<ArcSignalInner<T>>,
    pub(crate) _ty: PhantomData<fn(T)>,
}

impl<T: 'static> Clone for ArcWriteSignal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _ty: PhantomData,
        }
    }
}

impl<T: 'static> ArcWriteSignal<T> {
    pub fn set(&self, value: T) {
        self.update(move |slot| *slot = value);
    }

    pub fn update(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.inner.value.borrow_mut());
        notify_subscribers(&self.inner);
    }

    pub fn update_untracked(&self, f: impl FnOnce(&mut T)) {
        f(&mut self.inner.value.borrow_mut());
    }
}

// ---------------------------------------------------------------------------
// Top-level constructor — Solid-style `(read, write)` tuple
// ---------------------------------------------------------------------------

/// Allocate a fresh Arc-backed signal and split into read/write
/// halves. The Arc analog of [`signal`](super::signal::signal):
///
/// ```ignore
/// use whisker_runtime::reactive::arc_signal;
///
/// let (count, set_count) = arc_signal(0_i32);
/// set_count.set(1);
/// assert_eq!(count.get(), 1);
/// ```
///
/// The signal lives until both returned halves and any clone of
/// them are dropped — process lifetime if either ends up in a
/// `static` slot.
pub fn arc_signal<T: 'static>(initial: T) -> (ArcReadSignal<T>, ArcWriteSignal<T>) {
    ArcRwSignal::new(initial).split()
}
