//! Signal primitives: [`ReadSignal`], [`WriteSignal`], [`RwSignal`],
//! and the [`signal`] constructor.
//!
//! All three handle types are `Copy` newtypes over a [`NodeId`]; the
//! actual value lives in the runtime arena and is shared via an
//! `Rc<RefCell<dyn Any>>`. Cloning a handle is free; passing one into
//! a `move ||` closure doesn't tie any lifetime.
//!
//! The Solid-style tuple form `let (count, set_count) = signal(0)`
//! splits read and write capability into separate types, so a child
//! component can be handed `count: ReadSignal<i32>` without the
//! ability to write. The unified [`RwSignal`] is also provided for
//! cases where the same code site needs both halves.

use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use super::runtime::{NodeData, NodeId, Owner, ReactiveNode};
use super::scheduler;
use super::with_runtime;

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

/// Allocate a fresh signal in the current owner. Returns a
/// `(ReadSignal, WriteSignal)` pair — Solid-style separation.
///
/// ```ignore
/// let (count, set_count) = signal(0);
/// set_count.set(1);
/// assert_eq!(count.get(), 1);
/// ```
pub fn signal<T: 'static>(initial: T) -> (ReadSignal<T>, WriteSignal<T>) {
    let id = alloc_signal_node(initial);
    (
        ReadSignal {
            id,
            _ty: PhantomData,
        },
        WriteSignal {
            id,
            _ty: PhantomData,
        },
    )
}

fn alloc_signal_node<T: 'static>(initial: T) -> NodeId {
    let value: Rc<RefCell<dyn Any>> = Rc::new(RefCell::new(initial));
    let needs_warning = with_runtime(|rt| rt.current_owner().is_none());
    if needs_warning {
        super::warn_no_owner("signal()");
    }
    with_runtime(|rt| {
        let owner = rt.current_owner().unwrap_or_else(|| {
            // No current owner — fall back to a "global" detached owner.
            // We create one lazily so primitives created outside of any
            // explicit `with_owner` (e.g. in tests, or at app startup
            // before the first component mounts) still have a place to
            // live. They will only be freed by `__reset_for_tests` or
            // explicit dispose.
            let detached = rt.owners.insert(Owner::new(None));
            rt.owner_stack.push(detached);
            detached
        });
        let id = rt.nodes.insert(ReactiveNode {
            owner,
            data: NodeData::Signal { value },
            sources: Default::default(),
            subscribers: Default::default(),
        });
        if let Some(o) = rt.owners.get_mut(owner) {
            o.nodes.push(id);
        }
        id
    })
}

// ---------------------------------------------------------------------------
// ReadSignal — read-only handle
// ---------------------------------------------------------------------------

/// Read-only signal handle. `Copy`; safe to clone freely and move
/// into closures.
pub struct ReadSignal<T: 'static> {
    pub(crate) id: NodeId,
    pub(crate) _ty: PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for ReadSignal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for ReadSignal<T> {}

impl<T: 'static + Clone> ReadSignal<T> {
    /// Read the current value, registering this signal as a dependency
    /// of the currently-running effect / memo (if any).
    pub fn get(self) -> T {
        self.with(|v| v.clone())
    }

    /// Read without registering a dependency — useful inside effects
    /// where you want to read a value but not subscribe to it.
    pub fn get_untracked(self) -> T {
        self.with_untracked(|v| v.clone())
    }
}

impl<T: 'static> ReadSignal<T> {
    /// Borrowed read with dependency tracking. Useful when `T` is
    /// expensive to clone or doesn't implement `Clone`.
    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        let value = track_and_fetch(self.id);
        let borrow = value.borrow();
        let typed = borrow
            .downcast_ref::<T>()
            .expect("ReadSignal::with: type mismatch — signal storage corrupted");
        f(typed)
    }

    /// Borrowed read without tracking.
    pub fn with_untracked<R>(self, f: impl FnOnce(&T) -> R) -> R {
        let value = fetch_value(self.id);
        let borrow = value.borrow();
        let typed = borrow
            .downcast_ref::<T>()
            .expect("ReadSignal::with_untracked: type mismatch — signal storage corrupted");
        f(typed)
    }
}

// ---------------------------------------------------------------------------
// WriteSignal — write-only handle
// ---------------------------------------------------------------------------

/// Write-only signal handle. `Copy`. Setting or updating notifies all
/// subscribers; the notifications are enqueued (not run synchronously)
/// to support batched event-handler semantics — call [`flush`] to
/// drain.
///
/// [`flush`]: super::scheduler::flush
pub struct WriteSignal<T: 'static> {
    pub(crate) id: NodeId,
    pub(crate) _ty: PhantomData<fn(T)>,
}

impl<T: 'static> Clone for WriteSignal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for WriteSignal<T> {}

