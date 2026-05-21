//! Async-data primitive — fetches a value off the main thread,
//! marshals the result back through [`run_on_main_thread`], and
//! exposes the loading / ready / error state through a
//! [`ReadSignal`]-shaped handle.
//!
//! Mirrors the "fetch on worker, dispatch back to TASM thread" pattern
//! every Whisker app needs for HTTP / disk / DB calls — see
//! `examples/hn-reader` for a real use case. The primitive replaces
//! the hand-rolled `signal + thread::spawn + run_on_main_thread`
//! boilerplate apps were carrying.
//!
//! Pair with [`crate::view::suspense`] to render `fallback` while a
//! resource is loading and switch to its `children` view once the
//! value lands.
//!
//! ```ignore
//! let stories = resource(|| fetch_top_stories());
//! render! {
//!     Suspense(
//!         resource: stories,
//!         fallback: || render! { text(value: "Loading…") },
//!         children: |list: Vec<Story>| render! { /* render list */ },
//!     )
//! }
//! ```

use crate::main_thread::run_on_main_thread;

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

/// Fire-and-forget async fetch. Spawns a worker thread that runs
/// `fetcher`, then marshals the result back to the main thread via
/// [`run_on_main_thread`] and writes it into the returned
/// [`Resource`]'s underlying signal.
///
/// `fetcher` may block (HTTP, disk, etc.) — that's the whole point of
/// running it off the main thread. Returns immediately with a
/// `Resource<T>` in the [`ResourceState::Loading`] state.
///
/// Owner discipline: the underlying [`RwSignal`] is registered with
/// whatever owner is current at call time. If that owner is disposed
/// before the worker finishes, the eventual main-thread write is a
/// no-op (the signal node is gone), so no stale write hits a re-mounted
/// owner.
///
/// For tests, prefer [`resource_sync`] — it runs the fetcher inline
/// and doesn't require an active main-thread dispatcher.
pub fn resource<T, F>(fetcher: F) -> Resource<T>
where
    T: Clone + Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let state = RwSignal::new(ResourceState::Loading);
    std::thread::spawn(move || {
        let result = fetcher();
        run_on_main_thread(move || {
            state.set(match result {
                Ok(v) => ResourceState::Ready(v),
                Err(e) => ResourceState::Error(e),
            });
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
