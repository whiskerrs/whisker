//! Interactive back gestures — iOS edge [`SwipeBack`] and Android
//! [`AndroidPredictiveBack`] — both driving the **same coordinated
//! two-screen scrub** the non-interactive pop uses.
//!
//! A back gesture has a continuous `0..1` progress (finger drag on iOS, a
//! `BackEventCompat` on Android). Both gestures map that progress onto one
//! stack-transition controller via the shared helpers ([`begin`] /
//! [`scrub`] / [`settle`]): they point **both** the top wrapper
//! (`Role::Top`) and the revealed-under wrapper (`Role::Under`) at the top
//! controller and scrub it (`set_value(1.0 - back_progress)`), so the top
//! tracks the gesture while the screen beneath slides back from covered to
//! rest. On release/commit they hand off to `reverse()` (commit → run the
//! reconcile pop + `navigator.back()`) or `forward()` (cancel → restore).
//! The *only* platform-specific part is the progress input path: an
//! `Element` touch loop vs the `PredictiveBack` native module's events.
//!
//! The intermediate `0..1` lives in a real signal (the controller's
//! progress); both screens are posed by the runtime's pose effects — no
//! hand-written per-frame inline transform.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::event::TouchEvent;
use whisker::platform_module::WhiskerValue;
use whisker::runtime::event::bind_typed;
use whisker::runtime::reactive::on_mount;
use whisker::runtime::view::{BindType, Element};
use whisker::{AnimationController, component, module, on_cleanup, render, use_context};

use crate::render::components::RouterRoot;
use crate::render::handle::{PoseBinding, RouterHandle, StackBridge, use_navigator};
use crate::render::transition::{self, PoseMode, Role, SwipeEdge, Transition};

/// Approximate viewport width (pt) the finger travels for a full swipe.
/// A future revision can read the real container width.
const SWIPE_FULL_DISTANCE_PX: f32 = 402.0;
/// `clientX` within which a touchstart qualifies as an edge swipe.
const SWIPE_EDGE_THRESHOLD_PX: f32 = 24.0;
/// Progress (toward back) at release above which the gesture commits.
const SWIPE_COMMIT_THRESHOLD: f32 = 0.5;

/// In-flight gesture state. `None` when no swipe is active.
struct Gesture {
    start_x: f32,
    last_x: f32,
    identifier: i64,
    bridge: StackBridge,
    /// Back progress 0..1 (0 = top fully present, 1 = fully swiped away).
    progress: f32,
}

/// iOS edge swipe-back gesture component. Renders nothing; binds touch
/// handlers in `on_mount`.
#[component]
pub fn swipe_back() -> Element {
    let nav = use_navigator();
    // Bind to the router's screen-spanning root (a phantom slot has no
    // extent and would never be hit by a touch). The `RouterRoot` context is
    // published by `router()` BEFORE the children mount, so it is visible here.
    let container = use_context::<RouterRoot>().map(|r| r.0);
    on_mount(move || {
        if let Some(container) = container {
            install(container, nav.clone());
        }
    });
    render! { fragment() }
}

fn install(container: Element, nav: RouterHandle) {
    let gesture: Rc<RefCell<Option<Gesture>>> = Rc::new(RefCell::new(None));

    // touchstart — qualify the edge swipe and grab the active bridge.
    {
        let gesture = gesture.clone();
        let nav = nav.clone();
        bind_typed::<TouchEvent, _>(container, "touchstart", BindType::Bind, move |e| {
            if gesture.borrow().is_some() {
                return;
            }
            let Some(touch) = e
                .changed_touches
                .first()
                .copied()
                .or_else(|| e.touches.first().copied())
            else {
                return;
            };
            let start_x = touch.client_x as f32;
            if start_x > SWIPE_EDGE_THRESHOLD_PX {
                return;
            }
            // Grab the active stack's bridge and point both wrappers at the
            // top controller (shared with Android predictive-back). The iOS
            // edge swipe is always a left-edge gesture.
            let Some(bridge) = begin(&nav, SwipeEdge::Left) else {
                return;
            };
            *gesture.borrow_mut() = Some(Gesture {
                start_x,
                last_x: start_x,
                identifier: touch.identifier,
                bridge,
                progress: 0.0,
            });
        });
    }

    // touchmove — finger delta → back-progress, scrub both wrappers.
    {
        let gesture = gesture.clone();
        bind_typed::<TouchEvent, _>(container, "touchmove", BindType::Bind, move |e| {
            let mut g = gesture.borrow_mut();
            let Some(state) = g.as_mut() else { return };
            let Some(touch) = e
                .touches
                .iter()
                .find(|t| t.identifier == state.identifier)
                .or_else(|| {
                    e.changed_touches
                        .iter()
                        .find(|t| t.identifier == state.identifier)
                })
                .copied()
            else {
                return;
            };
            let x = touch.client_x as f32;
            state.last_x = x;
            let delta = (x - state.start_x).max(0.0);
            let progress = (delta / SWIPE_FULL_DISTANCE_PX).clamp(0.0, 1.0);
            state.progress = progress;
            scrub(&state.bridge, progress);
        });
    }

    // touchend / touchcancel — hand off to the controller with velocity.
    for end_name in ["touchend", "touchcancel"] {
        let gesture = gesture.clone();
        let nav = nav.clone();
        bind_typed::<TouchEvent, _>(container, end_name, BindType::Bind, move |_e| {
            let Some(state) = gesture.borrow_mut().take() else {
                return;
            };
            let commit = state.progress >= SWIPE_COMMIT_THRESHOLD;
            // A rough release velocity in progress units / second. Without
            // precise timestamps we approximate from the gesture's reach;
            // the controller clamps and springs regardless.
            let velocity = (state.progress * 4.0).clamp(0.0, 6.0);
            settle(&nav, &state.bridge, commit, Some(velocity));
        });
    }
}

