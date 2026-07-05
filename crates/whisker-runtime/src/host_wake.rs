//! Host wake-up bridge — the C callback the iOS / Android shell
//! registers so the runtime can ask the host to schedule another
//! `whisker_tick`.
//!
//! Lives outside [`view`] / [`reactive`] because both crates need it:
//! - [`crate::reactive::scheduler`] calls [`wake_runtime`] on the
//!   empty→non-empty edge of the pending queue so the host wakes
//!   up to drain effects.
//! - [`crate::whisker_dev_runtime`] (hot-reload receiver)
//!   calls [`wake_runtime`] from its WebSocket thread after parking
//!   a patch, so the host runs another tick that picks the patch
//!   up.
//!
//! Pre-Phase-6.5a this lived in `signal.rs` alongside the old
//! `Signal<T>` API. With that gone it's a standalone module.

use std::ffi::c_void;
use std::sync::Mutex;

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

/// Cross-thread mirror of the registered callback. Stored globally
/// so threads other than the TASM thread (e.g. the WebSocket
/// receiver in `whisker-dev-runtime`) can wake the runtime via
/// [`wake_runtime`] without needing thread-local access. The TASM
/// thread also writes through here, since there's only one slot
/// and locking is cheap on the rare path that uses it.
static REMOTE_WAKE: Mutex<Option<RequestFrameCb>> = Mutex::new(None);

/// Register the host's wake-up callback. Pass `None` to clear.
///
/// In production this is called once during
/// `whisker-driver::bootstrap::run` and never again.
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
/// thread — the host's callback contract is "any-thread, posts a
/// message to the TASM thread". No-op if no callback is registered
/// (signal writes during init may happen before bootstrap has wired
/// anything up).
pub fn wake_runtime() {
    let cb = REMOTE_WAKE.lock().ok().and_then(|g| *g);
    if let Some(cb) = cb {
        (cb.func)(cb.user_data);
    }
}

/// (Test only) clear the registered callback.
#[doc(hidden)]
pub fn __reset_for_tests() {
    if let Ok(mut guard) = REMOTE_WAKE.lock() {
        *guard = None;
    }
}
