//! [`AnimationController`] + the per-thread [`AnimationScheduler`].
//!
//! The controller is a state machine driving a `0..1` **progress**
//! signal. It is *idle when not driving*: `forward` / `reverse` /
//! `animate_to` / `repeat` register it with the scheduler (which keeps
//! the host ticking and advances it each frame); reaching the target,
//! `stop`, or owner-dispose deregisters it. `set_value` writes the
//! progress once without registering.
//!
//! See `docs/animation-design.md` for the model.

use std::cell::RefCell;
use std::rc::Rc;

use whisker_runtime::anim_hook;
use whisker_runtime::reactive::{ReadSignal, RwSignal, on_cleanup, signal};

use crate::config::AnimConfig;

/// How a running controller continues once it reaches its target.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Repeat {
    /// Stop at the target (the default).
    Once,
    /// Jump back to the start and run forward again, indefinitely.
    Loop,
    /// Ping-pong: on reaching a bound, flip direction and continue.
    Reverse,
}

/// Direction a controller is currently driving.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Dir {
    Forward,
    Backward,
}

/// Mutable controller state, shared between the [`AnimationController`]
/// handle, the scheduler's active list, and the registered cleanup.
struct ControllerState {
    cfg: AnimConfig,
    /// The progress signal (`0.0..=1.0`) consumers read.
    value: RwSignal<f32>,
    /// Whether this controller is currently self-driving.
    active: bool,
    /// Target progress the current run drives toward.
    target: f32,
    /// Progress at the moment the current run started.
    start_value: f32,
    /// Scheduler timestamp (ms) at which the current run started.
    ///
    /// `None` until the first `advance` after a run begins: the start
    /// time is anchored to that frame's `now_ms`, NOT to whatever the
    /// scheduler's `last_ms` happened to be at `register` time. The
    /// scheduler only updates `last_ms` while stepping, so after an idle
    /// gap it is stale; anchoring there made the first frame compute a
    /// huge elapsed and jump straight to the target (the "single tap
    /// teleports, mashing animates" bug). Lazy anchoring starts every run
    /// from progress 0 at its first real frame.
    start_ms: Option<f64>,
    /// Direction of the current run (for ping-pong bookkeeping).
    dir: Dir,
    repeat: Repeat,
    /// Callbacks fired when a (non-repeating) run reaches its target.
    on_finish: Vec<Box<dyn FnMut()>>,
}

impl ControllerState {
    /// Distance-proportional duration (ms) for the current run: a half
    /// sweep takes half the configured time, so `set_value` then
    /// `forward` settles at a consistent rate. Guards a zero/positive
    /// duration so a degenerate config finishes immediately.
    fn run_duration_ms(&self) -> f64 {
        let span = (self.target - self.start_value).abs();
        (self.cfg.duration_ms as f64) * span as f64
    }

    /// Advance to time `now_ms`. Returns `true` if still animating.
    fn advance(&mut self, now_ms: f64) -> bool {
        if !self.active {
            return false;
        }
        // Lazy time anchor: the first frame of a run defines its start, so
        // an idle gap before the run can't inflate the elapsed time. This
        // frame is therefore progress 0 (raw_t == 0) — the run advances
        // from the *next* frame onward.
        let start_ms = *self.start_ms.get_or_insert(now_ms);
        let dur = self.run_duration_ms();
        // A tiny epsilon (in ms) absorbs the f32→f64 rounding in
        // `run_duration_ms` (e.g. a 0.6 span yields dur ≈ 60.0000023,
        // so a frame at exactly 60ms would otherwise read t = 0.99999996
        // and never finish). Treat "within a hundredth of a ms of done"
        // as done.
        let finished = dur <= 0.0 || (now_ms - start_ms) >= dur - 1e-2;
        let raw_t = if finished {
            1.0
        } else {
            ((now_ms - start_ms) / dur).clamp(0.0, 1.0) as f32
        };
        let eased = self.cfg.curve.ease(raw_t);
        let progress = self.start_value + (self.target - self.start_value) * eased;
        self.value.set(progress);

        if !finished {
            return true;
        }

        // Reached the target.
        self.value.set(self.target);
        match self.repeat {
            Repeat::Once => {
                self.active = false;
                let mut cbs = std::mem::take(&mut self.on_finish);
                for cb in cbs.iter_mut() {
                    cb();
                }
                self.on_finish = cbs;
                false
            }
            Repeat::Loop => {
                // Restart from the opposite bound in the same direction.
                let (from, to) = match self.dir {
                    Dir::Forward => (0.0, 1.0),
                    Dir::Backward => (1.0, 0.0),
                };
                self.value.set(from);
                self.start_value = from;
                self.target = to;
                self.start_ms = Some(now_ms);
                true
            }
            Repeat::Reverse => {
                // Flip direction and continue from this bound.
                self.dir = match self.dir {
                    Dir::Forward => Dir::Backward,
                    Dir::Backward => Dir::Forward,
                };
                self.start_value = self.target;
                self.target = match self.dir {
                    Dir::Forward => 1.0,
                    Dir::Backward => 0.0,
                };
                self.start_ms = Some(now_ms);
                true
            }
        }
    }
}

