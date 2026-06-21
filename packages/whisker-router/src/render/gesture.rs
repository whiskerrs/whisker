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
use crate::render::transition::{self, Role};

/// DIAG (temporary): a visible UI marker that the gesture components flip,
/// so their body / `on_mount` execution can be confirmed by screenshot on
/// Android (where the Rust→logcat path is unreliable). router-smoke renders
/// [`DiagMarker`] which reads [`diag::marker`].
pub(crate) mod diag {
    use std::cell::Cell;

    use whisker::runtime::reactive::Owner;
    use whisker::{ReadSignal, RwSignal, computed};

    thread_local! {
        // Three independent flags, minted lazily under a detached root so
        // they outlive the transient gesture-component owners (the
        // arc/arena footgun: a signal minted in a child owner panics once
        // that owner disposes). `Cell<Option<…>>` defers the mint to first
        // use so there's a live runtime.
        static PB_BODY: Cell<Option<RwSignal<bool>>> = const { Cell::new(None) };
        static PB_MOUNT: Cell<Option<RwSignal<bool>>> = const { Cell::new(None) };
        static SWIPE_BODY: Cell<Option<RwSignal<bool>>> = const { Cell::new(None) };
    }

    fn flag(cell: &'static std::thread::LocalKey<Cell<Option<RwSignal<bool>>>>) -> RwSignal<bool> {
        cell.with(|c| {
            if let Some(s) = c.get() {
                return s;
            }
            let s = Owner::detached_root().with(|| RwSignal::new(false));
            c.set(Some(s));
            s
        })
    }

    /// Mark that `android_predictive_back()`'s body ran.
    pub(crate) fn set_pb_body_ran() {
        flag(&PB_BODY).set(true);
    }
    /// Mark that `android_predictive_back()`'s `on_mount` fired.
    pub(crate) fn set_pb_mount_ran() {
        flag(&PB_MOUNT).set(true);
    }
    /// Mark that `swipe_back()`'s body ran.
    pub(crate) fn set_swipe_body_ran() {
        flag(&SWIPE_BODY).set(true);
    }

    /// A reactive one-line status string for the on-screen marker.
    pub fn marker() -> ReadSignal<String> {
        let pb_body = flag(&PB_BODY);
        let pb_mount = flag(&PB_MOUNT);
        let swipe_body = flag(&SWIPE_BODY);
        computed(move || {
            let yn = |b: bool| if b { "Y" } else { "n" };
            std::format!(
                "DIAG pb_body={} pb_mount={} swipe_body={}",
                yn(pb_body.get()),
                yn(pb_mount.get()),
                yn(swipe_body.get()),
            )
        })
    }
}

/// DIAG (temporary): a small on-screen banner showing whether each gesture
/// component's body / `on_mount` ran — so Android body execution can be
/// confirmed by screenshot (the Rust→logcat path being unreliable there).
/// Place it inside `Router` in router-smoke; remove with the rest of the
/// diagnostics once the Android wiring is fixed.
#[component]
pub fn diag_marker() -> Element {
    let label = diag::marker();
    render! {
        view(style: "position: absolute; top: 8px; left: 8px; right: 8px; \
                     z-index: 999; background-color: #FF00FF; padding: 4px;") {
            text(value: label, style: "color: #FFFFFF; font-size: 12px;")
        }
    }
}

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
    // DIAG (temporary): compare against AndroidPredictiveBack — does THIS
    // sibling component's body run on Android?
    pb_log("swipe_back() body ENTER");
    diag::set_swipe_body_ran();
    let nav = use_navigator();
    // Bind to the router's screen-spanning root (a phantom slot has no
    // extent and would never be hit by a touch).
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
            // top controller (shared with Android predictive-back).
            let Some(bridge) = begin(&nav) else {
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

/// Start a back gesture on the deepest active stack: validate it can pop
/// and its transition supports an edge gesture, point **both** wrappers at
/// the top controller (`Top` / `Under`), and return the bridge. `None`
/// means no gesture should begin.
pub(crate) fn begin(nav: &RouterHandle) -> Option<StackBridge> {
    let bridge = nav.active_stack_bridge()?;
    if !bridge.can_back || !transition::supports_edge_swipe(bridge.transition) {
        return None;
    }
    if let (Some(ctrl), Some(top), Some(under)) =
        (&bridge.top_ctrl, &bridge.top_pose, &bridge.under_pose)
    {
        point(top, ctrl, Role::Top);
        point(under, ctrl, Role::Under);
    }
    Some(bridge)
}

/// Scrub the pair to `back_progress` (0 = top present, 1 = swiped away) by
/// setting the shared top controller's progress; both wrappers re-pose via
/// their pose bindings (Top reads `1-back`, Under reads `1-back` too).
pub(crate) fn scrub(bridge: &StackBridge, back_progress: f32) {
    if let Some(ctrl) = &bridge.top_ctrl {
        ctrl.set_value(1.0 - back_progress);
    }
}

/// Settle a back gesture on release. `commit` drives the top off-screen
/// (progress → 0) and calls `navigator.back()` on finish (the same
/// reconcile pop); cancel springs it back to present. `velocity` (progress
/// units/sec) is the release hand-off; `None` uses a plain run (Android's
/// system back carries no velocity).
pub(crate) fn settle(
    nav: &RouterHandle,
    bridge: &StackBridge,
    commit: bool,
    velocity: Option<f32>,
) {
    let Some(ctrl) = bridge.top_ctrl.clone() else {
        return;
    };
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
        // in lockstep.
        match velocity {
            Some(v) => ctrl.forward_with_velocity(v),
            None => ctrl.forward(),
        }
    }
}

