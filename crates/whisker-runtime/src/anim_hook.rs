//! Frame-driving hook for the continuous animation engine.
//!
//! The animation engine (`whisker-animation`) is a *separate* crate
//! that depends on this one — so this runtime crate cannot name its
//! `AnimationScheduler` directly (that would be a dependency cycle).
//! Instead the runtime exposes a tiny inversion-of-control surface:
//!
//! - The engine registers a **per-frame step callback** via
//!   [`set_step_callback`]. The callback advances every active
//!   controller by the elapsed wall-clock time and returns whether
//!   *any* controller is still animating.
//! - Each frame, the driver's `tick_frame` calls [`step`], which
//!   invokes the registered callback and latches its "still animating"
//!   result into a thread-local flag.
//! - [`is_animating`] reports that flag. The reactive scheduler's
//!   [`has_pending_work`](crate::reactive::has_pending_work) ORs it in,
//!   so the host keeps its vsync loop running while an animation is in
//!   flight and releases it the moment the last controller finishes —
//!   matching the runtime's level-triggered idle model.
//!
//! This keeps the runtime ignorant of *how* animation works (curves,
//! tweens, springs) while still owning the one thing only it can own:
//! the decision to keep ticking. Engine logic, types, and tests all
//! live in `whisker-animation`.
//!
//! Single-threaded, like the rest of the reactive runtime: everything
//! here lives in a `thread_local!` and runs on the TASM thread.

use std::cell::Cell;
use std::cell::RefCell;

/// Signature of the engine's per-frame advance callback.
///
/// Receives the current monotonic timestamp in **milliseconds**
/// (injectable for tests; the driver feeds it a real monotonic clock).
/// Returns `true` if at least one controller is still animating after
/// this step — i.e. the host should schedule another frame.
pub type StepCallback = Box<dyn FnMut(f64) -> bool>;

thread_local! {
    /// The engine's registered step callback, if any.
    static STEP: RefCell<Option<StepCallback>> = const { RefCell::new(None) };
    /// Latched "is anything animating right now" flag. Written by
    /// [`step`], read by [`is_animating`].
    static ANIMATING: Cell<bool> = const { Cell::new(false) };
}

/// Register the engine's per-frame step callback. Called once by
/// `whisker-animation` when its scheduler is first touched on this
/// thread. Passing a new callback replaces any previous one.
pub fn set_step_callback(cb: StepCallback) {
    STEP.with(|s| *s.borrow_mut() = Some(cb));
}

/// Advance the animation engine by one frame at monotonic time
/// `now_ms` (milliseconds). Invokes the registered step callback (if
/// any) and latches whether anything is still animating.
///
/// Called once per frame from the driver's `tick_frame`. A no-op (and
/// clears the animating flag) when no engine is registered.
pub fn step(now_ms: f64) {
    // Take the callback out of the cell so the engine body — which may
    // re-enter the runtime to write signals — never runs while we hold
    // the `STEP` borrow. Mirrors the scheduler's compute-Rc pattern.
    let cb = STEP.with(|s| s.borrow_mut().take());
    let Some(mut cb) = cb else {
        ANIMATING.with(|a| a.set(false));
        return;
    };
    let still = cb(now_ms);
    STEP.with(|s| *s.borrow_mut() = Some(cb));
    ANIMATING.with(|a| a.set(still));
}

/// Whether any controller was still animating as of the last [`step`].
///
/// `has_pending_work()` ORs this in so the host keeps ticking while an
/// animation is in flight.
pub fn is_animating() -> bool {
    ANIMATING.with(|a| a.get())
}

/// Directly set the animating flag. The engine calls this when a
/// controller is registered *between* frames (e.g. `forward()` from an
/// event handler) so `has_pending_work()` reports busy immediately —
/// before the next `step` has run — and the host wakes for a frame.
pub fn mark_animating(active: bool) {
    ANIMATING.with(|a| a.set(active));
}

/// (Test only) clear the registered callback and animating flag.
#[doc(hidden)]
pub fn __reset_for_tests() {
    STEP.with(|s| *s.borrow_mut() = None);
    ANIMATING.with(|a| a.set(false));
}
