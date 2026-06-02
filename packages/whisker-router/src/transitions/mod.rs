//! Transitions: pluggable visual animations for [`StackLayout`](crate::StackLayout).
//!
//! A transition's job is to describe *how the screen looks* during
//! a navigation — push, pop, anything in between. Each transition
//! implementation lives in its own submodule — [`ios_slide`] for
//! the default UIKit-style horizontal slide, [`vertical_slide`] for
//! the Y-axis variant, [`fade`] for an opacity cross-fade, and
//! [`instant`] for the no-animation fallback. This module re-exports
//! the trait + every built-in, plus the shared [`Direction`] /
//! [`Side`] / [`StackTransitionBox`] types each implementation
//! consumes.
//!
//! Custom transitions implement [`StackTransition`] and are passed
//! to the layout via [`StackTransitionBox::new`]:
//!
//! ```ignore
//! use whisker_router::{StackTransition, StackTransitionBox, Direction, Side};
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
//!
//! Interactive behaviour (iOS swipe-back, Android system back) is
//! intentionally **not** part of this trait — it lives in separate
//! [composable components](crate::gestures) that the user mounts
//! inside the layout. That way transitions stay pure and easy to
//! write, and gestures stay easy to mix-and-match without coupling
//! every transition impl to a touch-event loop.

use std::rc::Rc;

use whisker::runtime::view::Element;
use whisker::Style;

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

/// Pluggable transition for a stack layout — purely visual.
///
/// One [`animate`](StackTransition::animate) call per slot per
/// navigation — the [`Side`] argument distinguishes "drive the
/// incoming wrapper" from "drive the outgoing wrapper". The trait
/// is `'static` so it can be stored inside an `Rc`.
pub trait StackTransition: 'static {
    /// Drive the natural (non-interactive) animation for one slot
    /// wrapper.
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
