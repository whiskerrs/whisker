//! Derived memoised values.
//!
//! [`memo`] is the "effect that caches its return value" primitive.
//! Other reactive nodes can read a memo just like a signal — calling
//! `m.get()` registers them as subscribers, and writes to the memo's
//! sources trigger re-computation, which (if the value changed)
//! notifies the memo's subscribers in turn.
//!
//! Equality check: a memo only marks subscribers dirty when its new
//! return value differs from the cached one (`T: PartialEq`). This is
//! the typical "stable identity" optimisation — re-running upstream
//! work shouldn't cascade further unless the user-observable result
//! actually changed.

use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use super::runtime::{NodeData, NodeId, Owner, ReactiveNode};
use super::scheduler;
use super::with_runtime;

/// Read-only handle to a memoised value. `Copy`; works like a
/// `ReadSignal<T>` in every observable way.
pub struct Memo<T: 'static> {
    id: NodeId,
    _ty: PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for Memo<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for Memo<T> {}

impl<T: 'static> Memo<T> {
    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        let value = track_and_fetch(self.id);
        let borrow = value.borrow();
        let typed = borrow
            .downcast_ref::<T>()
            .expect("Memo::with: type mismatch — memo storage corrupted");
        f(typed)
    }

    pub fn with_untracked<R>(self, f: impl FnOnce(&T) -> R) -> R {
        let value = fetch_value(self.id);
        let borrow = value.borrow();
        let typed = borrow
            .downcast_ref::<T>()
            .expect("Memo::with_untracked: type mismatch — memo storage corrupted");
        f(typed)
    }
}

impl<T: 'static + Clone> Memo<T> {
    pub fn get(self) -> T {
        self.with(|v| v.clone())
    }

    pub fn get_untracked(self) -> T {
        self.with_untracked(|v| v.clone())
    }
}

/// Create a memoised computation. `f` is run once immediately to seed
/// the cache, then re-run whenever a tracked source changes. Memo
/// subscribers are only notified when the recomputed value differs
/// from the cached one (`T: PartialEq`).
pub fn memo<T: 'static + Clone + PartialEq>(mut f: impl FnMut() -> T + 'static) -> Memo<T> {
    // We need access to the node id inside the compute closure so it
    // can write back to its own value slot. Allocate the node first
    // with a placeholder value of the right type, then replace the
    // compute with one that knows the id.
    let initial = f();
    let value: Rc<RefCell<dyn Any>> = Rc::new(RefCell::new(initial));

    // The compute closure needs to be set after we know the NodeId,
    // since recomputing must write back into the same value slot and
    // notify subscribers if the new value differs.
    let compute_cell: Rc<RefCell<Option<Box<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let compute_cell_clone = compute_cell.clone();
    let trampoline: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(move || {
        // Take ownership of the inner closure, call it, put it back.
        // Mirrors the scheduler's pattern for the compute Rc itself —
        // we never hold the cell's borrow across the user closure.
        let mut taken = compute_cell_clone.borrow_mut().take();
        if let Some(ref mut inner) = taken {
            inner();
        }
        *compute_cell_clone.borrow_mut() = taken;
    }));

    let node_id = with_runtime(|rt| {
        let owner = rt.current_owner().unwrap_or_else(|| {
            let detached = rt.owners.insert(Owner::new(None));
            rt.owner_stack.push(detached);
            detached
        });
        let id = rt.nodes.insert(ReactiveNode {
            owner,
            data: NodeData::Memo {
                value: value.clone(),
                compute: trampoline,
            },
            sources: Default::default(),
            subscribers: Default::default(),
        });
        if let Some(o) = rt.owners.get_mut(owner) {
            o.nodes.push(id);
        }
        id
    });

    // Now we can build the actual compute closure that knows the node
    // id and can notify subscribers on value changes.
    let value_clone = value.clone();
    *compute_cell.borrow_mut() = Some(Box::new(move || {
        let new = f();
        let changed = {
            let borrow = value_clone.borrow();
            let old: &T = borrow
                .downcast_ref::<T>()
                .expect("Memo: type mismatch on recompute");
            old != &new
        };
        if changed {
            {
                let mut borrow = value_clone.borrow_mut();
                let slot = borrow
                    .downcast_mut::<T>()
                    .expect("Memo: type mismatch on write-back");
                *slot = new;
            }
            // Notify subscribers.
            let subscribers: Vec<NodeId> = with_runtime(|rt| {
                rt.nodes
                    .get(node_id)
                    .map(|n| n.subscribers.iter().copied().collect())
                    .unwrap_or_default()
            });
            for sub in subscribers {
                scheduler::schedule(sub);
            }
        }
    }));

    // First run: register dependencies. We've already computed the
    // initial value above, but we need a tracked run so the source
    // graph is populated. Triggering a flush walks the pending queue
    // which is currently empty, so we'd miss the tracking — instead
    // we explicitly schedule + flush, which the scheduler will treat
    // as the first run.
    scheduler::schedule(node_id);
    scheduler::flush();

    Memo {
        id: node_id,
        _ty: PhantomData,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers — duplicated from signal.rs for now; refactor when the
// API stabilises.
// ---------------------------------------------------------------------------

fn track_and_fetch(id: NodeId) -> Rc<RefCell<dyn Any>> {
    with_runtime(|rt| {
        if let Some(tracker) = rt.current_tracker {
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
            .expect("Memo: node disposed or not a value-bearing node")
    })
}

fn fetch_value(id: NodeId) -> Rc<RefCell<dyn Any>> {
    with_runtime(|rt| {
        rt.nodes
            .get(id)
            .and_then(|n| n.data.value().cloned())
            .expect("Memo: node disposed or not a value-bearing node")
    })
}
