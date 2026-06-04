// The implementation imports + helper machinery below stay compiled
// on non-iOS targets so the file reads as a single unit, but they
// are dead unless `target_os = "ios"`. Suppress the resulting
// dead-code / unused-import warnings only when not iOS — keeping
// warnings on for the live iOS build.
#![cfg_attr(not(target_os = "ios"), allow(dead_code, unused_imports))]

//! [`IosSwipeBack`] — iOS-style edge swipe-back gesture for
//! [`StackLayout`](crate::StackLayout).
//!
//! Mount as a child of the layout to enable a UIKit-style horizontal
//! swipe from the leading edge: the destination screen slides in
//! from the left with the 30% parallax and brightness dim of
//! [`IosSlide`](crate::IosSlide), the current screen tracks the
//! finger to the right, and release past the half-way point commits
//! the navigation.
//!
//! ```ignore
//! StackLayout(transition: IosSlide::default(), render: render) {
//!     IosSwipeBack()
//! }
//! ```
//!
//! The component is designed to pair with [`IosSlide`] for visual
//! consistency. Pairing it with a different transition (e.g.
//! [`Fade`](crate::Fade)) is allowed but the gesture will still
//! render an iOS slide pose during the drag — what the natural
//! transition does outside the gesture is unaffected.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use whisker::css::ToCss;
use whisker::event::TouchEvent;
use whisker::runtime::event::bind_typed;
use whisker::runtime::reactive::on_mount;
use whisker::runtime::view::{set_inline_styles, BindType, Element};
use whisker::{
    animate_cancel, animate_start, component, render, use_context, AnimateOptions, Style,
};

use crate::layouts::stack::{slot_css, StackLayoutHandle};
use crate::transitions::ios_slide;
use crate::transitions::{Direction, Side, StackTransitionBox};

/// Horizontal distance (in points) that maps the gesture to a full
/// `progress = 1.0`. iPhone 17 Pro is 402pt wide and the wrapper
/// covers the full viewport, so a 1:1 finger-to-edge ratio wants
/// this equal to the viewport width. Older iPhones differ by a few
/// points (375–430pt) — a future revision can read the actual
/// container width to make this exact across devices.
const SWIPE_FULL_DISTANCE_PX: f32 = 402.0;
/// `clientX` (in points) within which a `touchstart` qualifies as
/// an edge swipe-back attempt. iOS uses ~20pt; 24pt is generous
/// without eating into scrollable content.
const SWIPE_EDGE_THRESHOLD_PX: f32 = 24.0;
/// Progress at touch release above which the gesture commits.
const SWIPE_COMMIT_THRESHOLD: f32 = 0.5;
/// Floor on the touchend finish animation duration.
const SWIPE_MIN_FINISH_MS: u32 = 120;
/// Natural finish duration scaled by the remaining distance.
const SWIPE_FULL_FINISH_MS: u32 = 320;

/// iOS-style edge swipe-back gesture component.
///
/// Mount as a child of [`StackLayout`](crate::StackLayout); reads
/// the layout handle and the in-context
/// [`RouteStack`](crate::RouteStack) and binds the touch handlers
/// in `on_mount`. Renders no DOM of its own.
///
/// **Platform**: iOS only. On Android the body is a no-op so
/// cross-platform apps can mount it unconditionally alongside
/// [`AndroidPredictiveBack`] without having to `cfg`-gate the call
/// site. The Android back-gesture surface — including the
/// predictive-back preview — is owned by `AndroidPredictiveBack`
/// (which is itself a no-op on iOS).
///
/// The original Android failure mode this guard exists to prevent:
/// the container-level `touchstart` handler the iOS gesture installs
/// catches any tap whose `clientX` is within
/// `SWIPE_EDGE_THRESHOLD_PX` of the leading edge — wide enough to
/// include the leading edge of a back-chevron icon. The handler
/// then synchronously mounts a preview wrapper (running the
/// previous route's body and inserting a fresh subtree into the
/// container) DURING the touch dispatch. Lynx on Android does not
/// tolerate the mid-dispatch view-tree mutation and aborts the
/// process — the in-app back button visibly closes the app. The
/// cfg gate sidesteps the whole class of failure on Android.
#[component]
pub fn ios_swipe_back() -> Element {
    // The handler install path is alive only on iOS. Suppress the
    // `use_context` / `on_mount` calls on every other target so we
    // don't pay the bind cost or risk the cross-platform tap-versus-
    // touchstart collision described above. The Android side already
    // has [`AndroidPredictiveBack`] for the same role.
    #[cfg(target_os = "ios")]
    {
        let layout = use_context::<StackLayoutHandle>()
            .expect("IosSwipeBack must be a child of StackLayout");
        let transition: StackTransitionBox =
            StackTransitionBox::new(ios_slide::IosSlide::default());
        on_mount(move || install(&layout, transition.clone()));
    }

    // No visible output — gesture components only attach handlers.
    render! { fragment() }
}

