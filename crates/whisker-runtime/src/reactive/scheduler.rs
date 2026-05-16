//! Batching / flush of pending effect re-runs.
//!
//! Signal writes don't run subscribers immediately. They append the
//! subscribers to the runtime's `pending` queue. The queue is drained
//! by [`flush`] — explicitly, at the end of an event handler, or
//! implicitly when the runtime detects it should (see
//! `request_flush`).
//!
//! ## Why batch
//!
//! Several writes in the same logical transaction (typical of an
//! event handler that updates 3 signals) should produce **one** wave
//! of effect runs, not three. Solid / Leptos call this "microtask
//! batching".

use super::runtime::{NodeData, NodeId};
use super::with_runtime;

/// Mark `node` as needing a re-run on next flush. Called by signal
/// writes for every subscriber of the written signal.
///
/// Adding a node already in the queue is a no-op — the actual flush
/// dedups by walking the queue and checking the live runtime state
/// (a node may have been disposed between enqueue and flush).
pub(crate) fn schedule(node: NodeId) {
    with_runtime(|rt| {
        if !rt.pending.contains(&node) {
            rt.pending.push(node);
        }
    });
}

/// Drain the pending queue, re-running effects and memos in the order
/// they were scheduled. Skips entries whose node has been disposed.
///
/// Reentrant: a re-running effect may itself write signals, which
/// adds more nodes to the queue. `flush` keeps draining until the
/// queue is empty.
///
/// No-op if a flush is already in progress on this thread (signals
/// written during a flush just append, and the outer flush keeps
/// going).
pub fn flush() {
    let already_flushing = with_runtime(|rt| {
        let was = rt.flushing;
        if !was {
            rt.flushing = true;
        }
        was
    });
    if already_flushing {
        return;
    }

    // Drain loop. We `take` the current queue so signals written
    // during a re-run land in a fresh queue and don't perturb the
    // ordering of the current wave.
    loop {
        let batch: Vec<NodeId> = with_runtime(|rt| std::mem::take(&mut rt.pending));
        if batch.is_empty() {
            break;
        }
        for node in batch {
            run_node_if_alive(node);
        }
    }

    with_runtime(|rt| rt.flushing = false);
}

/// Re-run the compute closure for an effect or memo, if it's still
/// alive in the arena. Sets up dependency tracking around the call.
///
/// The compute closure itself is held as `Rc<RefCell<dyn FnMut()>>` so
/// we can clone the handle out of the runtime in a short borrow, drop
/// the borrow, and then invoke the closure — that way the closure
/// body is free to re-enter the runtime to read/write signals.
fn run_node_if_alive(node: NodeId) {
    // Step 1: grab the compute handle, clear old sources, set the
    // tracker. Short borrow.
    let compute = with_runtime(|rt| {
        let n = rt.nodes.get(node)?;
        let compute = match &n.data {
            NodeData::Effect { compute } => compute.clone(),
            NodeData::Memo { compute, .. } => compute.clone(),
            NodeData::Signal { .. } => return None,
        };
        // Detach from existing sources before re-tracking.
        let sources: Vec<_> = rt
            .nodes
            .get(node)?
            .sources
            .iter()
            .copied()
            .collect();
        for src in sources {
            if let Some(src_node) = rt.nodes.get_mut(src) {
                src_node.subscribers.remove(&node);
            }
        }
        if let Some(n) = rt.nodes.get_mut(node) {
            n.sources.clear();
        }
        rt.current_tracker = Some(node);
        Some(compute)
    });

    let Some(compute) = compute else { return };

    // Step 2: invoke compute. The runtime is unborrowed at this
    // point, so user code inside is free to enter `with_runtime`.
    {
        let mut borrow = compute.borrow_mut();
        (&mut *borrow)();
    }

    // Step 3: clear the tracker.
    with_runtime(|rt| {
        rt.current_tracker = None;
    });
}