// =====================================================================
// Shared coordinated-scrub helpers (iOS + Android)
// =====================================================================

/// Start a back gesture (from `edge`) on the deepest active stack:
/// validate it can pop and supports an edge gesture, point **both**
/// wrappers at the top controller (`Top` / `Under`) in the
/// **predictive-back** pose mode, and return the bridge. `None` means no
/// gesture should begin.
pub(crate) fn begin(nav: &RouterHandle, edge: SwipeEdge) -> Option<StackBridge> {
    let bridge = nav.active_stack_bridge()?;
    // Edge-swipe back is opt-in by *mounting* a gesture component
    // (`SwipeBack` / `AndroidPredictiveBack`); it is NOT gated on the
    // route's transition. The only requirement is that the stack can pop.
    if !bridge.can_back {
        return None;
    }
    // The interactive preview is platform-native:
    //  - Android: the Material **predictive-back** card (shrink + rounded
    //    corners + backdrop dim).
    //  - iOS / others: the interactive **iOS slide-back** (the top slides
    //    off to the right, the under parallaxes back) — the route's slide
    //    pose driven by the finger, NOT the Material card.
    let android = cfg!(target_os = "android");
    let mode = if android {
        PoseMode::Predictive(edge)
    } else {
        PoseMode::Transition(Transition::slide())
    };
    if let (Some(ctrl), Some(top), Some(under)) =
        (&bridge.top_ctrl, &bridge.top_pose, &bridge.under_pose)
    {
        point(top, ctrl, Role::Top, mode.clone());
        point(under, ctrl, Role::Under, mode);
        // The Material backdrop dim is Android-only; the iOS slide carries
        // its own subtle under-screen dim in `slide_pose`.
        if android {
            if let Some(dim_drive) = &bridge.dim_drive {
                dim_drive.set(Some(ctrl.clone()));
            }
        }
    }
    Some(bridge)
}

/// Scrub the pair to `back_progress` (0 = top present, 1 = swiped away):
/// set the shared top controller's progress. Both wrappers re-pose and the
/// backdrop dim follows automatically via their reactive bindings.
pub(crate) fn scrub(bridge: &StackBridge, back_progress: f32) {
    if let Some(ctrl) = &bridge.top_ctrl {
        ctrl.set_value(1.0 - back_progress);
    }
}

