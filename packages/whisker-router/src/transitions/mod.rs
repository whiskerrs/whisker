//! Transitions: pluggable animations for [`StackLayout`](crate::StackLayout).
//!
//! Each transition implementation lives in its own submodule —
//! [`ios_slide`] for the default UIKit-style horizontal slide,
//! [`vertical_slide`] for the Y-axis variant, [`fade`] for an
//! opacity cross-fade, and [`instant`] for the no-animation
//! fallback. This module re-exports the trait + every built-in,
//! plus the shared [`Direction`] / [`Side`] / [`StackTransitionBox`]
//! types each implementation consumes.
//!
//! Custom transitions implement [`StackTransition`] and are passed
//! to the layout via [`StackTransitionBox::new`]:
//!
//! ```ignore
//! use whisker_router::{StackTransition, StackTransitionBox, Direction};
//! use whisker::runtime::view::Element;
//!
//! struct ZoomSlide;
//! impl StackTransition for ZoomSlide {
//!     fn animate(&self, el: Element, side: Side, _dir: Direction) {
//!         if side != Side::Incoming { return; }
//!         whisker::animate_start(el, "zoom-in", &[
//!             ("0%",   &[("transform", "scale(0.85)"), ("opacity", "0")]),
//!             ("100%", &[("transform", "scale(1.0)"),  ("opacity", "1")]),
//!         ], &whisker::AnimateOptions::default()).ok();
//!     }
//! }
//! ```

use std::rc::Rc;

use whisker::runtime::view::Element;
use whisker::Style;

/// Primitives a transition's [`install_gestures`](StackTransition::install_gestures)
/// implementation uses to drive the stack interactively (iOS
/// swipe-back, Android predictive back).
///
/// The fields are intentionally narrow closures rather than the
/// raw slot/owner machinery — the gesture controller only needs to
/// know *what* it can do to the stack, not *how* the layout
/// internally tracks wrappers.
pub struct GestureContext {
    /// The `StackLayout`'s root container view — bind
    /// `touchstart`/`touchmove`/`touchend` handlers here.
    pub container: Element,
    /// Self-reference, so the gesture controller can call `pose` /
    /// `slot_style` / `foreground` on the active transition without
    /// re-binding through `&self`.
    pub transition: StackTransitionBox,
    /// `true` when the stack has more than one entry. Swipe-back
    /// only makes sense above the root; gate the gesture on this.
    pub can_back: Rc<dyn Fn() -> bool>,
    /// Build a wrapper element for the screen one step below the
    /// top of the stack, mount the rendered screen into it, and
    /// insert it at DOM index 0 of the container. Returns the
    /// wrapper handle.
    ///
    /// The layout retains ownership; release via
    /// [`dispose_preview`](Self::dispose_preview) or
    /// [`commit_preview_and_back`](Self::commit_preview_and_back).
    pub mount_preview: Rc<dyn Fn() -> Element>,
    /// Tear down the preview wrapper (remove from DOM, dispose the
    /// reactive owner). Idempotent.
    pub dispose_preview: Rc<dyn Fn()>,
    /// Promote the preview wrapper into the `current` slot, dispose
    /// the old current, and `stack.back()` with the
    /// `skip_animation` flag set so the route-change effect doesn't
    /// re-animate the navigation the gesture already finished.
    pub commit_preview_and_back: Rc<dyn Fn()>,
    /// Handle to the currently-foregrounded wrapper, if any —
    /// needed so the gesture controller can re-pose / re-style the
    /// outgoing screen alongside the preview.
    pub current_wrapper: Rc<dyn Fn() -> Option<Element>>,
}

pub mod fade;
pub mod instant;
pub mod ios_slide;
pub mod vertical_slide;

pub use fade::Fade;
pub use instant::Instant;
pub use ios_slide::{IosSlide, IOS_PARALLAX_PCT};
pub use vertical_slide::VerticalSlide;

/// Direction of the current navigation, derived from the
/// [`RouteStack`](crate::RouteStack) length delta.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Stack grew (a `push`).
    Forward,
    /// Stack shrank (a `back`).
    Backward,
    /// No animation: replace, replace_all-to-same-depth, or the
    /// first mount.
    None,
}

/// One of the two screens involved in a transition.
///
/// `Incoming` is the screen the navigation is bringing in (becomes
/// the new top of the stack after the transition). `Outgoing` is
/// the screen being replaced (was the top before the navigation,
/// slides away during the transition).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Side {
    /// The new top of the stack.
    Incoming,
    /// The screen being replaced.
    Outgoing,
}

