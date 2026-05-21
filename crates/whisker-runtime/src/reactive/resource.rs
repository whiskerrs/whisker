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

use std::future::Future;

use crate::tasks::spawn_local;

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
/// code reads through the accessors below or via [`Suspense`].
///
/// [`Suspense`]: crate::view::suspense
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
    /// context signal, exposed as a Resource so it can plug into
    /// `Suspense`) can build one without re-spawning a fetcher.
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

/// Fire-and-forget async fetch. Drives `fetcher` (an `async fn` or
/// `async move {…}` block) on Whisker's task pool and writes the
/// resolved [`Result`] into the returned [`Resource`]'s signal.
///
/// `fetcher` is called once on the TASM thread to obtain the
/// `Future`, which is then spawned onto [`crate::tasks::spawn_local`]
/// and polled by every tick. The future runs cooperatively — `await`
/// points yield back to the runtime so the UI stays responsive.
///
/// For blocking sync work inside the fetcher (e.g. `ureq::get(...)`,
/// `std::fs::read(...)`), wrap the call in
/// [`crate::tasks::run_blocking`] which moves it to a worker thread
/// and resumes the awaiting task on the main thread once the result
/// is back.
///
/// Returns immediately with a `Resource<T>` in
/// [`ResourceState::Loading`].
///
/// Owner discipline: the underlying [`RwSignal`] is registered with
/// whatever owner is current at call time. If that owner is disposed
/// before the future completes, the eventual write is a no-op (the
/// signal node is gone), so no stale write hits a re-mounted owner.
///
/// For tests, prefer [`resource_sync`] — it runs the fetcher inline
/// and doesn't depend on the executor having been ticked.
pub fn resource<T, F, Fut>(fetcher: F) -> Resource<T>
where
    T: Clone + 'static,
    F: FnOnce() -> Fut + 'static,
    Fut: Future<Output = Result<T, String>> + 'static,
{
    let state = RwSignal::new(ResourceState::Loading);
    spawn_local(async move {
        let result = fetcher().await;
        state.set(match result {
            Ok(v) => ResourceState::Ready(v),
            Err(e) => ResourceState::Error(e),
        });
    });
    Resource { state }
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
    let state = RwSignal::new(match fetcher() {
        Ok(v) => ResourceState::Ready(v),
        Err(e) => ResourceState::Error(e),
    });
    Resource { state }
}
