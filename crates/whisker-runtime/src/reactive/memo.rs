//! Derived memoised values.
//!
//! [`memo`] is the "effect that caches its return value" primitive.
//! Conceptually it's a compute-driven [`ReadSignal`]: subscribers read
//! through `r.get()` (and friends) exactly the way they would a signal,
//! but the value is produced by the compute closure rather than written
//! externally. The closure re-runs when its tracked sources change,
//! and subscribers are only notified when the recomputed value differs
//! from the cached one (`T: PartialEq`) — the typical "stable identity"
//! optimisation.
//!
//! [`memo`] returns a [`ReadSignal<T>`], not a separate `Memo<T>`
//! type. This is the canonical "readable reactive value" handle in
//! Whisker; component props that expect a dynamic value should take a
//! `ReadSignal<T>` (or `WriteSignal<T>` / `RwSignal<T>` for write
//! capabilities) regardless of whether the source is a primitive
//! signal or a memoised computation. See `docs/reactivity-design.md`
//! for the rationale.

use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use super::runtime::{NodeData, NodeId, Owner, ReactiveNode};
use super::scheduler;
use super::signal::ReadSignal;
use super::with_runtime;

/// Back-compat alias. `memo()` now returns [`ReadSignal<T>`] directly;
/// `Memo<T>` is kept as a type alias so existing code (`fn foo(m:
/// Memo<i32>)`) keeps compiling. New code should write `ReadSignal<T>`.
#[deprecated(
    since = "0.2.0",
    note = "memo() now returns ReadSignal<T>. Use ReadSignal<T> in type signatures instead."
)]
pub type Memo<T> = ReadSignal<T>;

/// Create a memoised computation. `f` is run once immediately to seed
/// the cache, then re-run whenever a tracked source changes.
///
/// The returned [`ReadSignal<T>`] reads the cached value via `.get()`
/// / `.with()` / `.get_untracked()` / `.with_untracked()` — exactly
/// the same surface as a primitive signal. Subscribers (downstream
/// effects, memos, `{expr}` interpolations) are only notified when the
/// recomputed value differs from the previously-cached one
/// (`T: PartialEq`), so a memo whose result is unchanged costs nothing
/// downstream.
///
/// ```ignore
/// let (count, _set) = signal(0_i32);
/// let doubled: ReadSignal<i32> = memo(move || count.get() * 2);
/// assert_eq!(doubled.get(), 0);
/// ```
pub fn memo<T: 'static + Clone + PartialEq>(mut f: impl FnMut() -> T + 'static) -> ReadSignal<T> {
    // We need access to the node id inside the compute closure so it
    // can write back to its own value slot. Allocate the node first
    // with a placeholder value of the right type, then replace the
    // compute with one that knows the id.
    let initial = f();
    let value: Rc<RefCell<dyn Any>> = Rc::new(RefCell::new(initial));

    // The compute closure needs to be set after we know the NodeId,
    // since recomputing must write back into the same value slot and
    // notify subscribers if the new value differs.
    type ComputeCell = Rc<RefCell<Option<Box<dyn FnMut()>>>>;
    let compute_cell: ComputeCell = Rc::new(RefCell::new(None));
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

    let needs_warning = with_runtime(|rt| rt.current_owner().is_none());
    if needs_warning {
        super::warn_no_owner("memo()");
    }
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

    ReadSignal {
        id: node_id,
        _ty: PhantomData,
    }
}