/// Per-active-gesture state stored in the `gesture` cell. `None`
/// means no swipe is in flight; the touchstart handler populates
/// it, touchmove updates it, touchend drains it.
struct GestureState {
    start_x: f32,
    progress: f32,
    identifier: i64,
}

fn install(layout: &StackLayoutHandle, transition: StackTransitionBox) {
    let container = layout.container;
    let mount_preview = layout.mount_preview.clone();
    let dispose_preview = layout.dispose_preview.clone();
    let commit_back = layout.commit_preview_and_back.clone();
    let current_wrapper = layout.current_wrapper.clone();

    let gesture: Rc<RefCell<Option<GestureState>>> = Rc::new(RefCell::new(None));
    // Cached preview wrapper between touchstart and the end event
    // — lets touchmove pose it without going through
    // `current_wrapper` every frame.
    let preview_wrapper: Rc<Cell<Option<Element>>> = Rc::new(Cell::new(None));

    // touchstart — qualify the edge swipe, mount the preview
    // wrapper, capture state.
    {
        let gesture = gesture.clone();
        let preview_wrapper = preview_wrapper.clone();
        let mount_preview = mount_preview.clone();
        let current_wrapper = current_wrapper.clone();
        let transition = transition.clone();
        bind_typed::<TouchEvent, _>(container, "touchstart", BindType::Bind, move |e| {
            if gesture.borrow().is_some() {
                return;
            }
            let touch = match e
                .changed_touches
                .first()
                .copied()
                .or_else(|| e.touches.first().copied())
            {
                Some(t) => t,
                None => return,
            };
            let start_x = touch.client_x as f32;
            if start_x > SWIPE_EDGE_THRESHOLD_PX {
                return;
            }

            let wrapper = mount_preview();
            apply_pose(
                wrapper,
                transition.0.as_ref(),
                Side::Incoming,
                Direction::Backward,
                0.0,
            );
            if let Some(cur) = current_wrapper() {
                apply_pose(
                    cur,
                    transition.0.as_ref(),
                    Side::Outgoing,
                    Direction::Backward,
                    0.0,
                );
            }

            preview_wrapper.set(Some(wrapper));
            *gesture.borrow_mut() = Some(GestureState {
                start_x,
                progress: 0.0,
                identifier: touch.identifier,
            });
        });
    }

    // touchmove — translate finger displacement into progress and
    // re-pose preview + current each frame.
    {
        let gesture = gesture.clone();
        let preview_wrapper = preview_wrapper.clone();
        let current_wrapper = current_wrapper.clone();
        let transition = transition.clone();
        bind_typed::<TouchEvent, _>(container, "touchmove", BindType::Bind, move |e| {
            let mut g = gesture.borrow_mut();
            let state = match g.as_mut() {
                Some(s) => s,
                None => return,
            };
            let touch = match e
                .touches
                .iter()
                .find(|t| t.identifier == state.identifier)
                .or_else(|| {
                    e.changed_touches
                        .iter()
                        .find(|t| t.identifier == state.identifier)
                }) {
                Some(t) => *t,
                None => return,
            };
            let delta = (touch.client_x as f32 - state.start_x).max(0.0);
            let progress = (delta / SWIPE_FULL_DISTANCE_PX).clamp(0.0, 1.0);
            state.progress = progress;

            if let Some(wrapper) = preview_wrapper.get() {
                apply_pose(
                    wrapper,
                    transition.0.as_ref(),
                    Side::Incoming,
                    Direction::Backward,
                    progress,
                );
            }
            if let Some(cur) = current_wrapper() {
                apply_pose(
                    cur,
                    transition.0.as_ref(),
                    Side::Outgoing,
                    Direction::Backward,
                    progress,
                );
            }
        });
    }

    // touchend / touchcancel — drive the finish through Lynx's
    // animator (short duration, `fill: forwards`). Inline writes
    // during a string of rapid touch-move updates were getting
    // dropped/coalesced — the animator path always takes hold.
    for end_name in ["touchend", "touchcancel"] {
        let gesture = gesture.clone();
        let preview_wrapper = preview_wrapper.clone();
        let current_wrapper = current_wrapper.clone();
        let dispose_preview = dispose_preview.clone();
        let commit_back = commit_back.clone();
        bind_typed::<TouchEvent, _>(container, end_name, BindType::Bind, move |_e| {
            let state = match gesture.borrow_mut().take() {
                Some(s) => s,
                None => return,
            };
            let commit = state.progress >= SWIPE_COMMIT_THRESHOLD;
            let target_progress: f32 = if commit { 1.0 } else { 0.0 };

            let from = state.progress;
            let remaining = (target_progress - from).abs();
            let duration_ms =
                ((remaining * SWIPE_FULL_FINISH_MS as f32) as u32).max(SWIPE_MIN_FINISH_MS);

            if let Some(wrapper) = preview_wrapper.get() {
                animate_to_pose(
                    wrapper,
                    Side::Incoming,
                    Direction::Backward,
                    from,
                    target_progress,
                    duration_ms,
                    "swipe-finish-incoming",
                );
            }
            if let Some(cur) = current_wrapper() {
                animate_to_pose(
                    cur,
                    Side::Outgoing,
                    Direction::Backward,
                    from,
                    target_progress,
                    duration_ms,
                    "swipe-finish-outgoing",
                );
            }

            preview_wrapper.set(None);
            if commit {
                commit_back();
            } else {
                dispose_preview();
            }
        });
    }
}