thread_local! {
    static SCHEDULER: RefCell<AnimationScheduler> =
        RefCell::new(AnimationScheduler::new());
}

/// One per runtime thread: holds the set of active controllers and
/// advances them each frame. Registered with the runtime's
/// [`anim_hook`] the first time any controller is created on this
/// thread, so the driver's `tick_frame` drives it.
struct AnimationScheduler {
    active: Vec<Rc<RefCell<ControllerState>>>,
    /// Most recent timestamp seen from `step` — used as the start time
    /// for a `forward`/`reverse` issued between frames.
    last_ms: f64,
    /// Whether the per-frame callback has been registered with the
    /// runtime's `anim_hook` for this thread.
    hook_installed: bool,
}

impl AnimationScheduler {
    fn new() -> Self {
        Self {
            active: Vec::new(),
            last_ms: 0.0,
            hook_installed: false,
        }
    }
}

/// Ensure the scheduler's per-frame step callback is registered with
/// the runtime, so `tick_frame` advances animations on this thread.
fn ensure_hook_installed() {
    let already = SCHEDULER.with(|s| {
        let mut s = s.borrow_mut();
        let was = s.hook_installed;
        s.hook_installed = true;
        was
    });
    if !already {
        anim_hook::set_step_callback(Box::new(step));
    }
}

/// Advance every active controller by one frame at `now_ms`. Returns
/// `true` if any controller is still animating afterward. This is the
/// callback the runtime's `anim_hook` invokes each `tick_frame`.
fn step(now_ms: f64) -> bool {
    // Snapshot the active list outside the scheduler borrow: a
    // controller's `advance` writes its value signal, which schedules
    // reactive subscribers — none of which re-enter the scheduler, but
    // keeping the borrow window tight matches the runtime's discipline.
    let snapshot: Vec<Rc<RefCell<ControllerState>>> = SCHEDULER.with(|s| {
        let mut s = s.borrow_mut();
        s.last_ms = now_ms;
        s.active.clone()
    });

    let mut finished: Vec<*const RefCell<ControllerState>> = Vec::new();
    for st in &snapshot {
        let still = st.borrow_mut().advance(now_ms);
        if !still {
            finished.push(Rc::as_ptr(st));
        }
    }

    SCHEDULER.with(|s| {
        let mut s = s.borrow_mut();
        if !finished.is_empty() {
            s.active.retain(|st| !finished.contains(&Rc::as_ptr(st)));
        }
        let any = !s.active.is_empty();
        anim_hook::mark_animating(any);
        any
    })
}

/// Register `state` as active (idempotent) and wake the host so a frame
/// runs. The run's start time is anchored lazily on its first `advance`
/// (see `ControllerState::start_ms`), so `register` clears it rather than
/// reading the scheduler's possibly-stale `last_ms`.
fn register(state: &Rc<RefCell<ControllerState>>) {
    let already = SCHEDULER.with(|s| {
        let s = s.borrow();
        s.active.iter().any(|a| Rc::ptr_eq(a, state))
    });
    state.borrow_mut().start_ms = None;
    if !already {
        SCHEDULER.with(|s| s.borrow_mut().active.push(state.clone()));
    }
    // Report busy immediately (before the next `step`) and nudge the
    // host to schedule a frame.
    anim_hook::mark_animating(true);
    whisker_runtime::host_wake::wake_runtime();
}

/// Deregister `state` from the active list (no-op if absent).
fn deregister(state: &Rc<RefCell<ControllerState>>) {
    SCHEDULER.with(|s| {
        let mut s = s.borrow_mut();
        s.active.retain(|a| !Rc::ptr_eq(a, state));
        if s.active.is_empty() {
            anim_hook::mark_animating(false);
        }
    });
}

/// (Test only) number of currently-active controllers on this thread.
#[doc(hidden)]
pub fn __active_count() -> usize {
    SCHEDULER.with(|s| s.borrow().active.len())
}

/// (Test only) drive one animation frame at monotonic time `now_ms`
/// (milliseconds) without a real clock. Mirrors what the driver's
/// `tick_frame` does, then flushes the reactive queue so tween
/// `computed`s recompute.
///
/// Returns `true` if any controller is still animating.
#[doc(hidden)]
pub fn __step_for_tests(now_ms: f64) -> bool {
    let still = step(now_ms);
    whisker_runtime::reactive::flush();
    still
}

/// (Test only) reset the scheduler thread-local. Pairs with the
/// runtime's `__reset_for_tests`.
#[doc(hidden)]
pub fn __reset_for_tests() {
    SCHEDULER.with(|s| *s.borrow_mut() = AnimationScheduler::new());
    anim_hook::__reset_for_tests();
}

