//! Async-data primitive — runs an `async` fetcher on Whisker's
//! single-threaded task pool ([`crate::tasks`]) and exposes the
//! loading / ready / error state through a [`ReadSignal`]-shaped
//! handle.
//!
//! The fetcher runs on the TASM thread under
//! [`futures_executor::LocalPool`]. For blocking sync IO (`ureq`,
//! `std::fs`, …) inside the fetcher, wrap the call in
//! [`crate::tasks::run_blocking`] which offloads to a fresh worker
//! thread and marshals the result back via [`run_on_main_thread`]:
//!
//! ```ignore
//! use whisker::runtime::tasks::run_blocking;
//!
//! let stories = resource(|| async {
//!     run_blocking(|| {
//!         ureq::get("https://hn.algolia.com/...")
//!             .call()
//!             .map_err(|e| e.to_string())?
//!             .into_string()
//!             .map_err(|e| e.to_string())
//!     })
//!     .await
//!     .and_then(|body| parse(&body))
//! });
//! ```
//!
//! For purely-async fetchers (a non-blocking HTTP client, a
//! pre-computed value, etc.) you can just write `async move { ... }`
//! and skip the `run_blocking` step.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use crate::tasks::spawn_local;

use super::runtime::NodeId;
use super::signal::RwSignal;

/// Three-state machine the [`Resource`] cycles through. `Clone` so
/// reads inside effects can take owned copies without borrowing the
/// underlying signal slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceState<T> {
    /// Worker hasn't returned yet — neither value nor error available.
    Loading,
    /// Worker returned `Ok(v)` — `v` is the fetched value.
    Ready(T),
    /// Worker returned `Err(msg)`. The string is the user-readable
    /// reason. (Plain `String` rather than a generic `E` keeps the
    /// type parameter count low and matches the common pattern of
    /// stringifying upstream errors with `.map_err(|e| e.to_string())`.)
    Error(String),
}

impl<T> ResourceState<T> {
    pub fn is_loading(&self) -> bool {
        matches!(self, ResourceState::Loading)
    }
    pub fn is_ready(&self) -> bool {
        matches!(self, ResourceState::Ready(_))
    }
    pub fn is_error(&self) -> bool {
        matches!(self, ResourceState::Error(_))
    }
}

/// Copy handle to a deferred value. Wraps an [`RwSignal`] whose slot
/// the worker thread writes into once the fetch completes; consumer
/// code reads through the accessors below.
pub struct Resource<T: Clone + 'static> {
    state: RwSignal<ResourceState<T>>,
}

// Hand-written Copy/Clone — `derive(Copy)` would require `T: Copy`
// which is unnecessarily strict (the resource only holds a u32-ish
// signal handle, not the T itself).
impl<T: Clone + 'static> Copy for Resource<T> {}
impl<T: Clone + 'static> Clone for Resource<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Clone + 'static> Resource<T> {
    /// Construct a `Resource<T>` backed by an externally-owned
    /// [`RwSignal`]. The signal becomes the resource's source of
    /// truth — writes to it surface as state transitions through
    /// the resource's accessors.
    ///
    /// Hidden from rustdoc: regular users go through [`resource`] or
    /// [`resource_sync`]. This is here so tests + non-standard
    /// "synthetic resource" cases (e.g. a value derived from a
    /// context signal, exposed as a Resource) can build one without
    /// re-spawning a fetcher.
    #[doc(hidden)]
    pub fn from_state(state: RwSignal<ResourceState<T>>) -> Self {
        Self { state }
    }

    /// Read the current state (reactive — registers a dependency on
    /// the underlying signal).
    pub fn state(&self) -> ResourceState<T> {
        self.state.get()
    }

    /// Convenience: return `Some(value)` when ready, `None` otherwise.
    pub fn get(&self) -> Option<T> {
        match self.state.get() {
            ResourceState::Ready(v) => Some(v),
            _ => None,
        }
    }

    /// Convenience: `true` while the worker is still running.
    pub fn loading(&self) -> bool {
        matches!(self.state.get(), ResourceState::Loading)
    }

    /// Convenience: return `Some(message)` if the fetch ended in error.
    pub fn error(&self) -> Option<String> {
        match self.state.get() {
            ResourceState::Error(e) => Some(e),
            _ => None,
        }
    }
}