/// Cancel any natural-transition animation that might still be
/// holding `element` at its end pose via `fill: forwards`.
///
/// Without this, inline `transform` set during a drag is overridden
/// by the prior animation rule's computed style and the element
/// won't move at all. Lynx no-ops on cancel-of-nonexistent so the
/// shotgun-cancel is cheap.
fn clear_natural_animations(element: Element) {
    for name in [
        "stack-ios-incoming-forward",
        "stack-ios-incoming-backward",
        "stack-ios-outgoing-forward",
        "stack-ios-outgoing-backward",
        "swipe-finish-incoming",
        "swipe-finish-outgoing",
    ] {
        let _ = animate_cancel(element, name);
    }
}

/// Apply the iOS slide pose for `progress` to `element` as an
/// inline style, alongside the transition's [`slot_style`] and the
/// layout's base [`slot_css`].
///
/// Inline styles take effect only once any active CSS animation is
/// cancelled — Lynx's animator with `fill: forwards` (which the
/// natural push/pop uses) clamps the element to its end pose and
/// shadows out-of-band style writes.
fn apply_pose(
    element: Element,
    transition: &dyn crate::transitions::StackTransition,
    side: Side,
    direction: Direction,
    progress: f32,
) {
    clear_natural_animations(element);
    let mut style = slot_css().to_css_string();
    let decoration = match transition.slot_style(side, direction) {
        Style::Static(s) => s,
        Style::Dynamic(_) => String::new(),
    };
    if !decoration.is_empty() {
        style.push_str(&decoration);
    }
    for (prop, val) in ios_slide::pose(side, direction, progress) {
        style.push_str(&format!("{prop}: {val};"));
    }
    set_inline_styles(element, &style);
}

/// Animate `element` from the pose at `from_progress` to the pose
/// at `to_progress` using Lynx's animator. `fill: forwards` keeps
/// the element at the end pose after the animation completes; the
/// caller doesn't have to listen for `animationend`.
#[allow(clippy::too_many_arguments)]
fn animate_to_pose(
    element: Element,
    side: Side,
    direction: Direction,
    from_progress: f32,
    to_progress: f32,
    duration_ms: u32,
    animation_name: &'static str,
) {
    let from = ios_slide::pose(side, direction, from_progress);
    let to = ios_slide::pose(side, direction, to_progress);
    if from.is_empty() || to.is_empty() {
        return;
    }
    let from_kf: Vec<(&str, &str)> = from.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let to_kf: Vec<(&str, &str)> = to.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let _ = animate_start(
        element,
        animation_name,
        &[("0%", &from_kf), ("100%", &to_kf)],
        &AnimateOptions {
            duration_ms,
            easing: "ease-out".into(),
            fill: "forwards".into(),
            ..Default::default()
        },
    );
}
