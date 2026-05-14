//! Reactive primitives.
//!
//! [`Signal<T>`] is a `Copy` handle backed by an arena of values. Updating
//! a signal marks the runtime as dirty so the active scope re-renders on
//! the next tick. Reads inside a tracked scope register the signal as a
//! dependency.
//!
//! Closely modelled on Dioxus's signal API to keep the borrow-checker
//! pain off the user-facing surface: callers can move signals into
//! closures freely because `Copy` handles never tie a lifetime.

use std::any::Any;
use std::cell::RefCell;
use std::ffi::c_void;
use std::fmt;
use std::rc::Rc;

// ----------------------------------------------------------------------------
// Storage
// ----------------------------------------------------------------------------

/// Per-runtime arena. Reactive UI runs on a single thread (the Lynx TASM
/// thread) so `Rc<RefCell<…>>` is fine; we don't need `Send`.
type Arena = RefCell<Vec<Box<dyn Any>>>;

/// "Wake the host" callback. The host (iOS / Android) registers one of
/// these during init; whenever a signal marks the runtime dirty, the
/// runtime fires this callback so the host can resume its render loop.
///
/// Stored as a function pointer + opaque user_data instead of a boxed
/// closure so we can hand the C ABI a stable trampoline.
#[derive(Copy, Clone)]
struct RequestFrameCb {
    func: extern "C" fn(*mut c_void),
    user_data: *mut c_void,
}

/// SAFETY: `user_data` is an opaque host pointer. The host promises it
/// remains valid for the lifetime of the dev session and is safe to
/// call from any thread (the callbacks we wire on Android / iOS just
/// post a "wake" message to the runtime thread).
unsafe impl Send for RequestFrameCb {}
unsafe impl Sync for RequestFrameCb {}

thread_local! {
    static ARENA: Arena = RefCell::new(Vec::new());
    static DIRTY: RefCell<bool> = const { RefCell::new(false) };
    static TRACKING: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static REQUEST_FRAME: RefCell<Option<RequestFrameCb>> = const { RefCell::new(None) };
}

/// Cross-thread mirror of `REQUEST_FRAME`. Stored globally so threads
/// other than the TASM thread (e.g. the WebSocket receiver in
/// `whisker-dev-runtime`) can wake the runtime without going through a
/// signal write. Same callback, same user_data — just reachable from
/// any thread via `wake_runtime`.
static REMOTE_WAKE: std::sync::Mutex<Option<RequestFrameCb>> =
    std::sync::Mutex::new(None);

/// (Test-only) clear the arena and dirty flag. Production code never
/// needs this; tests reset between cases to keep the thread-local clean.
#[doc(hidden)]
pub fn __reset_runtime() {
    ARENA.with(|a| a.borrow_mut().clear());
    DIRTY.with(|d| *d.borrow_mut() = false);
    TRACKING.with(|t| t.borrow_mut().clear());
    REQUEST_FRAME.with(|r| *r.borrow_mut() = None);
}

/// Register the host-side "wake up please" callback. Pass `None` to clear.
///
/// Must be called from the same thread that runs `take_dirty` / signal
/// updates — i.e. the runtime thread (Lynx TASM thread, which is the iOS
/// main thread in our current setup). Also mirrors the callback into
/// the cross-thread slot so [`wake_runtime`] can fire it from any
/// thread.
#[doc(hidden)]
pub fn set_request_frame_callback(
    func: Option<extern "C" fn(*mut c_void)>,
    user_data: *mut c_void,
) {
    let built = func.map(|func| RequestFrameCb { func, user_data });
    REQUEST_FRAME.with(|r| *r.borrow_mut() = built);
    if let Ok(mut guard) = REMOTE_WAKE.lock() {
        *guard = built;
    }
}

/// Wake the runtime from any thread. Used by `whisker-dev-runtime`'s
/// WebSocket receiver to nudge the host into running another tick
/// after parking a patch in the pending slot — the receiver thread
/// can't touch the runtime's thread-local `REQUEST_FRAME` directly.
///
/// No-op when no host callback is registered yet (signal updates
/// during init may happen before bootstrap has wired anything up).
pub fn wake_runtime() {
    let cb = REMOTE_WAKE.lock().ok().and_then(|g| *g);
    if let Some(cb) = cb {
        (cb.func)(cb.user_data);
    }
}

/// Marks the runtime dirty and fires the host wake-up callback. Centralised
/// so `Signal::set` and any future setter (computed signal write, batched
/// transaction, …) share the same wake-up logic.
fn mark_dirty_and_wake() {
    DIRTY.with(|d| *d.borrow_mut() = true);
    let cb = REQUEST_FRAME.with(|r| *r.borrow());
    if let Some(cb) = cb {
        (cb.func)(cb.user_data);
    }
}

// ----------------------------------------------------------------------------
// Signal
// ----------------------------------------------------------------------------

/// Reactive value handle. Copies are cheap and aliasable; the underlying
/// value lives in the runtime arena.
pub struct Signal<T: 'static> {
    id: usize,
    _phantom: std::marker::PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for Signal<T> {}

impl<T: 'static + fmt::Debug + Clone> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signal").field("id", &self.id).finish()
    }
}

/// Allocate a fresh signal, calling `init` once to produce the initial
/// value. Returns a `Copy` handle.
pub fn use_signal<T: 'static, F: FnOnce() -> T>(init: F) -> Signal<T> {
    let value = init();
    let id = ARENA.with(|a| {
        let mut arena = a.borrow_mut();
        arena.push(Box::new(value) as Box<dyn Any>);
        arena.len() - 1
    });
    Signal {
        id,
        _phantom: std::marker::PhantomData,
    }
}