/// Reactive async fetch. Drives `fetcher` (an `async fn` or
/// `async move {…}` block) on Whisker's task pool and writes the
/// resolved [`Result`] into the returned [`Resource`]'s signal — then
/// **re-runs the fetcher whenever any signal it read changes**.
///
/// Reactivity: the fetcher is wrapped in a reactive [`effect`] and the
/// spawned future re-installs that effect node as the current observer
/// on every `poll`. As a result, signals read **anywhere** in the
/// fetcher are tracked as dependencies of the resource — both in the
/// synchronous prefix (before the first `.await`) and after any
/// `.await` point. When any tracked signal changes, the fetcher runs
/// again from scratch and the resource updates.
///
/// While a (re)fetch is in flight the resource returns to
/// [`ResourceState::Loading`]. Only the latest run's result is
/// committed: a monotonically-increasing generation counter guards the
/// write, so if a newer run starts before an older in-flight fetch
/// resolves, the stale result is discarded rather than clobbering the
/// fresh state. In-flight stale fetches are abandoned *cooperatively*
/// (the superseded future stops at its next `poll` boundary) — there is
/// no hard cancellation, and any worker thread spawned via
/// [`crate::tasks::run_blocking`] runs to completion with its result
/// dropped.
///
/// Dynamic-dependency caveat: dependencies are rebuilt on every run, so
/// a signal that is only read on *some* code path is only a dependency
/// on the runs where that path actually executes. A signal read after
/// an `.await` is only tracked once the future advances past that
/// suspension point.
///
/// For blocking sync work inside the fetcher (e.g. `ureq::get(...)`,
/// `std::fs::read(...)`), wrap the call in
/// [`crate::tasks::run_blocking`] which moves it to a worker thread
/// and resumes the awaiting task on the main thread once the result
/// is back.
///
/// Returns immediately with a `Resource<T>` in
/// [`ResourceState::Loading`]; the first fetch is spawned during the
/// effect's synchronous initial run.
///
/// Owner discipline: the underlying [`RwSignal`] and the driving effect
/// are registered with whatever owner is current at call time. If that
/// owner is disposed, the effect stops re-running and any eventual
/// write is a no-op (the signal node is gone), so no stale write hits a
/// re-mounted owner.
///
/// For tests / already-in-memory values, prefer [`resource_sync`] — it
/// runs the fetcher inline once, untracked, and doesn't depend on the
/// executor having been ticked.
pub fn resource<T, F, Fut>(fetcher: F) -> Resource<T>
where
    T: Clone + 'static,
    F: Fn() -> Fut + 'static,
    Fut: Future<Output = Result<T, String>> + 'static,
{
    let state = RwSignal::new(ResourceState::Loading);
    // Monotonic run counter. Each effect run bumps it and stamps its
    // spawned future; the future only commits its result if the
    // counter still matches at completion time (generation guard).
    let generation = Rc::new(Cell::new(0u64));
    let fetcher = Rc::new(fetcher);

    super::effect::effect(move || {
        // Inside an effect run the runtime's `current_tracker` IS this
        // effect's node. Capture it so the spawned future can re-install
        // it as the observer around each poll (so post-`.await` reads
        // register as deps of this node too).
        let node = super::current_tracker().expect("resource effect must run under a tracker");

        let my_gen = generation.get().wrapping_add(1);
        generation.set(my_gen);

        // Build the future. The fetcher's SYNCHRONOUS prefix runs here,
        // and because we're inside the effect run, its signal reads
        // register as dependencies of `node`.
        let fut = (fetcher)();

        // Return to Loading on every (re)fetch. Write untracked so this
        // never creates a dependency edge — the effect must not depend
        // on `state` (the signal it writes) or it would re-trigger
        // itself in an infinite loop.
        state.update_untracked(|s| *s = ResourceState::Loading);

        spawn_local(ScopedFetch {
            node,
            my_gen,
            generation: generation.clone(),
            state,
            fut: Box::pin(fut),
        });
    });

    Resource { state }
}

/// A spawned fetch future that re-installs its resource's effect node
/// as the current reactive observer on every `poll`, so signal reads
/// after `.await` points are tracked as dependencies of the resource.
/// A generation stamp lets a superseded run abandon itself
/// cooperatively without clobbering a fresher result.
struct ScopedFetch<T: Clone + 'static> {
    /// The driving effect's node — re-installed as the observer per poll.
    node: NodeId,
    /// This run's generation stamp.
    my_gen: u64,
    /// Shared run counter; if it has moved past `my_gen` we're stale.
    generation: Rc<Cell<u64>>,
    /// Resource state slot to commit into.
    state: RwSignal<ResourceState<T>>,
    /// The fetcher's future (single-threaded / `!Send` is fine here).
    fut: Pin<Box<dyn Future<Output = Result<T, String>>>>,
}

impl<T: Clone + 'static> Future for ScopedFetch<T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // Destructure through the pin so the borrow of `fut` inside the
        // `with_observer` closure doesn't collide with reads of the
        // other fields.
        let this = self.get_mut();

        // A newer run superseded us: abandon WITHOUT installing the
        // observer (add no stale dependency edges) and WITHOUT writing
        // state. The inner future is dropped with this `ScopedFetch`.
        if this.generation.get() != this.my_gen {
            return Poll::Ready(());
        }

        let node = this.node;
        let fut = &mut this.fut;
        // Re-install the resource's effect node as the current observer
        // for THIS poll so reads after `.await` register as deps of it.
        let poll = super::with_observer(node, || fut.as_mut().poll(cx));

        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                // Commit only if we're still the latest run.
                if this.generation.get() == this.my_gen {
                    this.state.set(match result {
                        Ok(v) => ResourceState::Ready(v),
                        Err(e) => ResourceState::Error(e),
                    });
                }
                Poll::Ready(())
            }
        }
    }
}

/// Synchronous-fetch variant. Runs `fetcher` inline on the calling
/// thread and writes the result directly into the resource's signal.
/// No worker thread, no main-thread dispatcher needed — useful for
/// tests, for cases where the value is already in memory, and for
/// computed pseudo-resources (e.g. derive from a context value).
///
/// The returned `Resource` is in [`ResourceState::Ready`] or
/// [`ResourceState::Error`] *immediately* — never `Loading`.
pub fn resource_sync<T, F>(fetcher: F) -> Resource<T>
where
    T: Clone + 'static,
    F: FnOnce() -> Result<T, String>,
{
    // `fetcher` is a one-shot seed for the resource's RwSignal —
    // its signal reads are meant to compute an initial value, not to
    // re-fire the resource when those signals change. Run it under
    // `untrack` so the reads don't leak into whatever outer effect
    // / computed / component body happens to be calling
    // `resource_sync`. Same principle as the computed seed guard.
    let state = RwSignal::new(match super::untrack(fetcher) {
        Ok(v) => ResourceState::Ready(v),
        Err(e) => ResourceState::Error(e),
    });
    Resource { state }
}