/// Drives a `0..1` progress signal as an explicit state machine.
///
/// Construct with [`AnimationController::new`], then drive it with
/// [`forward`](Self::forward) / [`reverse`](Self::reverse) /
/// [`stop`](Self::stop) / [`animate_to`](Self::animate_to) /
/// [`set_value`](Self::set_value). Read the live progress via
/// [`value`](Self::value) — a `ReadSignal<f32>` consumable anywhere in
/// the reactive graph (e.g. by a [`Tween`](crate::Tween)).
///
/// **No auto-play**: a freshly-constructed controller sits at `0.0` and
/// requests no frames until you drive it. **Idle is free**: when no
/// controller is driving, the engine adds no per-frame work.
///
/// The controller is owned by the current reactive owner: when that
/// owner disposes, the controller deregisters from the scheduler — no
/// leaked frame requests.
#[derive(Clone)]
pub struct AnimationController {
    state: Rc<RefCell<ControllerState>>,
}

impl AnimationController {
    /// Create a controller for `cfg`, sitting idle at progress `0.0`.
    pub fn new(cfg: AnimConfig) -> Self {
        ensure_hook_installed();
        let value = signal(0.0_f32);
        let state = Rc::new(RefCell::new(ControllerState {
            cfg,
            value,
            active: false,
            target: 0.0,
            start_value: 0.0,
            start_ms: None,
            dir: Dir::Forward,
            repeat: Repeat::Once,
            on_finish: Vec::new(),
        }));

        // Deregister on owner dispose so a controller created inside a
        // component never leaves a dangling frame request behind.
        let weak = Rc::downgrade(&state);
        on_cleanup(move || {
            if let Some(st) = weak.upgrade() {
                deregister(&st);
            }
        });

        Self { state }
    }

    /// The live progress as a read-only signal (`0.0..=1.0`).
    pub fn value(&self) -> ReadSignal<f32> {
        self.state.borrow().value.read_only()
    }

    /// Drive from the current value toward `1.0`.
    pub fn forward(&self) {
        self.animate_to(1.0);
    }

    /// Drive from the current value back toward `0.0`.
    pub fn reverse(&self) {
        self.animate_to(0.0);
    }

    /// Drive from the current value toward an arbitrary `target`
    /// (clamped to `0.0..=1.0`). If already at the target, finishes
    /// immediately (fires `on_finish`) without registering a frame.
    pub fn animate_to(&self, target: f32) {
        let target = target.clamp(0.0, 1.0);
        let start_value = self.state.borrow().value.get_untracked();
        {
            let mut st = self.state.borrow_mut();
            st.repeat = Repeat::Once;
            st.target = target;
            st.start_value = start_value;
            st.dir = if target >= start_value {
                Dir::Forward
            } else {
                Dir::Backward
            };
            st.active = true;
        }
        if (target - start_value).abs() <= f32::EPSILON {
            // Already there: settle synchronously, fire on_finish, and
            // don't register (nothing to animate).
            let mut st = self.state.borrow_mut();
            st.value.set(target);
            st.active = false;
            let mut cbs = std::mem::take(&mut st.on_finish);
            drop(st);
            for cb in cbs.iter_mut() {
                cb();
            }
            self.state.borrow_mut().on_finish = cbs;
            return;
        }
        register(&self.state);
    }

    /// Halt at the current value. Deregisters from the scheduler; the
    /// value signal holds its last progress. Does not fire `on_finish`.
    pub fn stop(&self) {
        self.state.borrow_mut().active = false;
        deregister(&self.state);
    }

    /// Set the progress to `v` (clamped `0.0..=1.0`) **once**, without
    /// self-driving. The canonical finger-driven path: one update per
    /// gesture frame. Deregisters any in-flight run so the manual value
    /// isn't immediately overwritten by the scheduler.
    pub fn set_value(&self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        {
            let mut st = self.state.borrow_mut();
            st.active = false;
            st.value.set(v);
        }
        deregister(&self.state);
    }

    /// Register a callback fired each time a non-repeating run reaches
    /// its target (via `forward` / `reverse` / `animate_to`). Multiple
    /// callbacks accumulate.
    pub fn on_finish(&self, cb: impl FnMut() + 'static) {
        self.state.borrow_mut().on_finish.push(Box::new(cb));
    }

    /// Run forward to `1.0`, then restart from `0.0` and run forward
    /// again, indefinitely. Registers the controller.
    pub fn repeat(&self) {
        let start_value = self.state.borrow().value.get_untracked();
        {
            let mut st = self.state.borrow_mut();
            st.repeat = Repeat::Loop;
            st.dir = Dir::Forward;
            st.start_value = start_value;
            st.target = 1.0;
            st.active = true;
        }
        register(&self.state);
    }

    /// Ping-pong forever: forward to `1.0`, reverse to `0.0`, repeat.
    /// Registers the controller.
    pub fn repeat_reverse(&self) {
        let start_value = self.state.borrow().value.get_untracked();
        {
            let mut st = self.state.borrow_mut();
            st.repeat = Repeat::Reverse;
            st.dir = Dir::Forward;
            st.start_value = start_value;
            st.target = 1.0;
            st.active = true;
        }
        register(&self.state);
    }

    /// Whether this controller is currently self-driving.
    pub fn is_animating(&self) -> bool {
        self.state.borrow().active
    }
}
