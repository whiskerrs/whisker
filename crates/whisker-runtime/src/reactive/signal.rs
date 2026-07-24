//! Signal primitives: [`ReadSignal`], [`WriteSignal`], [`RwSignal`],
//! and the [`signal`] constructor.
//!
//! All three handle types are `Copy` newtypes over a [`NodeId`]; the
//! actual value lives in the runtime arena and is shared via an
//! `Rc<RefCell<dyn Any>>`. Cloning a handle is free; passing one into
//! a `move ||` closure doesn't tie any lifetime.
//!
//! `signal(0)` returns a combined [`RwSignal`] — the read+write handle
//! you reach for most, and the one that threads cleanly through `move`
//! closures and component props. When you want the Solid-style split
//! capability, call [`RwSignal::split`] for the `(ReadSignal,
//! WriteSignal)` pair, or [`RwSignal::read_only`] /
//! [`RwSignal::write_only`] for a single narrowed capability — so a
//! child component can be handed `count: ReadSignal<i32>` without the
//! ability to write.

use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use super::runtime::{NodeData, NodeId, ReactiveNode, Scope};
use super::scheduler;
use super::with_runtime;

/// Allocate a fresh signal in the current owner. Returns a combined
/// [`RwSignal`] — the read+write handle.
///
/// ```ignore
/// let count = signal(0);
/// count.set(1);
/// assert_eq!(count.get(), 1);
/// ```
///
/// For the Solid-style split capability, call [`RwSignal::split`] for
/// the `(ReadSignal, WriteSignal)` pair, or [`RwSignal::read_only`] /
/// [`RwSignal::write_only`] for a single narrowed capability:
///
/// ```ignore
/// let (count, set_count) = signal(0).split();
/// set_count.set(1);
/// assert_eq!(count.get(), 1);
/// ```
pub fn signal<T: 'static>(initial: T) -> RwSignal<T> {
    let id = alloc_signal_node(initial);
    RwSignal {
        id,
        _ty: PhantomData,
    }
}

fn alloc_signal_node<T: 'static>(initial: T) -> NodeId {
    let value: Rc<RefCell<dyn Any>> = Rc::new(RefCell::new(initial));
    let needs_warning = with_runtime(|rt| rt.current_owner().is_none());
    if needs_warning {
        super::warn_no_owner("signal()");
    }
    with_runtime(|rt| {
        let owner = rt.current_owner().unwrap_or_else(|| {
            // Detached fallback owner: primitives allocated outside any
            // `Owner::with` (tests, pre-mount startup) still need a
            // lifecycle pin; only `__reset_for_tests` / explicit dispose
            // will free them.
            let detached = rt.owners.insert(Scope::new(None));
            rt.owner_stack.push(detached);
            detached
        });
        let id = rt.nodes.insert(ReactiveNode {
            owner,
            data: NodeData::Signal { value },
            sources: Default::default(),
            subscribers: Default::default(),
            arc_sources: Vec::new(),
        });
        if let Some(o) = rt.owners.get_mut(owner) {
            o.nodes.push(id);
        }
        id
    })
}

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
    /// of the currently-running effect / computed (if any).
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
        let value = fetch_value(self.id);
        // Arc-backed storage routes through `ArcRwSignal`'s
        // tracker-aware path, which subscribes via `arc_sources`; the
        // arena `subscribers` set stays empty for these entries.
        let arc_handle: Option<super::arc_signal::ArcRwSignal<T>> = {
            let borrow = value.borrow();
            borrow
                .downcast_ref::<super::arc_signal::ArcRwSignal<T>>()
                .cloned()
        };
        if let Some(arc) = arc_handle {
            return arc.with(f);
        }
        track_node(self.id);
        let borrow = value.borrow();
        let typed = borrow
            .downcast_ref::<T>()
            .expect("ReadSignal::with: type mismatch — signal storage corrupted");
        f(typed)
    }

    /// Borrowed read without tracking.
    pub fn with_untracked<R>(self, f: impl FnOnce(&T) -> R) -> R {
        let value = fetch_value(self.id);
        let arc_handle: Option<super::arc_signal::ArcRwSignal<T>> = {
            let borrow = value.borrow();
            borrow
                .downcast_ref::<super::arc_signal::ArcRwSignal<T>>()
                .cloned()
        };
        if let Some(arc) = arc_handle {
            return arc.with_untracked(f);
        }
        let borrow = value.borrow();
        let typed = borrow
            .downcast_ref::<T>()
            .expect("ReadSignal::with_untracked: type mismatch — signal storage corrupted");
        f(typed)
    }
}

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

