//! [`IosSlide`] — UINavigationController-style horizontal slide
//! with parallax, leading-edge shadow on the moving screen, and a
//! subtle brightness dim on the parallaxed background screen.
//!
//! Default transition for [`StackLayout`](crate::StackLayout). Also
//! installs an iOS-native edge swipe-back gesture via
//! [`StackTransition::install_gestures`] — the gesture is intrinsic
//! to this transition rather than the layout, so swapping in a
//! [`Fade`](super::Fade) or [`Instant`](super::Instant) transition
//! drops swipe-back automatically.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use whisker::event::TouchEvent;
use whisker::runtime::event::bind_typed;
use whisker::runtime::view::{set_inline_styles, BindType, Element};
use whisker::{animate_cancel, animate_start, AnimateOptions, Style};

use super::{Direction, GestureContext, Side, StackTransition};

/// One animation's CSS endpoints — `(name, from_props, to_props)`.
type Keyframes<'a> = (
    &'static str,
    Vec<(&'static str, &'a str)>,
    Vec<(&'static str, &'a str)>,
);

/// Outgoing screen translates ~30% of viewport toward the leading
/// edge during a horizontal push — UIKit's parallax amount.
pub const IOS_PARALLAX_PCT: f32 = 30.0;

/// Depth shadow declared as static inline style on the foreground
/// wrapper. Lynx's animator drops `box-shadow` from `@keyframes`
/// rules, so it has to ride along as a `slot_style` declaration
/// rather than an animated property. 5-part syntax is required.
const IOS_LEADING_SHADOW: &str = "box-shadow: -4px 0px 16px 0px rgba(0, 0, 0, 0.06);";

/// Parallaxed (background) screen sits one notch dimmer than fully
/// lit. Carried via `slot_style` as the initial state; the matching
/// `animate` interpolates `filter: brightness(...)` between this
/// and `brightness(1.0)` so the screen brightens as it returns to
/// the foreground (or dims as it leaves).
const IOS_PARALLAX_DIM: &str = "filter: brightness(0.85);";

const DEFAULT_DURATION_MS: u32 = 320;
const DEFAULT_EASING: &str = "ease-in-out";

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

/// iOS UINavigationController-style horizontal slide.
#[derive(Copy, Clone, Debug)]
pub struct IosSlide {
    /// Duration in milliseconds (default 320ms).
    pub duration_ms: u32,
    /// Easing function string (default `"ease-in-out"`).
    pub easing: &'static str,
}

impl Default for IosSlide {
    fn default() -> Self {
        Self {
            duration_ms: DEFAULT_DURATION_MS,
            easing: DEFAULT_EASING,
        }
    }
}

impl StackTransition for IosSlide {
    fn animate(&self, element: Element, side: Side, direction: Direction) {
        let parallax = format!("translateX(-{IOS_PARALLAX_PCT}%)");
        let (name, from, to): Keyframes = match (side, direction) {
            (Side::Incoming, Direction::Forward) => (
                "stack-ios-incoming-forward",
                vec![("transform", "translateX(100%)")],
                vec![("transform", "translateX(0%)")],
            ),
            (Side::Incoming, Direction::Backward) => (
                "stack-ios-incoming-backward",
                vec![
                    ("transform", parallax.as_str()),
                    ("filter", "brightness(0.85)"),
                ],
                vec![
                    ("transform", "translateX(0%)"),
                    ("filter", "brightness(1.0)"),
                ],
            ),
            (Side::Outgoing, Direction::Forward) => (
                "stack-ios-outgoing-forward",
                vec![
                    ("transform", "translateX(0%)"),
                    ("filter", "brightness(1.0)"),
                ],
                vec![
                    ("transform", parallax.as_str()),
                    ("filter", "brightness(0.85)"),
                ],
            ),
            (Side::Outgoing, Direction::Backward) => (
                "stack-ios-outgoing-backward",
                vec![("transform", "translateX(0%)")],
                vec![("transform", "translateX(100%)")],
            ),
            (_, Direction::None) => return,
        };
        let _ = animate_start(
            element,
            name,
            &[("0%", &from), ("100%", &to)],
            &AnimateOptions {
                duration_ms: self.duration_ms,
                easing: self.easing.into(),
                fill: "forwards".into(),
                ..Default::default()
            },
        );
    }