/// Pluggable transition for a stack layout.
///
/// One [`animate`](StackTransition::animate) call per slot per
/// navigation — the [`Side`] argument distinguishes "drive the
/// incoming wrapper" from "drive the outgoing wrapper". The trait
/// is `'static` so it can be stored inside an `Rc`.
pub trait StackTransition: 'static {
    /// Drive the animation for one slot wrapper.
    ///
    /// `side` says which slot is being animated (incoming = the
    /// new top of the stack; outgoing = the screen being replaced).
    /// `direction` says which way the navigation is going. Called
    /// from `on_mount` so the element already has a `sign` and is in
    /// Lynx's fiber tree. Invoked twice per transition — once with
    /// `Side::Incoming`, then immediately with `Side::Outgoing` if
    /// there is one — in the same mount tick.
    fn animate(&self, element: Element, side: Side, direction: Direction);

    /// Which side paints on top during the transition.
    ///
    /// Default: `Incoming` on `Forward` / `None`, `Outgoing` on
    /// `Backward`. iOS push covers the previous screen with the new
    /// one; iOS pop keeps the current screen on top while it slides
    /// off, revealing the returning screen from underneath.
    ///
    /// Override when the "covering" screen of your transition is
    /// always one side regardless of direction (e.g. cross-fade
    /// where layering is cosmetic; modal-style presentations where
    /// the modal always sits on top).
    ///
    /// (Implementation note: Lynx's animator ignores explicit
    /// `z-index` while a transform animation is in flight, so
    /// [`StackLayout`](crate::StackLayout) reads this value to
    /// choose the wrapper's DOM insertion order — a sibling later
    /// in the document paints on top. Returning a value here is
    /// enough; you don't need to touch z-index yourself.)
    fn foreground(&self, direction: Direction) -> Side {
        match direction {
            Direction::Backward => Side::Outgoing,
            _ => Side::Incoming,
        }
    }

    /// Inline CSS appended to a slot wrapper, varying by side and
    /// direction.
    ///
    /// Lynx's animator silently drops some properties from
    /// `@keyframes` rules — notably `box-shadow` — and some effects
    /// look right only as an initial state that the animation then
    /// interpolates from (e.g. a brightness dim that fades to full
    /// when the previous screen returns on pop). Anything that
    /// needs to live on the wrapper alongside the layout positioning
    /// goes here.
    ///
    /// Returning a [`Style::Dynamic`](whisker::Style::Dynamic) value
    /// makes the decoration reactive — the wrapper is re-styled
    /// whenever any signal the closure reads fires (useful for
    /// theme-driven shadow colours, etc.). [`Style::Static`] applies
    /// once when the slot mounts and once again when its role flips
    /// at the next navigation.
    ///
    /// The decoration is appended verbatim to the wrapper's
    /// `style` attribute, so include trailing semicolons.
    ///
    /// Returns an empty static style by default.
    fn slot_style(&self, side: Side, direction: Direction) -> Style {
        let _ = (side, direction);
        Style::from("")
    }

    /// Sample the transition's pose at progress `t ∈ [0.0, 1.0]`,
    /// returning the per-property CSS values for the slot wrapper at
    /// that point in the animation.
    ///
    /// `t = 0.0` is the start pose (incoming off-screen for a push,
    /// outgoing at rest for a pop); `t = 1.0` is the settled end
    /// pose. The returned `(prop, value)` pairs describe the
    /// **dynamic** part of the animation only — static decoration
    /// like `box-shadow` belongs in [`slot_style`](Self::slot_style)
    /// and lives on the wrapper for the whole transition.
    ///
    /// This is the entry point for **interactive transitions** —
    /// gesture-driven navigation like iOS swipe-back. The layout
    /// queries `pose` per frame while the user drags, and at gesture
    /// release it builds keyframes from the current pose to the
    /// commit-/cancel-pose and lets Lynx's animator finish the motion.
    ///
    /// Default: empty `Vec` — the transition doesn't support
    /// interactive scrubbing (the layout falls back to running
    /// [`animate`](Self::animate) wholesale at the natural duration).
    fn pose(&self, side: Side, direction: Direction, progress: f32) -> Vec<(&'static str, String)> {
        let _ = (side, direction, progress);
        Vec::new()
    }

    /// Wire interactive gesture handlers onto the layout's
    /// container, if the transition has any. Called once per
    /// `StackLayout` mount.
    ///
    /// This is where transitions opt into gesture-driven navigation
    /// — [`IosSlide`] implements an edge swipe-back here, while the
    /// other built-ins (cross-fade, vertical slide, instant) leave
    /// the default no-op in place. Gesture state lives inside the
    /// transition implementation; the layout exposes only the
    /// primitives the gesture needs via [`GestureContext`].
    fn install_gestures(&self, ctx: &GestureContext) {
        let _ = ctx;
    }
}

/// Cheap-to-clone handle wrapping any [`StackTransition`]
/// implementation. The `#[component]` macro re-clones every prop on
/// every body invocation, so the prop type must be `Clone` — this
/// wrapper makes any `dyn StackTransition` satisfy that.
#[derive(Clone)]
pub struct StackTransitionBox(pub Rc<dyn StackTransition>);

impl StackTransitionBox {
    /// Wrap a [`StackTransition`] implementation.
    pub fn new<T: StackTransition>(t: T) -> Self {
        Self(Rc::new(t))
    }
}

impl Default for StackTransitionBox {
    /// The default transition is [`IosSlide`].
    fn default() -> Self {
        Self::new(IosSlide::default())
    }
}
