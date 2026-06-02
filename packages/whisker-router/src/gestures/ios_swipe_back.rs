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
    animate_cancel, animate_start, component, render, spawn_local, use_context, AnimateOptions,
    ElementHandle, Style,
};

use crate::layouts::stack::{slot_css, StackLayoutHandle};
use crate::transitions::ios_slide;
use crate::transitions::{Direction, Side, StackTransitionBox};

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
/// Fallback used while the async container measurement is in
/// flight — a hair under iPhone 17 Pro's 402pt viewport. The
/// progress derivation reads `container_width` (set by the
/// `boundingClientRect` task that fires on mount and at each
/// touchstart) and only falls back to this constant when the cell
/// is still zero.
const SWIPE_FALLBACK_WIDTH_PX: f32 = 400.0;

/// iOS-style edge swipe-back gesture component.
///
/// Mount as a child of [`StackLayout`](crate::StackLayout); reads
/// the layout handle and the in-context
/// [`RouteStack`](crate::RouteStack) and binds the touch handlers
/// in `on_mount`. Renders no DOM of its own.
#[component]
pub fn ios_swipe_back() -> Element {
    let layout =
        use_context::<StackLayoutHandle>().expect("IosSwipeBack must be a child of StackLayout");
    let transition: StackTransitionBox = StackTransitionBox::new(ios_slide::IosSlide::default());

    on_mount(move || install(&layout, transition.clone()));

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
    // Container width in points — cached from `boundingClientRect`.
    // Lynx exposes the layout box asynchronously so the value
    // arrives a frame after we ask; until then the touchmove
    // handler falls back to `SWIPE_FALLBACK_WIDTH_PX`. We re-fire
    // the measurement on every `touchstart` so orientation changes
    // and split-screen resizes pick up between gestures.
    let container_width: Rc<Cell<f32>> = Rc::new(Cell::new(0.0));
    // `bind_once` ensures we only attach the `ElementRef` once;
    // it's a cheap arena handle and rebinding is harmless, but
    // keeping it pinned to one allocation lets us return the same
    // `ElementHandle` from every `measure()` call.
    let container_ref = ElementHandle::new();
    container_ref.r().bind(container);

    let measure = {
        let container_width = container_width.clone();
        move || {
            let container_width = container_width.clone();
            spawn_local(async move {
                if let Ok(rect) = container_ref.bounding_client_rect().await {
                    let w = rect.width as f32;
                    if w > 0.0 {
                        container_width.set(w);
                    }
                }
            });
        }
    };
    measure();

    // touchstart — qualify the edge swipe, mount the preview
    // wrapper, capture state.
    {
        let gesture = gesture.clone();
        let preview_wrapper = preview_wrapper.clone();
        let mount_preview = mount_preview.clone();
        let current_wrapper = current_wrapper.clone();
        let transition = transition.clone();
        let measure = measure.clone();
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
            // Re-measure on every touchstart so orientation /
            // split-screen changes between gestures land in
            // `container_width` before the next touchmove tick.
            measure();

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
        let container_width = container_width.clone();
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
            let full = {
                let measured = container_width.get();
                if measured > 0.0 {
                    measured
                } else {
                    SWIPE_FALLBACK_WIDTH_PX
                }
            };
            let progress = (delta / full).clamp(0.0, 1.0);
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