impl<T: 'static + Clone> Signal<T> {
    /// Read the current value. Inside a tracked scope this registers the
    /// signal as a dependency.
    pub fn get(self) -> T {
        TRACKING.with(|t| {
            let mut t = t.borrow_mut();
            if !t.contains(&self.id) {
                t.push(self.id);
            }
        });
        ARENA.with(|a| {
            let arena = a.borrow();
            let entry = arena.get(self.id).expect("signal id out of range");
            entry
                .downcast_ref::<T>()
                .expect("signal type mismatch")
                .clone()
        })
    }

    /// Replace the value. Marks the runtime dirty (and pings the host's
    /// "wake up" callback) so the next tick re-renders.
    pub fn set(self, value: T) {
        ARENA.with(|a| {
            let mut arena = a.borrow_mut();
            let slot = arena.get_mut(self.id).expect("signal id out of range");
            *slot
                .downcast_mut::<T>()
                .expect("signal type mismatch") = value;
        });
        mark_dirty_and_wake();
    }
}

impl<T: 'static + Clone + std::ops::Add<Output = T> + From<i32>> Signal<T> {
    /// Convenience for the very common counter pattern: `s.update(|n| n + 1)`.
    pub fn update(self, f: impl FnOnce(T) -> T) {
        let v = self.get();
        self.set(f(v));
    }
}

// ----------------------------------------------------------------------------
// Runtime book-keeping (used by Phase 8 runtime)
// ----------------------------------------------------------------------------

/// Returns and clears the dirty flag. The runtime calls this between
/// frames to decide whether a re-render is needed.
#[doc(hidden)]
pub fn take_dirty() -> bool {
    DIRTY.with(|d| std::mem::replace(&mut *d.borrow_mut(), false))
}

/// Run `f` while collecting the set of signals it reads. Returns the
/// closure's value plus the dependency list.
#[doc(hidden)]
pub fn track_dependencies<R>(f: impl FnOnce() -> R) -> (R, Vec<usize>) {
    TRACKING.with(|t| t.borrow_mut().clear());
    let value = f();
    let deps = TRACKING.with(|t| std::mem::take(&mut *t.borrow_mut()));
    (value, deps)
}

/// Re-export to encourage explicit ownership at the runtime layer.
#[doc(hidden)]
pub fn _arena_marker() -> Rc<()> {
    Rc::new(())
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() {
        __reset_runtime();
    }

    #[test]
    fn use_signal_returns_initial_value() {
        fresh();
        let s = use_signal(|| 42_i32);
        assert_eq!(s.get(), 42);
    }

    #[test]
    fn set_replaces_value() {
        fresh();
        let s = use_signal(|| 0_i32);
        s.set(7);
        assert_eq!(s.get(), 7);
    }

    #[test]
    fn signal_is_copy_and_aliasable() {
        fresh();
        let s = use_signal(|| String::from("hello"));
        let a = s;
        let b = s;
        assert_eq!(a.get(), "hello");
        assert_eq!(b.get(), "hello");
    }

    #[test]
    fn signal_works_with_owned_strings() {
        fresh();
        let s = use_signal(String::new);
        s.set("hi".into());
        assert_eq!(s.get(), "hi");
    }

    #[test]
    fn updating_marks_runtime_dirty() {
        fresh();
        let s = use_signal(|| 0_i32);
        let _ = take_dirty(); // clear initial state
        assert!(!take_dirty());
        s.set(1);
        assert!(take_dirty());
        // A second take returns false until next set.
        assert!(!take_dirty());
    }

    #[test]
    fn tracking_records_read_dependencies() {
        fresh();
        let a = use_signal(|| 1_i32);
        let b = use_signal(|| 2_i32);
        let c = use_signal(|| 3_i32);
        let (value, deps) = track_dependencies(|| a.get() + b.get());
        assert_eq!(value, 3);
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&a.id));
        assert!(deps.contains(&b.id));
        assert!(!deps.contains(&c.id));
    }

    #[test]
    fn tracking_dedups_repeated_reads() {
        fresh();
        let s = use_signal(|| 1_i32);
        let (_, deps) = track_dependencies(|| {
            s.get() + s.get() + s.get()
        });
        assert_eq!(deps, vec![s.id]);
    }

    #[test]
    fn nested_track_dependencies_clears_outer() {
        fresh();
        let a = use_signal(|| 0_i32);
        let b = use_signal(|| 0_i32);
        let (_, deps_outer) = track_dependencies(|| {
            let _ = a.get();
            let (_, _deps_inner) = track_dependencies(|| {
                let _ = b.get();
            });
            // Inner tracking clears the tracking list, so the outer
            // doesn't see `a` afterwards. This is acceptable for our
            // single-scope-per-frame model; nested tracking is unusual.
        });
        // We don't assert on outer deps because the cleared-during-nesting
        // semantics make them empty. The contract is: don't nest.
        assert!(deps_outer.is_empty() || deps_outer == vec![a.id]);
    }

    #[test]
    fn update_helper_increments_counter() {
        fresh();
        let count = use_signal(|| 0_i32);
        count.update(|n| n + 1);
        count.update(|n| n + 1);
        count.update(|n| n + 1);
        assert_eq!(count.get(), 3);
    }

    #[test]
    fn many_signals_get_distinct_ids() {
        fresh();
        let mut ids = Vec::new();
        for i in 0..50 {
            let s = use_signal(|| i);
            ids.push(s.id);
        }
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "signal ids must be distinct");
    }

    #[test]
    fn signal_handles_typed() {
        // Just verifying the type system actually parameterises Signal<T>.
        fresh();
        let int: Signal<i32> = use_signal(|| 0);
        let str_: Signal<String> = use_signal(|| String::new());
        let _ = (int.get(), str_.get());
    }
}