    fn slot_style(&self, side: Side, direction: Direction) -> Style {
        let raw = match (side, direction) {
            (Side::Incoming, Direction::Forward) => IOS_LEADING_SHADOW,
            (Side::Outgoing, Direction::Backward) => IOS_LEADING_SHADOW,
            (Side::Outgoing, Direction::Forward) => IOS_PARALLAX_DIM,
            (Side::Incoming, Direction::Backward) => IOS_PARALLAX_DIM,
            _ => "",
        };
        Style::from(raw)
    }

    fn pose(&self, side: Side, direction: Direction, progress: f32) -> Vec<(&'static str, String)> {
        // Endpoints per `(side, direction)` mirror `animate()`'s
        // keyframes: `(tx_from, tx_to, br_from, br_to)`. Linear
        // interpolation in CSS-property space.
        let (tx_from, tx_to, br_from, br_to) = match (side, direction) {
            (Side::Incoming, Direction::Forward) => (100.0, 0.0, 1.0, 1.0),
            (Side::Incoming, Direction::Backward) => (-IOS_PARALLAX_PCT, 0.0, 0.85, 1.0),
            (Side::Outgoing, Direction::Forward) => (0.0, -IOS_PARALLAX_PCT, 1.0, 0.85),
            (Side::Outgoing, Direction::Backward) => (0.0, 100.0, 1.0, 1.0),
            (_, Direction::None) => return Vec::new(),
        };
        let t = progress.clamp(0.0, 1.0);
        let tx = tx_from + (tx_to - tx_from) * t;
        let br = br_from + (br_to - br_from) * t;
        vec![
            ("transform", format!("translateX({tx}%)")),
            ("filter", format!("brightness({br})")),
        ]
    }

