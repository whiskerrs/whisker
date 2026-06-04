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

/// Safety cap on how many drain iterations [`flush`] will run before
/// logging a warning and bailing out. Each iteration processes one
/// batch of effects scheduled during the previous batch — so a healthy
/// cascade settles in a handful of iterations, and hitting the cap
/// indicates a runaway feedback loop (an effect writing a signal it
/// reads, transitively).
const FLUSH_ITERATION_CAP: usize = 256;

/// Mark `node` as needing a re-run on next flush. Called by signal
/// writes for every subscriber of the written signal.
///
/// Adding a node already in the queue is a no-op — the actual flush
/// dedups by walking the queue and checking the live runtime state
/// (a node may have been disposed between enqueue and flush).
///
/// Pings the host's request-frame callback the first time a new wave
/// of work is scheduled (transition from empty → non-empty pending),
/// so the runtime can wake out of an idle state.
pub(crate) fn schedule(node: NodeId) {
    let was_empty = with_runtime(|rt| {
        let was_empty = rt.pending.is_empty();
        if !rt.pending.contains(&node) {
            rt.pending.push(node);
        }
        was_empty
    });
    // Only nudge the host on the leading edge of a flush wave. While
    // a flush is already in progress (or there's other pending work),
    // the host is either actively running or already poked.
    if was_empty {
        crate::host_wake::wake_runtime();
    }
}

/// Drain the pending queue, re-running effects and computeds in the order
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
    let mut iterations = 0;
    loop {
        let batch: Vec<NodeId> = with_runtime(|rt| std::mem::take(&mut rt.pending));
        if batch.is_empty() {
            break;
        }
        iterations += 1;
        if iterations > FLUSH_ITERATION_CAP {
            // Drop the residual queue so we don't keep spinning, and
            // warn loudly — this almost always indicates an effect
            // that writes a signal it reads (a feedback loop).
            eprintln!(
                "whisker-reactive: flush exceeded {FLUSH_ITERATION_CAP} iterations; \
                 likely an effect with a self-feedback loop. Dropping {} pending nodes.",
                batch.len()
            );
            with_runtime(|rt| rt.pending.clear());
            break;
        }
        for node in batch {
            run_node_if_alive(node);
        }
    }

    with_runtime(|rt| rt.flushing = false);
}

/// Re-run the compute closure for an effect or computed, if it's still
/// alive in the arena. Sets up dependency tracking and owner-stack
/// scoping around the call.
///
/// The compute closure itself is held as `Rc<RefCell<dyn FnMut()>>` so
/// we can clone the handle out of the runtime in a short borrow, drop
/// the borrow, and then invoke the closure — that way the closure
/// body is free to re-enter the runtime to read/write signals.
///
/// **Owner-stack handling**: the effect's owning [`Owner`] is pushed
/// onto `owner_stack` before the closure runs and popped after. This
/// way any reactive primitives or component mounts the closure
/// creates (e.g. children of a `Show` / `For` that re-mount on dep
/// change) become *children of the effect's owner* — same as on the
/// initial run — instead of leaking into whatever owner happened to
/// be current at flush time.
fn run_node_if_alive(node: NodeId) {
    // Step 1: grab the compute handle, clear old sources, set the
    // tracker. Short borrow.
    //
    // `arc_sources` is taken out of the runtime borrow first because
    // its `unsubscribe` callees re-enter the runtime via Arc-signal
    // internals (and even if they don't today, they're free user
    // code that may grow that way). Drop the runtime borrow before
    // iterating.
    let prep = with_runtime(|rt| {
        let n = rt.nodes.get(node)?;
        let owner = n.owner;
        // Paused-owner gate: defer the run and skip. The node lands
        // on `rt.deferred`; `resume_owner` moves it back into
        // `pending` so the effect catches up once its scope is
        // active again. We snapshot the kind early — pure Signal
        // nodes never need the gate (they have no compute), so we
        // only check the flag for Effect / Computed.
        let compute = match &n.data {
            NodeData::Effect { compute } => compute.clone(),
            NodeData::Computed { compute, .. } => compute.clone(),
            NodeData::Signal { .. } => return None,
        };
        let paused = rt.owners.get(owner).map(|o| o.paused).unwrap_or(false);
        if paused {
            if !rt.deferred.contains(&node) {
                rt.deferred.push(node);
            }
            return None;
        }
        // Detach from existing arena sources before re-tracking.
        let sources: Vec<_> = rt.nodes.get(node)?.sources.iter().copied().collect();
        for src in sources {
            if let Some(src_node) = rt.nodes.get_mut(src) {
                src_node.subscribers.remove(&node);
            }
        }
        if let Some(n) = rt.nodes.get_mut(node) {
            n.sources.clear();
        }
        // Take the arc_sources out — we'll call unsubscribe on each
        // outside the runtime borrow.
        let arc_sources = rt
            .nodes
            .get_mut(node)
            .map(|n| std::mem::take(&mut n.arc_sources))
            .unwrap_or_default();
        rt.current_tracker = Some(node);
        rt.owner_stack.push(owner);
        Some((compute, arc_sources))
    });

    let Some((compute, arc_sources)) = prep else {
        return;
    };

    // Step 2: tell each Arc-backed signal "drop me from your subscriber
    // list" so a stale subscription doesn't outlast our last
    // dependency on it. The compute body below will re-register a
    // fresh subscription against every Arc signal it reads.
    for arc_src in arc_sources {
        arc_src.unsubscribe(node);
    }

    // Step 3: invoke compute. The runtime is unborrowed at this
    // point, so user code inside is free to enter `with_runtime`.
    {
        let mut borrow = compute.borrow_mut();
        (*borrow)();
    }

    // Step 4: restore book-keeping — pop the owner we pushed and
    // clear the tracker.
    with_runtime(|rt| {
        rt.owner_stack.pop();
        rt.current_tracker = None;
    });
}
