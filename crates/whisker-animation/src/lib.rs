//! Whisker's **continuous, signal-based animation engine**.
//!
//! This is the second of Whisker's two animation systems (see
//! `docs/animation-design.md`). Unlike Lynx's discrete CSS-keyframe
//! animator, this engine lives entirely in Rust and drives a `0..1`
//! **progress** signal that the reactive runtime advances each frame —
//! so an animated value is just a [`ReadSignal`] you can read anywhere
//! in the signal graph, with full control over forward / reverse /
//! stop / scrub.
//!
//! ## Model: Controller + Tween (Flutter's split)
//!
//! - [`AnimationController`] drives a `0..1` progress as an explicit
//!   state machine. **No auto-play** — it sits idle until you call
//!   [`forward`](AnimationController::forward) /
//!   [`reverse`](AnimationController::reverse) /
//!   [`animate_to`](AnimationController::animate_to) /
//!   [`set_value`](AnimationController::set_value). **Idle is free**:
//!   when nothing is driving, the engine requests no frames.
//! - [`Tween<T>`] is a pure `0..1 → T` mapping. One controller can
//!   drive several tweens.
//! - [`animated`] builds a `(value_signal, controller)` pair for the
//!   common single-value case.
//!
//! ```ignore
//! use whisker_animation::{animated, AnimConfig};
//!
//! let (x, ctrl) = animated(0.0_f32, 100.0, AnimConfig::ease_out(300));
//! ctrl.forward();                 // explicit — you decide when it runs
//! // read `x.get()` in a css!/computed; it tracks progress each frame
//! ```
//!
//! ## Frame driving
//!
//! The engine keeps the runtime's level-triggered render loop ticking
//! only while a controller is active: registering wakes the host and
//! marks the runtime busy; reaching the target (or `stop`, or
//! owner-dispose) deregisters and releases vsync. The per-frame advance
//! is invoked from the driver's `tick_frame` via the runtime's
//! `anim_hook`; progress is derived from a monotonic millisecond
//! timestamp, which tests inject directly through
//! [`__step_for_tests`](controller::__step_for_tests).

mod animatable;
mod config;
mod controller;
mod curve;
mod tween;

pub use animatable::Animatable;
pub use config::AnimConfig;
pub use controller::AnimationController;
pub use curve::Curve;
pub use tween::{Tween, animated};

#[doc(hidden)]
pub use controller::{__active_count, __reset_for_tests, __step_for_tests};

#[cfg(test)]
mod tests;