    fn install_gestures(&self, ctx: &GestureContext) {
        let container = ctx.container;
        let can_back = ctx.can_back.clone();
        let mount_preview = ctx.mount_preview.clone();
        let dispose_preview = ctx.dispose_preview.clone();
        let commit_back = ctx.commit_preview_and_back.clone();
        let current_wrapper = ctx.current_wrapper.clone();
        let transition = ctx.transition.clone();

        // Per-active-gesture state. `None` means no swipe is in
        // flight; the touchstart handler populates it, touchmove
        // updates it, touchend drains it.
        let gesture: Rc<RefCell<Option<GestureState>>> = Rc::new(RefCell::new(None));
        // Cached preview wrapper between touchstart and the end
        // event — let touchmove pose it without going through
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
            let can_back = can_back.clone();
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
                if !can_back() {
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
                    // The current wrapper takes the outgoing role for
                    // a backward navigation — restyle so direction-
                    // dependent decoration (depth shadow) is in place
                    // before any pose update.
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

        // touchmove — translate finger displacement into progress
        // and re-pose preview + current each frame.
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

        // touchend / touchcancel — snap to the final pose and
        // commit-or-cancel immediately. Smooth finishing animations
        // need to wait for animationend, which is unreliable for
        // short / no-op animations; the snap is at least visibly
        // correct and we can layer a finishing tween back on once
        // the basic flow is verified.
        for end_name in ["touchend", "touchcancel"] {
            let gesture = gesture.clone();
            let preview_wrapper = preview_wrapper.clone();
            let current_wrapper = current_wrapper.clone();
            let transition = transition.clone();
            let dispose_preview = dispose_preview.clone();
            let commit_back = commit_back.clone();
            bind_typed::<TouchEvent, _>(container, end_name, BindType::Bind, move |_e| {
                let state = match gesture.borrow_mut().take() {
                    Some(s) => s,
                    None => return,
                };
                let commit = state.progress >= SWIPE_COMMIT_THRESHOLD;
                let target_progress: f32 = if commit { 1.0 } else { 0.0 };

                // Drive the finish via Lynx's animator (short
                // duration, `fill: forwards`) instead of a raw
                // `set_inline_styles` write. Inline writes during a
                // run of rapid touch-move updates were getting
                // dropped/coalesced — the animator path always
                // takes hold.
                let from = state.progress;
                let remaining = (target_progress - from).abs();
                let duration_ms = ((remaining * 320.0) as u32).max(120);

                if let Some(wrapper) = preview_wrapper.get() {
                    animate_to_pose(
                        wrapper,
                        transition.0.as_ref(),
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
                        transition.0.as_ref(),
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
}

/// Per-active-gesture state stored in the `gesture` cell.
struct GestureState {
    start_x: f32,
    progress: f32,
    identifier: i64,
}

/// Cancel any natural-transition animation that might still be
/// holding `element` at its end pose via `fill: forwards`.
///
/// Without this, inline `transform` set during a drag is overridden
/// by the prior animation rule's computed style and the element
/// won't move at all. We don't track which animation is active per
/// wrapper, so this just tries to cancel all four names — Lynx
/// no-ops on cancel-of-nonexistent.
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

/// Slot layout style — must be re-emitted by every `apply_pose`
/// call because `set_inline_styles` replaces the full inline string
/// (it isn't a CSSOM merge). Without these properties, a wrapper
/// styled by the gesture falls back to default static positioning
/// and starts sharing space with its sibling instead of overlaying
/// it — that's what showed up as the "two screens side by side"
/// glitch after a swipe-back commit.
const SLOT_LAYOUT_STYLE: &str = "position: absolute; top: 0; left: 0; \
     width: 100%; height: 100%; overflow: visible;";

/// Apply the dynamic pose for `progress` to `element` as an inline
/// style, alongside the transition's [`slot_style`] decoration and
/// the [`SLOT_LAYOUT_STYLE`] layout base.
///
/// Inline styles take effect only once any active CSS animation is
/// cancelled — Lynx's animator with `fill: forwards` (which the
/// natural push/pop uses) clamps the element to its end pose and
/// shadows out-of-band style writes.
/// Animate `element` from the pose at `from_progress` to the pose
/// at `to_progress` using Lynx's animator. `fill: forwards` keeps
/// the element at the end pose after the animation completes; the
/// caller doesn't have to listen for `animationend`.
///
/// `apply_pose` writes inline styles for the live drag — fast and
/// fine for per-frame updates — but Lynx coalesces a string of
/// rapid inline writes such that the final one (the cancel/commit
/// snap) sometimes never lands. The animator path always takes
/// hold, so the touchend handler uses this instead.
#[allow(clippy::too_many_arguments)]
fn animate_to_pose(
    element: Element,
    transition: &dyn StackTransition,
    side: Side,
    direction: Direction,
    from_progress: f32,
    to_progress: f32,
    duration_ms: u32,
    animation_name: &'static str,
) {
    let from = transition.pose(side, direction, from_progress);
    let to = transition.pose(side, direction, to_progress);
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

fn apply_pose(
    element: Element,
    transition: &dyn StackTransition,
    side: Side,
    direction: Direction,
    progress: f32,
) {
    clear_natural_animations(element);
    let mut style = String::from(SLOT_LAYOUT_STYLE);
    let decoration = match transition.slot_style(side, direction) {
        Style::Static(s) => s,
        Style::Dynamic(_) => String::new(),
    };
    if !decoration.is_empty() {
        style.push_str(&decoration);
    }
    for (prop, val) in transition.pose(side, direction, progress) {
        style.push_str(&format!("{prop}: {val};"));
    }
    set_inline_styles(element, &style);
}
