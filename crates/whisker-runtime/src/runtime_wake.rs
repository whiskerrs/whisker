//! Host wake-up boundary for retained runtime instances and ABI adapters.
//!
//! Lives outside [`crate::view`] / [`crate::reactive`] because both need it:
//! - [`crate::reactive::scheduler`] calls [`wake_runtime`] on the
//!   empty→non-empty edge of the pending queue so the host wakes
//!   up to drain effects.
//! - the `whisker-dev-runtime` hot-reload receiver calls [`wake_runtime`]
//!   after parking a patch.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::Mutex;

/// Any-thread wake-up endpoint supplied by a Host for one runtime instance.
pub trait RuntimeWake: Send + Sync + 'static {
    /// Coalesces or posts a request to drive the owning UI event loop.
    fn wake(&self);
}

impl<F> RuntimeWake for F
where
    F: Fn() + Send + Sync + 'static,
{
    fn wake(&self) {
        self();
    }
}

/// Cloneable wake-up capability captured by async task wakers.
#[derive(Clone)]
pub struct RuntimeWakeHandle(Arc<dyn RuntimeWake>);

impl std::fmt::Debug for RuntimeWakeHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeWakeHandle(..)")
    }
}

impl RuntimeWakeHandle {
    /// Wraps one Host wake-up endpoint.
    pub fn new(wake: impl RuntimeWake) -> Self {
        Self(Arc::new(wake))
    }

    /// Requests one future runtime drive.
    pub fn wake(&self) {
        self.0.wake();
    }
}

/// "Wake the host" callback. The host registers one of these during
/// init via [`set_request_frame_callback`]; whenever the runtime
/// transitions from idle to "we have pending work", it fires the
/// callback so the host can resume its render loop (e.g. unpause a
/// `CADisplayLink`).
///
/// Stored as a raw fn pointer + opaque `user_data` rather than a
/// boxed closure so the C ABI can pass it through unchanged.
#[derive(Copy, Clone)]
struct RequestFrameCb {
    func: extern "C" fn(*mut c_void),
    user_data: *mut c_void,
}

/// SAFETY: `user_data` is an opaque host pointer. The host promises
/// it remains valid for the lifetime of the dev session and is safe
/// to call from any thread (the registered callbacks on Android /
/// iOS just post a "wake" message onto the runtime thread).
unsafe impl Send for RequestFrameCb {}
unsafe impl Sync for RequestFrameCb {}

/// Cross-thread mirror of the callback registered by an ABI adapter. Direct
/// Rust Hosts use [`RuntimeWakeHandle`] instead; mobile bindings retain this
/// slot so foreign callbacks can wake the runtime without thread-local access.
static REMOTE_WAKE: Mutex<Option<RequestFrameCb>> = Mutex::new(None);

thread_local! {
    static ACTIVE_WAKE: RefCell<Option<RuntimeWakeHandle>> = const { RefCell::new(None) };
}

pub(crate) fn swap_active_wake(wake: &mut Option<RuntimeWakeHandle>) {
    ACTIVE_WAKE.with_borrow_mut(|active| std::mem::swap(active, wake));
}

pub(crate) fn current_wake_handle() -> Option<RuntimeWakeHandle> {
    ACTIVE_WAKE.with_borrow(Clone::clone)
}

/// Register the host's wake-up callback. Pass `None` to clear.
///
/// ABI adapters call this while attaching a Host runtime.
#[doc(hidden)]
pub fn set_request_frame_callback(
    func: Option<extern "C" fn(*mut c_void)>,
    user_data: *mut c_void,
) {
    let built = func.map(|func| RequestFrameCb { func, user_data });
    if let Ok(mut guard) = REMOTE_WAKE.lock() {
        *guard = built;
    }
}

/// Fire the registered wake callback, if any. Safe to call from any
/// thread — the Host callback must post onto its owning UI event loop. No-op
/// if no callback is registered.
pub fn wake_runtime() {
    if let Some(wake) = current_wake_handle() {
        wake.wake();
        return;
    }
    let cb = REMOTE_WAKE.lock().ok().and_then(|g| *g);
    if let Some(cb) = cb {
        (cb.func)(cb.user_data);
    }
}

/// (Test only) clear the registered callback.
#[doc(hidden)]
pub fn __reset_for_tests() {
    ACTIVE_WAKE.with_borrow_mut(|wake| *wake = None);
    if let Ok(mut guard) = REMOTE_WAKE.lock() {
        *guard = None;
    }
}

#[cfg(test)]
pub(crate) fn host_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|error| error.into_inner())
}