/// Combined read-write signal handle — what [`signal`] returns.
/// Equivalent to holding both a `ReadSignal<T>` and `WriteSignal<T>`
/// for the same underlying node. Use [`split`](RwSignal::split) for the
/// `(read, write)` pair, or [`read_only`](RwSignal::read_only) /
/// [`write_only`](RwSignal::write_only) to narrow to a single
/// capability.
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

/// Identity equality: two handles are equal when they address the same
/// reactive node, not when their *values* are equal (the value isn't even
/// read). Lets a `Copy` handle be used as a map/registry key.
impl<T: 'static> PartialEq for RwSignal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl<T: 'static> Eq for RwSignal<T> {}

impl<T: 'static> RwSignal<T> {
    /// Allocate a new combined-handle signal. Equivalent to
    /// [`signal`].
    pub fn new(initial: T) -> Self {
        signal(initial)
    }

    /// Project to a read-only handle pointing at the same underlying
    /// value. Useful when handing the signal to consumers that
    /// shouldn't be able to write — and the conversion path used by
    /// `From<RwSignal<T>> for Signal<T>` to fold an RwSignal into a
    /// `Signal::Dynamic` variant.
    pub fn read_only(self) -> ReadSignal<T> {
        ReadSignal {
            id: self.id,
            _ty: PhantomData,
        }
    }

    /// Project to a write-only handle pointing at the same underlying
    /// value. Mirror of [`read_only`](RwSignal::read_only); useful when
    /// handing the signal to consumers that should only write.
    pub fn write_only(self) -> WriteSignal<T> {
        WriteSignal {
            id: self.id,
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

    /// Non-panicking variant of [`set`]. Returns `true` if the write
    /// happened, `false` if the underlying signal has already been
    /// disposed (e.g. its owner was torn down). Used by callers that
    /// legitimately race owner disposal — the canonical example is
    /// `ElementRef::__unbind`, called from an `on_cleanup` callback
    /// that may fire after the signal's owner has freed its nodes.
    pub fn try_set(self, value: T) -> bool {
        try_write_and_notify(self.id, move |slot| *slot = value, true)
    }

    /// Non-panicking variant of [`update`]. Same semantics as
    /// [`try_set`].
    pub fn try_update(self, f: impl FnOnce(&mut T)) -> bool {
        try_write_and_notify(self.id, f, true)
    }

    /// Whether this signal's backing node is still live in the arena.
    /// `false` once the owner that allocated it has been disposed
    /// (which frees the node). Non-panicking and non-notifying — a
    /// cheap probe for callers that outlive their signal's owner and
    /// must not touch a freed slot (e.g. the animation scheduler
    /// advancing a controller whose owning wrapper was torn down
    /// mid-run). Reading (`get`/`with`) such a signal panics; this does
    /// not.
    pub fn is_disposed(self) -> bool {
        with_runtime(|rt| rt.nodes.get(self.id).is_none())
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

// Conversions: Arc-backed handle → arena-backed `Copy` handle.
//
// Module pattern: stash an [`ArcRwSignal`] in a `OnceLock` so the
// value lives for the process; expose it as [`RwSignal`] /
// [`ReadSignal`] / [`WriteSignal`] so callers get a `Copy` handle
// with the same ergonomics as a component-local signal. The arena
// entry is a lifecycle pin only: when its owner disposes, one Arc
// strong count decrements but every other holder keeps the value
// alive. Reads forward through the Arc, so every handle observes the
// same value and the same subscriber graph.

fn register_arc_in_current_owner<T: 'static>(arc: super::arc_signal::ArcRwSignal<T>) -> NodeId {
    let value: Rc<RefCell<dyn Any>> = Rc::new(RefCell::new(arc));
    let needs_warning = with_runtime(|rt| rt.current_owner().is_none());
    if needs_warning {
        super::warn_no_owner("ArcRwSignal::into::<RwSignal>");
    }
    with_runtime(|rt| {
        let owner = rt.current_owner().unwrap_or_else(|| {
            let detached = rt.owners.insert(Scope::new(None));
            rt.owner_stack.push(detached);
            detached
        });
        let id = rt.nodes.insert(ReactiveNode {
            owner,
            data: NodeData::Signal { value },
            sources: Default::default(),
            subscribers: Default::default(),
            arc_sources: Vec::new(),
        });
        if let Some(o) = rt.owners.get_mut(owner) {
            o.nodes.push(id);
        }
        id
    })
}

impl<T: 'static> From<super::arc_signal::ArcRwSignal<T>> for RwSignal<T> {
    /// Register the Arc-backed signal as a `Copy` arena handle in the
    /// current owner. The arena entry stores the `ArcRwSignal` itself;
    /// `ReadSignal::with` / `WriteSignal::update` downcast it on each
    /// access and forward to the Arc's tracker-aware methods.
    fn from(arc: super::arc_signal::ArcRwSignal<T>) -> Self {
        let id = register_arc_in_current_owner(arc);
        RwSignal {
            id,
            _ty: PhantomData,
        }
    }
}

impl<T: 'static> From<super::arc_signal::ArcReadSignal<T>> for ReadSignal<T> {
    /// Build an arena-backed read handle whose underlying signal is
    /// shared with the source [`ArcReadSignal`]. Writes through any
    /// handle (the Arc, another converted `WriteSignal`, …) propagate
    /// to this handle's subscribers via the shared Arc inner.
    fn from(arc_r: super::arc_signal::ArcReadSignal<T>) -> Self {
        let arc = super::arc_signal::ArcRwSignal { inner: arc_r.inner };
        let id = register_arc_in_current_owner(arc);
        ReadSignal {
            id,
            _ty: PhantomData,
        }
    }
}

