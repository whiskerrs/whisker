//! Side-effecting reactive computation.
//!
//! `effect(f)` registers `f` against the current owner, runs it once
//! immediately (recording every signal it reads as a dependency), and
//! arranges for `f` to be re-run whenever any of those dependencies
//! changes.
//!
//! The first run is synchronous (no batching) so the caller can rely
//! on the effect having executed by the time `effect` returns. Later
//! runs are scheduled through the [scheduler](super::scheduler) and
//! drained at flush time.

use std::cell::RefCell;
use std::rc::Rc;

use super::runtime::{NodeData, NodeId, Owner, ReactiveNode};
use super::scheduler;
use super::with_runtime;

/// Register `f` as a reactive effect. Returns the node id (so tests
/// can inspect it; user code generally discards the return value).
///
/// The closure receives no arguments and returns nothing. The
/// `Option<R>`-of-previous-value variant from Solid/Leptos is omitted
/// in v1 — it can be layered on later without breaking this API.
pub fn effect(mut f: impl FnMut() + 'static) -> NodeId {
    // Allocate the node first (with a placeholder compute we replace
    // before running) so the closure can see its own id if it ever
    // needs to. The Rc<RefCell<...>> wrapper lets the scheduler clone
    // a handle out of the runtime in a short borrow.
    let compute: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(move || f()));

    let node_id = with_runtime(|rt| {
        let owner = rt.current_owner().unwrap_or_else(|| {
            let detached = rt.owners.insert(Owner::new(None));
            rt.owner_stack.push(detached);
            detached
        });
        let id = rt.nodes.insert(ReactiveNode {
            owner,
            data: NodeData::Effect {
                compute: compute.clone(),
            },
            sources: Default::default(),
            subscribers: Default::default(),
        });
        if let Some(o) = rt.owners.get_mut(owner) {
            o.nodes.push(id);
        }
        id
    });

    // Run it immediately so dependencies are recorded and the side
    // effect runs by the time `effect()` returns.
    scheduler::schedule(node_id);
    scheduler::flush();

    node_id
}