impl<T: 'static> WriteSignal<T> {
    /// Replace the value, notifying subscribers.
    pub fn set(self, value: T) {
        self.update(move |slot| *slot = value);
    }

    /// Mutate the value in place, notifying subscribers.
    pub fn update(self, f: impl FnOnce(&mut T)) {
        write_and_notify(self.id, f, /* notify = */ true);
    }

    /// Mutate without notifying subscribers — escape hatch for cases
    /// where you want to update internal state without triggering a
    /// re-render. Use sparingly; the typical reason this is wrong is
    /// that you actually do want subscribers to see the change.
    pub fn update_untracked(self, f: impl FnOnce(&mut T)) {
        write_and_notify(self.id, f, /* notify = */ false);
    }
}

// ---------------------------------------------------------------------------
// RwSignal — combined read/write handle
// ---------------------------------------------------------------------------

/// Combined read-write signal handle. Equivalent to holding both a
/// `ReadSignal<T>` and `WriteSignal<T>` for the same underlying node.
pub struct RwSignal<T: 'static> {
    id: NodeId,
    _ty: PhantomData<fn(T) -> T>,
}

impl<T: 'static> Clone for RwSignal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for RwSignal<T> {}

impl<T: 'static> RwSignal<T> {
    /// Allocate a new combined-handle signal.
    pub fn new(initial: T) -> Self {
        let id = alloc_signal_node(initial);
        Self {
            id,
            _ty: PhantomData,
        }
    }

    /// Split into separate read and write halves. The handles continue
    /// to refer to the same underlying value.
    pub fn split(self) -> (ReadSignal<T>, WriteSignal<T>) {
        (
            ReadSignal {
                id: self.id,
                _ty: PhantomData,
            },
            WriteSignal {
                id: self.id,
                _ty: PhantomData,
            },
        )
    }

    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        ReadSignal::<T> {
            id: self.id,
            _ty: PhantomData,
        }
        .with(f)
    }

    pub fn with_untracked<R>(self, f: impl FnOnce(&T) -> R) -> R {
        ReadSignal::<T> {
            id: self.id,
            _ty: PhantomData,
        }
        .with_untracked(f)
    }

    pub fn update(self, f: impl FnOnce(&mut T)) {
        WriteSignal::<T> {
            id: self.id,
            _ty: PhantomData,
        }
        .update(f);
    }

    pub fn update_untracked(self, f: impl FnOnce(&mut T)) {
        WriteSignal::<T> {
            id: self.id,
            _ty: PhantomData,
        }
        .update_untracked(f);
    }

    pub fn set(self, value: T) {
        WriteSignal::<T> {
            id: self.id,
            _ty: PhantomData,
        }
        .set(value);
    }
}

impl<T: 'static + Clone> RwSignal<T> {
    pub fn get(self) -> T {
        ReadSignal::<T> {
            id: self.id,
            _ty: PhantomData,
        }
        .get()
    }

    pub fn get_untracked(self) -> T {
        ReadSignal::<T> {
            id: self.id,
            _ty: PhantomData,
        }
        .get_untracked()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers — keep the runtime borrow window narrow
// ---------------------------------------------------------------------------

/// Look up the value handle for `id`, register the current tracker as
/// a subscriber, and return a clone of the Rc. The borrow on the
/// runtime is released before the caller dereferences the value, so
/// user code inside the eventual `f(&T)` is free to re-enter the
/// runtime.
fn track_and_fetch(id: NodeId) -> Rc<RefCell<dyn Any>> {
    with_runtime(|rt| {
        if let Some(tracker) = rt.current_tracker {
            // Avoid self-subscription (a memo reading its own value).
            if tracker != id {
                if let Some(node) = rt.nodes.get_mut(id) {
                    node.subscribers.insert(tracker);
                }
                if let Some(track_node) = rt.nodes.get_mut(tracker) {
                    track_node.sources.insert(id);
                }
            }
        }
        rt.nodes
            .get(id)
            .and_then(|n| n.data.value().cloned())
            .expect("ReadSignal: signal disposed or not a value-bearing node")
    })
}

fn fetch_value(id: NodeId) -> Rc<RefCell<dyn Any>> {
    with_runtime(|rt| {
        rt.nodes
            .get(id)
            .and_then(|n| n.data.value().cloned())
            .expect("ReadSignal: signal disposed or not a value-bearing node")
    })
}

/// Mutate the value of signal `id` under `f`, optionally notifying
/// subscribers afterwards.
fn write_and_notify<T: 'static>(id: NodeId, f: impl FnOnce(&mut T), notify: bool) {
    // Step 1: pull a clone of the Rc handle in a short borrow.
    let value = with_runtime(|rt| {
        rt.nodes
            .get(id)
            .and_then(|n| n.data.value().cloned())
            .expect("WriteSignal: signal disposed or not a value-bearing node")
    });

    // Step 2: mutate without holding the runtime borrow.
    {
        let mut borrow = value.borrow_mut();
        let typed = borrow
            .downcast_mut::<T>()
            .expect("WriteSignal: type mismatch — signal storage corrupted");
        f(typed);
    }

    // Step 3: schedule subscribers.
    if notify {
        let subscribers: Vec<NodeId> = with_runtime(|rt| {
            rt.nodes
                .get(id)
                .map(|n| n.subscribers.iter().copied().collect())
                .unwrap_or_default()
        });
        for sub in subscribers {
            scheduler::schedule(sub);
        }
    }
}