impl<T: 'static> From<super::arc_signal::ArcWriteSignal<T>> for WriteSignal<T> {
    /// Build an arena-backed write handle whose underlying signal is
    /// shared with the source [`ArcWriteSignal`].
    fn from(arc_w: super::arc_signal::ArcWriteSignal<T>) -> Self {
        let arc = super::arc_signal::ArcRwSignal { inner: arc_w.inner };
        let id = register_arc_in_current_owner(arc);
        WriteSignal {
            id,
            _ty: PhantomData,
        }
    }
}

// Internal helpers — keep the runtime borrow window narrow.

/// Register the current tracker as an arena subscriber of `id`.
/// Used only for direct-`T` signals (the arc-backed path tracks via
/// [`super::arc_signal::ArcSubscription`] instead).
fn track_node(id: NodeId) {
    with_runtime(|rt| {
        if let Some(tracker) = rt.current_tracker {
            // Avoid self-subscription (a computed reading its own value).
            if tracker != id {
                if let Some(node) = rt.nodes.get_mut(id) {
                    node.subscribers.insert(tracker);
                }
                if let Some(track_node) = rt.nodes.get_mut(tracker) {
                    track_node.sources.insert(id);
                }
            }
        }
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
    let _ = try_write_and_notify(id, f, notify);
}

/// `write_and_notify` variant that returns `false` instead of
/// panicking when the signal is disposed. Used by callers that
/// legitimately may race against owner disposal — most notably the
/// Phase N `ElementRef::__unbind` path, which runs from an
/// `on_cleanup` callback after the underlying RwSignal's owner has
/// already freed its nodes.
fn try_write_and_notify<T: 'static>(id: NodeId, f: impl FnOnce(&mut T), notify: bool) -> bool {
    // Short borrow to pull the Rc handle, then drop before mutating.
    let Some(value) = with_runtime(|rt| rt.nodes.get(id).and_then(|n| n.data.value().cloned()))
    else {
        return false;
    };

    // Arc-backed entries forward to `ArcRwSignal::update` so the
    // change propagates through the shared inner (every other handle,
    // including the original `OnceLock`-stashed Arc, sees it).
    let arc_handle: Option<super::arc_signal::ArcRwSignal<T>> = {
        let borrow = value.borrow();
        borrow
            .downcast_ref::<super::arc_signal::ArcRwSignal<T>>()
            .cloned()
    };
    if let Some(arc) = arc_handle {
        if notify {
            arc.update(f);
        } else {
            arc.update_untracked(f);
        }
        return true;
    }

    // Direct-T storage: mutate under the borrow, then schedule
    // arena subscribers.
    {
        let mut borrow = value.borrow_mut();
        let typed = borrow
            .downcast_mut::<T>()
            .expect("WriteSignal: type mismatch — signal storage corrupted");
        f(typed);
    }

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
    true
}
