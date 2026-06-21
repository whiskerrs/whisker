//! [`SwipeBack`] — the iOS edge swipe-back gesture, rebuilt against the
//! new core + the continuous animation engine.
//!
//! Mount it as a child of the [`Router`](crate::render::Router) (or any
//! element that spans the screen). On a leading-edge horizontal drag it
//! points **both** the top and the revealed-under wrapper at the top
//! stack's transition controller (as `Top` / `Under`) and **scrubs that
//! one controller** with `set_value` (1.0 → 0.0 as the finger moves
//! right) — so the exact same coordinated two-screen model the
//! non-interactive pop uses drives the drag: the top tracks the finger
//! while the screen beneath slides back from covered to rest. On release
//! it hands off with the finger's velocity
//! ([`reverse_with_velocity`](whisker::AnimationController::reverse_with_velocity)
//! to commit, [`forward_with_velocity`](whisker::AnimationController::forward_with_velocity)
//! to cancel); a commit calls `navigator.back()` on finish, which runs the
//! same reconcile pop and unmounts the popped entry.
//!
//! This is the generalised form of the old `ios_swipe_back.rs`, but the
//! intermediate 0..1 lives in a real signal (the controller's progress)
//! and both screens are posed by the runtime's pose effects — no
//! hand-written per-frame inline transform.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::event::TouchEvent;
use whisker::runtime::event::bind_typed;
use whisker::runtime::reactive::on_mount;
use whisker::runtime::view::{BindType, Element};
use whisker::{component, render, use_context};

use crate::render::components::RouterRoot;
use crate::render::handle::{PoseBinding, RouterHandle, StackBridge, use_navigator};
use crate::render::transition::{self, Role};

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
            // Only swipe when the deepest active stack can pop and its
            // transition supports an edge swipe (horizontal slide).
            let Some(bridge) = nav.active_stack_bridge() else {
                return;
            };
            if !bridge.can_back || !transition::supports_edge_swipe(bridge.transition) {
                return;
            }
            // Point BOTH wrappers at the top controller (Top / Under) so
            // one scrubbed progress drives the coordinated pair — the
            // same model the non-interactive pop uses.
            if let (Some(ctrl), Some(top), Some(under)) =
                (&bridge.top_ctrl, &bridge.top_pose, &bridge.under_pose)
            {
                point(top, ctrl, Role::Top);
                point(under, ctrl, Role::Under);
            }
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

            let Some(ctrl) = state.bridge.top_ctrl.clone() else {
                return;
            };

            if commit {
                // Drive the top off-screen (progress → 0) with momentum;
                // both wrappers follow via their pose bindings. On finish
                // call `back()`, which runs the same coordinated pop and
                // unmounts the popped entry (its controller is already at
                // 0, so the reconcile's reverse settles instantly).
                let nav = nav.clone();
                let done = Rc::new(RefCell::new(false));
                ctrl.on_finish(move |finished| {
                    if finished && !*done.borrow() {
                        *done.borrow_mut() = true;
                        nav.back();
                    }
                });
                ctrl.reverse_with_velocity(velocity);
            } else {
                // Cancel: spring the top back to fully present; the under
                // wrapper, pointed at the same controller as `Under`,
                // slides back to covered in lockstep.
                ctrl.forward_with_velocity(velocity);
            }
        });
    }
}

/// Scrub the pair to `back_progress` (0 = top present, 1 = swiped away) by
/// setting the shared top controller's progress; both wrappers re-pose via
/// their pose bindings (Top reads `1-back`, Under reads `1-back` too).
fn scrub(bridge: &StackBridge, back_progress: f32) {
    if let Some(ctrl) = &bridge.top_ctrl {
        ctrl.set_value(1.0 - back_progress);
    }
}

/// Point a wrapper's pose binding at controller `c` playing `role`.
fn point(binding: &PoseBinding, c: &whisker::AnimationController, role: Role) {
    binding.ctrl.set(c.clone());
    binding.role.set(role);
}
