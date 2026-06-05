//! Derived (computed) values.
//!
//! [`computed`] is the "effect that caches its return value" primitive.
//! Conceptually it's a compute-driven [`ReadSignal`]: subscribers read
//! through `r.get()` (and friends) exactly the way they would a signal,
//! but the value is produced by the compute closure rather than written
//! externally. The closure re-runs when its tracked sources change,
//! and subscribers are only notified when the recomputed value differs
//! from the cached one (`T: PartialEq`) — the typical "stable identity"
//! optimisation.
//!
//! [`computed`] returns a [`ReadSignal<T>`] directly. This is the
//! canonical "readable reactive value" handle in Whisker; component
//! props that expect a dynamic value should take a `ReadSignal<T>`
//! (or `WriteSignal<T>` / `RwSignal<T>` for write capabilities)
//! regardless of whether the source is a primitive signal or a
//! computed value. See `docs/reactivity-design.md` for the rationale.

use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use super::runtime::{NodeData, NodeId, ReactiveNode, Scope};
use super::scheduler;
use super::signal::ReadSignal;
use super::{untrack, with_runtime};

/// Create a computed (derived) value. `f` is run once immediately to
/// seed the cache, then re-run whenever a tracked source changes.
///
/// The returned [`ReadSignal<T>`] reads the cached value via `.get()`
/// / `.with()` / `.get_untracked()` / `.with_untracked()` — exactly
/// the same surface as a primitive signal. Subscribers (downstream
/// effects, computed values, `{expr}` interpolations) are only
/// notified when the recomputed value differs from the previously-
/// cached one (`T: PartialEq`), so a computed value whose result is
/// unchanged costs nothing downstream.
///
/// ```ignore
/// let (count, _set) = signal(0_i32);
/// let doubled: ReadSignal<i32> = computed(move || count.get() * 2);
/// assert_eq!(doubled.get(), 0);
/// ```
pub fn computed<T: 'static + Clone + PartialEq>(
    mut f: impl FnMut() -> T + 'static,
) -> ReadSignal<T> {
    // Seed inside `untrack` so the read graph it produces doesn't
    // leak into whatever outer reactive node we may be constructed
    // inside. The seed only initialises the cache; real dependency
    // edges are registered by the explicit `schedule + flush` below.
    // Without this guard, calling `computed(move || sig.get())` from
    // inside an effect makes that effect a subscriber of `sig` — and
    // a write to `sig` then re-runs the outer effect (often a route /
    // component mount), silently leaking a fresh computed node every
    // tick.
    let initial = untrack(&mut f);
    let value: Rc<RefCell<dyn Any>> = Rc::new(RefCell::new(initial));

    // Compute closure is set after we know the NodeId, so recomputes
    // can write back into the same slot and notify subscribers on
    // change.
    type ComputeCell = Rc<RefCell<Option<Box<dyn FnMut()>>>>;
    let compute_cell: ComputeCell = Rc::new(RefCell::new(None));
    let compute_cell_clone = compute_cell.clone();
    let trampoline: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(move || {
        // Take/run/put-back so we never hold the cell's borrow across
        // user code — mirrors the scheduler's pattern for the compute Rc.
        let mut taken = compute_cell_clone.borrow_mut().take();
        if let Some(ref mut inner) = taken {
            inner();
        }
        *compute_cell_clone.borrow_mut() = taken;
    }));

    let needs_warning = with_runtime(|rt| rt.current_owner().is_none());
    if needs_warning {
        super::warn_no_owner("computed()");
    }
    let node_id = with_runtime(|rt| {
        let owner = rt.current_owner().unwrap_or_else(|| {
            let detached = rt.owners.insert(Scope::new(None));
            rt.owner_stack.push(detached);
            detached
        });
        let id = rt.nodes.insert(ReactiveNode {
            owner,
            data: NodeData::Computed {
                value: value.clone(),
                compute: trampoline,
            },
            sources: Default::default(),
            subscribers: Default::default(),
            arc_sources: Vec::new(),
        });
        if let Some(o) = rt.owners.get_mut(owner) {
            o.nodes.push(id);
        }
        id
    });

    // Real compute closure: knows the node id, writes back to the
    // value slot, notifies subscribers on change.
    let value_clone = value.clone();
    *compute_cell.borrow_mut() = Some(Box::new(move || {
        let new = f();
        let changed = {
            let borrow = value_clone.borrow();
            let old: &T = borrow
                .downcast_ref::<T>()
                .expect("computed: type mismatch on recompute");
            old != &new
        };
        if changed {
            {
                let mut borrow = value_clone.borrow_mut();
                let slot = borrow
                    .downcast_mut::<T>()
                    .expect("computed: type mismatch on write-back");
                *slot = new;
            }
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

    // First tracked run: populates the source graph. The initial
    // value was already cached via the untracked seed above; we
    // explicitly schedule + flush here because a bare `flush()` over
    // an empty pending queue would miss tracking.
    scheduler::schedule(node_id);
    scheduler::flush();

    ReadSignal {
        id: node_id,
        _ty: PhantomData,
    }
}