/// Settle a back gesture on release. `commit` drives the top off-screen
/// (progress → 0) and calls `navigator.back()` on finish (the same
/// reconcile pop); cancel springs it back to present and clears the dim.
/// `velocity` (progress units/sec) is the release hand-off; `None` uses a
/// plain run (Android's system back carries no velocity).
pub(crate) fn settle(
    nav: &RouterHandle,
    bridge: &StackBridge,
    commit: bool,
    velocity: Option<f32>,
) {
    let Some(ctrl) = bridge.top_ctrl.clone() else {
        return;
    };
    let dim_drive = bridge.dim_drive;

    // NB: do NOT re-anchor the controller before the run. `reverse()` /
    // `forward()` animate from the controller's CURRENT value to the
    // target, which is exactly the remaining gesture distance. A deep
    // swipe (value ≈ 0.05) commits by animating 0.05 → 0 — a short, correct
    // arc; forcing it back to 0.18 first would visibly jump *backward*
    // before going to 0. When the user dragged all the way (value already
    // at the target) the run finishes immediately — also correct, since
    // the dismiss is already visually complete.
    if commit {
        // Drive the top off-screen; both wrappers follow via their pose
        // bindings. On finish call `back()`, which runs the same
        // coordinated pop and unmounts the popped entry (its controller is
        // already at 0, so the reconcile's reverse settles instantly).
        let nav = nav.clone();
        let done = Rc::new(RefCell::new(false));
        ctrl.on_finish(move |finished| {
            if finished && !*done.borrow() {
                *done.borrow_mut() = true;
                // Release the dim drive (→ opacity 0) as the pop commits.
                if let Some(d) = dim_drive {
                    d.set(None);
                }
                nav.back();
            }
        });
        match velocity {
            Some(v) => ctrl.reverse_with_velocity(v),
            None => ctrl.reverse(),
        }
    } else {
        // Cancel: spring the top back to fully present; the under wrapper,
        // pointed at the same controller as `Under`, slides back to covered
        // in lockstep. The dim fades to 0 reactively as the controller
        // returns to 1.0, then the drive is released on finish.
        let done = Rc::new(RefCell::new(false));
        ctrl.on_finish(move |finished| {
            if finished && !*done.borrow() {
                *done.borrow_mut() = true;
                if let Some(d) = dim_drive {
                    d.set(None);
                }
            }
        });
        match velocity {
            Some(v) => ctrl.forward_with_velocity(v),
            None => ctrl.forward(),
        }
    }
}

/// Point a wrapper's pose binding at controller `c` playing `role` in
/// pose `mode`.
fn point(binding: &PoseBinding, c: &AnimationController, role: Role, mode: PoseMode) {
    binding.ctrl.set(c.clone());
    binding.role.set(role);
    binding.mode.set(mode);
}

// =====================================================================
// Android predictive-back
// =====================================================================

/// Android 13+ predictive-back gesture component — the platform-back twin
/// of [`SwipeBack`], driving the identical coordinated scrub.
///
/// Mount it as a child of the [`Router`](crate::render::Router) (alongside
/// `SwipeBack`; each simply waits on its own platform's input). It
/// subscribes to the `whisker-router:PredictiveBack` native module:
///
/// - `backStarted` → [`begin`] the gesture on the active stack.
/// - `backProgressed { progress }` → [`scrub`] the pair by `progress`.
/// - `backCancelled` → [`settle`] as a cancel (spring back to present).
/// - `backInvoked` (commit) → [`settle`] as a commit → `navigator.back()`.
///
/// On API < 34 the platform delivers only `backInvoked` (no preview); the
/// component then just commits — back still works, without the drag
/// preview. Renders nothing.
#[component]
pub fn android_predictive_back() -> Element {
    let nav = use_navigator();
    let module = module!("PredictiveBack");

    // The in-flight bridge for the current predictive-back gesture. Shared
    // across the four event listeners. The native `PredictiveBack` module
    // fires on the Android UI thread — the same thread as Whisker's main
    // loop — so the `MainThreadOnly` shim safely carries the `!Sync`
    // `RouterHandle` + `RefCell` state across the `Send + Sync` bound the
    // bridge's listener box requires.
    let state: Rc<RefCell<Option<StackBridge>>> = Rc::new(RefCell::new(None));

    let started = {
        let shared = MainThreadOnly {
            inner: (nav.clone(), state.clone()),
        };
        module.on_event("backStarted", move |payload| {
            // Capture `shared` whole (not `shared.inner`) so Rust 2021
            // disjoint captures carry its `Send + Sync` impl.
            let shared = &shared;
            let (nav, state) = &shared.inner;
            // Fallback in case the router-init fetch ran before the host
            // Activity attached (idempotent once installed).
            try_fetch_device_corner_radius();
            *state.borrow_mut() = begin(nav, back_edge(&payload));
        })
    };

    let progressed = {
        let shared = MainThreadOnly {
            inner: state.clone(),
        };
        module.on_event("backProgressed", move |payload| {
            let shared = &shared;
            let state = &shared.inner;
            // Also retry here in case the platform skips `backStarted`.
            try_fetch_device_corner_radius();
            if let Some(bridge) = state.borrow().as_ref() {
                // Update the finger's vertical pivot so the shared-element
                // card follows it, then scrub. The drag scrubs only the
                // PREVIEW half of the predictive timeline (value 1.0 → 0.5);
                // the commit/cancel settle drives the rest. See
                // `predictive_pose`'s two-phase doc.
                transition::set_gesture_pivot_y(payload_touch_y(&payload));
                scrub(bridge, back_progress(&payload) * 0.5);
            }
        })
    };

    let cancelled = {
        let shared = MainThreadOnly {
            inner: (nav.clone(), state.clone()),
        };
        module.on_event("backCancelled", move |_payload| {
            let shared = &shared;
            let (nav, state) = &shared.inner;
            if let Some(bridge) = state.borrow_mut().take() {
                settle(nav, &bridge, /* commit = */ false, None);
            }
        })
    };

    let invoked = {
        let shared = MainThreadOnly {
            inner: (nav.clone(), state.clone()),
        };
        module.on_event("backInvoked", move |_payload| {
            let shared = &shared;
            let (nav, state) = &shared.inner;
            match state.borrow_mut().take() {
                // Interactive path (API 34+): a gesture was in flight, so
                // commit it (animate the top off, then `back()`).
                Some(bridge) => settle(nav, &bridge, /* commit = */ true, None),
                // No preview (API < 34, or a discrete press): just pop.
                None => {
                    nav.back();
                }
            }
        })
    };

    // Hold the subscriptions for the component's lifetime; dropping them on
    // unmount fires the module's `OnStopObserving` → the Activity releases
    // its `OnBackPressedCallback`.
    on_cleanup(move || {
        drop(started);
        drop(progressed);
        drop(cancelled);
        drop(invoked);
    });

    render! { fragment() }
}

