//! [`SwipeBack`] — the iOS edge swipe-back gesture, rebuilt against the
//! new core + the continuous animation engine.
//!
//! Mount it as a child of the [`Router`](crate::render::Router) (or any
//! element that spans the screen). On a leading-edge horizontal drag it
//! **scrubs the top stack's transition controller** with `set_value`
//! (1.0 → 0.0 as the finger moves right), so the top screen tracks the
//! finger and the screen beneath is revealed. On release it hands off to
//! the controller with the finger's velocity
//! ([`reverse_with_velocity`](whisker::AnimationController::reverse_with_velocity)
//! to commit, [`forward_with_velocity`](whisker::AnimationController::forward_with_velocity)
//! to cancel) for a natural spring-to-rest, then calls
//! `navigator.back()` once a commit animation finishes.
//!
//! This is the generalised form of the old `ios_swipe_back.rs`, but the
//! intermediate 0..1 lives in a real signal (the controller's progress),
//! not a hand-written per-frame inline transform.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::event::TouchEvent;
use whisker::runtime::event::bind_typed;
use whisker::runtime::reactive::on_mount;
use whisker::runtime::view::{BindType, Element, set_inline_styles};
use whisker::{component, render, use_context};

use crate::render::components::RouterRoot;
use crate::render::handle::{RouterHandle, StackBridge, use_navigator};
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
            let under = state.bridge.under_wrapper;
            let transition = state.bridge.transition;

            if commit {
                // Drive the top off-screen (progress → 0 in our pose
                // convention) with momentum, then pop on finish.
                let nav = nav.clone();
                let done = Rc::new(RefCell::new(false));
                ctrl.on_finish(move |finished| {
                    if finished && !*done.borrow() {
                        *done.borrow_mut() = true;
                        // Reveal-side settles to rest before the pop.
                        if let Some(u) = under {
                            let (t, o) = transition::pose(transition, Role::Under, 0.0);
                            set_inline_styles(u, &under_style(t, o));
                        }
                        nav.back();
                    }
                });
                ctrl.reverse_with_velocity(velocity);
            } else {
                // Cancel: spring the top back to fully present.
                if let Some(u) = under {
                    let (t, o) = transition::pose(transition, Role::Under, 1.0);
                    set_inline_styles(u, &under_style(t, o));
                }
                ctrl.forward_with_velocity(velocity);
            }
        });
    }
}

/// Scrub both wrappers to `back_progress` (0 = present, 1 = swiped away).
///
/// The top controller's value is "presence of the top" (1 = present), so
/// it is set to `1 - back_progress`; that re-poses the top wrapper via
/// its own pose `computed`. The under wrapper is posed directly (it is
/// not driven by the top controller).
fn scrub(bridge: &StackBridge, back_progress: f32) {
    if let Some(ctrl) = &bridge.top_ctrl {
        ctrl.set_value(1.0 - back_progress);
    }
    if let Some(u) = bridge.under_wrapper {
        let (t, o) = transition::pose(bridge.transition, Role::Under, 1.0 - back_progress);
        set_inline_styles(u, &under_style(t, o));
    }
}

/// Inline style for the revealed under wrapper during a swipe (matches
/// the stack wrapper base, with the scrubbed pose).
fn under_style(transform: String, opacity: f32) -> String {
    format!(
        "position: absolute; left: 0; top: 0; right: 0; bottom: 0; \
         display: flex; flex-direction: column; transform: {transform}; \
         opacity: {opacity};"
    )
}
