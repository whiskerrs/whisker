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
//! Single-threaded, like the rest of the UI runtime: the active execution slot
//! is thread-local, while `RuntimeContext` retains one isolated animation state
//! per mounted instance between Host callbacks.

use std::cell::RefCell;

/// Signature of the engine's per-frame advance callback.
///
/// Receives the current monotonic timestamp in **milliseconds**
/// (injectable for tests; the driver feeds it a real monotonic clock).
/// Returns `true` if at least one controller is still animating after
/// this step — i.e. the host should schedule another frame.
pub type StepCallback = Box<dyn FnMut(f64) -> bool>;

pub(crate) struct AnimationState {
    step: Option<StepCallback>,
    animating: bool,
}

impl AnimationState {
    pub(crate) const fn new() -> Self {
        Self {
            step: None,
            animating: false,
        }
    }
}

thread_local! {
    static STATE: RefCell<AnimationState> = const { RefCell::new(AnimationState::new()) };
}

/// Register the engine's per-frame step callback. Called once by
/// `whisker-animation` when its scheduler is first touched on this
/// thread. Passing a new callback replaces any previous one.
pub fn set_step_callback(cb: StepCallback) {
    STATE.with_borrow_mut(|state| state.step = Some(cb));
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
    let cb = STATE.with_borrow_mut(|state| state.step.take());
    let Some(mut cb) = cb else {
        STATE.with_borrow_mut(|state| state.animating = false);
        return;
    };
    let still = cb(now_ms);
    STATE.with_borrow_mut(|state| {
        state.step = Some(cb);
        state.animating = still;
    });
}

/// Whether any controller was still animating as of the last [`step`].
///
/// `has_pending_work()` ORs this in so the host keeps ticking while an
/// animation is in flight.
pub fn is_animating() -> bool {
    STATE.with_borrow(|state| state.animating)
}

/// Directly set the animating flag. The engine calls this when a
/// controller is registered *between* frames (e.g. `forward()` from an
/// event handler) so `has_pending_work()` reports busy immediately —
/// before the next `step` has run — and the host wakes for a frame.
pub fn mark_animating(active: bool) {
    STATE.with_borrow_mut(|state| state.animating = active);
}

pub(crate) fn swap_state(state: &mut AnimationState) {
    STATE.with_borrow_mut(|active| std::mem::swap(active, state));
}

/// (Test only) clear the registered callback and animating flag.
#[doc(hidden)]
pub fn __reset_for_tests() {
    STATE.with_borrow_mut(|state| *state = AnimationState::new());
}