/// Read `touchY` (0..1, finger Y as a fraction of screen height) from a
/// back-event payload, defaulting to centre (0.5).
fn payload_touch_y(payload: &WhiskerValue) -> f32 {
    let WhiskerValue::Map(fields) = payload else {
        return 0.5;
    };
    match fields.get("touchY") {
        Some(WhiskerValue::Float(v)) => *v as f32,
        Some(WhiskerValue::Int(v)) => *v as f32,
        _ => 0.5,
    }
}

/// Read `progress` (0..1, back-direction) from a back-event payload.
pub(crate) fn back_progress(payload: &WhiskerValue) -> f32 {
    let WhiskerValue::Map(fields) = payload else {
        return 0.0;
    };
    match fields.get("progress") {
        Some(WhiskerValue::Float(v)) => *v as f32,
        Some(WhiskerValue::Int(v)) => *v as f32,
        _ => 0.0,
    }
    .clamp(0.0, 1.0)
}

/// Global "device corner radius successfully installed" latch. Set true
/// only on a *successful* fetch (a numeric result), so an early attempt
/// (e.g. at router init, before the host Activity attaches) that falls
/// back to the default can be retried on the first real gesture.
static CORNER_RADIUS_INSTALLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Query the device display corner radius and install it, at most once
/// (idempotent across the router-init call and the per-gesture fallback).
/// No-op once a value has been installed. `pub(crate)` so `Router` can
/// prime it at init.
pub(crate) fn try_fetch_device_corner_radius() {
    use std::sync::atomic::Ordering;
    if CORNER_RADIUS_INSTALLED.load(Ordering::Relaxed) {
        return;
    }
    let dp = match module!("PredictiveBack").invoke("getDeviceCornerRadius", std::vec![]) {
        WhiskerValue::Float(f) => f as f32,
        WhiskerValue::Int(i) => i as f32,
        // Non-numeric: module not ready / wrong platform — retry later.
        _ => return,
    };
    // A non-positive reading is the "host not attached yet" sentinel (the
    // Activity/insets weren't resolvable at router init). Keep the default
    // and retry on the first real gesture rather than latching it.
    if dp <= 0.0 {
        return;
    }
    transition::set_device_corner_radius(dp);
    CORNER_RADIUS_INSTALLED.store(true, Ordering::Relaxed);
}

/// Read `swipeEdge` (0 = left, 1 = right) from a back-event payload,
/// defaulting to left.
fn back_edge(payload: &WhiskerValue) -> SwipeEdge {
    let WhiskerValue::Map(fields) = payload else {
        return SwipeEdge::Left;
    };
    match fields.get("swipeEdge") {
        Some(WhiskerValue::Int(v)) => SwipeEdge::from_android(*v),
        Some(WhiskerValue::Float(v)) => SwipeEdge::from_android(*v as i64),
        _ => SwipeEdge::Left,
    }
}

/// Asserts main-thread-only access to `inner`. The native `PredictiveBack`
/// module fires on the Android UI thread, the same thread as the Whisker
/// main loop, so the unsafe `Send + Sync` is sound for this single source.
/// Never expose this beyond the gesture module.
struct MainThreadOnly<T> {
    inner: T,
}
// Safety: see the type-level comment.
unsafe impl<T> Send for MainThreadOnly<T> {}
unsafe impl<T> Sync for MainThreadOnly<T> {}
