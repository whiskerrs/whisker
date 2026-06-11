//! [`IosSwipeBack`] — iOS-style edge swipe-back gesture for
//! [`StackLayout`](crate::StackLayout).
//!
//! Mount as a child of the layout to enable a UIKit-style horizontal
//! swipe from the leading edge: the destination screen slides in
//! from the left with [`IosSlide`](crate::IosSlide)'s 30% parallax
//! and brightness dim, the current screen tracks the finger to the
//! right, and release past the half-way point commits the back
//! navigation.
//!
//! ```ignore
//! StackLayout(transition: IosSlide::default(), render: render.into()) {
//!     IosSwipeBack()
//! }
//! ```
//!
//! Designed to pair with [`IosSlide`](crate::IosSlide) for visual
//! consistency. Pairing with a different transition (e.g.
//! [`Fade`](crate::Fade)) is allowed — the gesture renders the iOS
//! slide pose during the drag regardless, and the natural transition
//! is unaffected outside the gesture.

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

// 1:1 finger-to-edge ratio wants this equal to the viewport width.
// 402pt is iPhone 17 Pro; older iPhones differ by 375-430pt. A
// future revision can read the actual container width.
const SWIPE_FULL_DISTANCE_PX: f32 = 402.0;
// `clientX` within which a touchstart qualifies as an edge swipe.
// iOS uses ~20pt; 24pt is generous without eating into scroll.
const SWIPE_EDGE_THRESHOLD_PX: f32 = 24.0;
// Progress at touch release above which the gesture commits.
const SWIPE_COMMIT_THRESHOLD: f32 = 0.5;
// Touchend finish animation duration floor.
const SWIPE_MIN_FINISH_MS: u32 = 120;
// Natural finish duration scaled by remaining distance.
const SWIPE_FULL_FINISH_MS: u32 = 320;

/// iOS-style edge swipe-back gesture component for
/// [`StackLayout`](crate::StackLayout).
///
/// Renders no DOM of its own; reads the
/// [`StackLayoutHandle`](crate::StackLayoutHandle) from context and
/// binds touch handlers in `on_mount`. See the [module docs](self)
/// for the user-facing summary.
#[component]
pub fn ios_swipe_back() -> Element {
    let layout =
        use_context::<StackLayoutHandle>().expect("IosSwipeBack must be a child of StackLayout");
    let transition: StackTransitionBox = StackTransitionBox::new(ios_slide::IosSlide::default());

    on_mount(move || install(&layout, transition.clone()));

    render! { fragment() }
}

// `None` = no swipe in flight; touchstart populates, touchmove
// updates, touchend drains.
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
    // Cache the preview wrapper across the gesture so touchmove
    // doesn't have to re-resolve through `current_wrapper` per frame.
    let preview_wrapper: Rc<Cell<Option<Element>>> = Rc::new(Cell::new(None));

    // touchstart — qualify the edge swipe and capture state.
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

            // `None` = stack at the root, no back-target to reveal —
            // abort the gesture.
            let Some(wrapper) = mount_preview() else {
                return;
            };
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

    // touchmove — finger delta → progress, re-pose each frame.
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
    // animator (short duration, fill: forwards). Inline writes get
    // dropped/coalesced under rapid touchmoves; the animator path
    // always takes hold.
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

// Cancel any prior natural-transition animation. Without this, the
// previous animation's `fill: forwards` keeps the element at its end
// pose and shadows inline writes — the drag would appear frozen.
// Lynx no-ops on cancel-of-nonexistent, so the shotgun-cancel is
// cheap.
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

// Write the iOS slide pose at `progress` inline, alongside the
// transition's `slot_style` and the layout's base `slot_css`. Inline
// styles only take effect after the active CSS animation is
// cancelled (see `clear_natural_animations`).
fn apply_pose(
    element: Element,
    transition: &dyn crate::transitions::StackTransition,
    side: Side,
    direction: Direction,
    progress: f32,
) {
    clear_natural_animations(element);
    // Incoming == the screen being revealed (the new top once the
    // swipe commits) → `relative` so its children stay hit-testable;
    // the outgoing screen sliding out stays `absolute`. Matches the
    // top/non-top split in `StackLayout`'s `slot_css`.
    let mut style = slot_css(matches!(side, Side::Incoming)).to_css_string();
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

// Animate `element` from the pose at `from_progress` to the pose at
// `to_progress` using Lynx's animator. `fill: forwards` keeps the
// element at the end pose without an `animationend` listener.
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
        &AnimateOptions::new()
            .duration_ms(duration_ms)
            .easing("ease-out")
            .fill("forwards"),
    );
}