/// Point a wrapper's pose binding at controller `c` playing `role`.
fn point(binding: &PoseBinding, c: &AnimationController, role: Role) {
    binding.ctrl.set(c.clone());
    binding.role.set(role);
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
    // DIAG (temporary): the FIRST statement, before any context lookup.
    pb_log("android_predictive_back() body ENTER");

    // DIAG (temporary, LOGGING-INDEPENDENT): flip a visible UI marker so
    // the component's body execution can be confirmed by screenshot on
    // Android, where the Rust→logcat path is unreliable. router-smoke
    // renders `diag_pb_marker()` somewhere on screen.
    diag::set_pb_body_ran();

    let nav = use_navigator();
    let module = module!("PredictiveBack");
    pb_log("android_predictive_back() got navigator + module — subscribing");

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
        module.on_event("backStarted", move |_payload| {
            pb_log("event: backStarted");
            // Capture `shared` whole (not `shared.inner`) so Rust 2021
            // disjoint captures carry its `Send + Sync` impl.
            let shared = &shared;
            let (nav, state) = &shared.inner;
            let bridge = begin(nav);
            pb_log(&format!("backStarted -> begin() = {}", bridge.is_some()));
            *state.borrow_mut() = bridge;
        })
    };
    log_sub("backStarted", started.error());

    let progressed = {
        let shared = MainThreadOnly {
            inner: state.clone(),
        };
        module.on_event("backProgressed", move |payload| {
            let shared = &shared;
            let state = &shared.inner;
            let p = back_progress(&payload);
            pb_log(&format!("event: backProgressed progress={p}"));
            if let Some(bridge) = state.borrow().as_ref() {
                scrub(bridge, p);
            }
        })
    };
    log_sub("backProgressed", progressed.error());

    let cancelled = {
        let shared = MainThreadOnly {
            inner: (nav.clone(), state.clone()),
        };
        module.on_event("backCancelled", move |_payload| {
            pb_log("event: backCancelled");
            let shared = &shared;
            let (nav, state) = &shared.inner;
            if let Some(bridge) = state.borrow_mut().take() {
                settle(nav, &bridge, /* commit = */ false, None);
            }
        })
    };
    log_sub("backCancelled", cancelled.error());

    let invoked = {
        let shared = MainThreadOnly {
            inner: (nav.clone(), state.clone()),
        };
        module.on_event("backInvoked", move |_payload| {
            pb_log("event: backInvoked");
            let shared = &shared;
            let (nav, state) = &shared.inner;
            match state.borrow_mut().take() {
                // Interactive path (API 34+): a gesture was in flight, so
                // commit it (animate the top off, then `back()`).
                Some(bridge) => settle(nav, &bridge, /* commit = */ true, None),
                // No preview (API < 34, or a discrete press): just pop.
                None => {
                    pb_log("backInvoked with no in-flight gesture -> nav.back()");
                    nav.back();
                }
            }
        })
    };
    log_sub("backInvoked", invoked.error());

    // DIAG (temporary): confirm on_mount fires too (vs only the body).
    on_mount(|| {
        pb_log("android_predictive_back() on_mount fired");
        diag::set_pb_mount_ran();
    });

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

/// Read `progress` (0..1, back-direction) from a `backProgressed` payload.
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

/// DIAG (temporary): report whether a subscription registered. An `ok`
/// log confirms the Rust side fired `on_event` (which is what triggers the
/// module's `OnStartObserving` on the Kotlin side); a `FAILED` localises a
/// registration failure.
fn log_sub(event: &str, error: Option<&str>) {
    match error {
        Some(err) => pb_log(&format!("subscribe {event}: FAILED: {err}")),
        None => pb_log(&format!("subscribe {event}: ok")),
    }
}

/// DIAG (temporary): emit a predictive-back trace line that is visible in
/// **logcat on Android** under the `WhiskerPB` tag — the same tag the
/// Kotlin half uses, so one `adb logcat -s WhiskerPB` shows both sides.
///
/// `eprintln!` is NOT reliably forwarded to logcat on Android (the
/// dev-runtime's stdout/stderr → logcat pipe only runs under the
/// `hot-reload` feature, and not in every build), which left the Rust side
/// of this gesture a black box. On Android this routes through the bridge's
/// `whisker_bridge_log_info` (a guaranteed-linked C symbol that calls
/// `__android_log_print` — "the debug path that survives Android's
/// stderr-is-dropped policy"), so the trace always lands. On non-Android it
/// falls back to `eprintln!`.
fn pb_log(msg: &str) {
    #[cfg(target_os = "android")]
    {
        // Declared here (not imported) so whisker-router needs no new dep:
        // the symbol is exported by `whisker-driver-sys`'s bridge and is
        // present in the final linked binary.
        unsafe extern "C" {
            fn whisker_bridge_log_info(
                tag: *const std::os::raw::c_char,
                msg: *const std::os::raw::c_char,
            );
        }
        let tag = b"WhiskerPB\0";
        let mut buf = Vec::with_capacity(msg.len() + 1);
        buf.extend_from_slice(msg.as_bytes());
        buf.push(0);
        unsafe {
            whisker_bridge_log_info(tag.as_ptr() as *const _, buf.as_ptr() as *const _);
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        eprintln!("[pb] {msg}");
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
