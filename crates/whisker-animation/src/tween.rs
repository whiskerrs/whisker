//! [`Tween<T>`] — a pure, reusable `0..1 → T` mapping, and the
//! [`animated`] convenience constructor.

use whisker_runtime::reactive::{ReadSignal, computed};

use crate::animatable::Animatable;
use crate::config::AnimConfig;
use crate::controller::AnimationController;

/// A stateless interpolation definition: maps a controller's `0..1`
/// progress to a value of type `T`. Reusable — one `Tween` can be
/// `animate`d against different controllers, and one controller can
/// drive several tweens.
#[derive(Clone)]
pub struct Tween<T: Animatable> {
    from: T,
    to: T,
}

impl<T: Animatable + PartialEq> Tween<T> {
    /// Define a tween from `from` (progress `0.0`) to `to` (progress
    /// `1.0`).
    pub fn new(from: T, to: T) -> Self {
        Self { from, to }
    }

    /// Tie this tween to `ctrl`, returning a [`ReadSignal<T>`] that
    /// tracks `T::lerp(from, to, ctrl.value())` — recomputed each time
    /// the controller's progress changes. The value is consumable
    /// anywhere in the reactive graph.
    pub fn animate(&self, ctrl: &AnimationController) -> ReadSignal<T> {
        let from = self.from.clone();
        let to = self.to.clone();
        let progress = ctrl.value();
        computed(move || T::lerp(&from, &to, progress.get()))
    }
}

/// Build a single-value animated signal and its controller together.
///
/// Sugar for the common case: `animated(from, to, cfg)` constructs a
/// [`Tween`] from `from` to `to`, a fresh [`AnimationController`] for
/// `cfg`, and returns `(value_signal, controller)`.
///
/// **No auto-play** — nothing moves until you drive the controller
/// (`ctrl.forward()` / `ctrl.reverse()` / …). This is the only
/// `animated` form; the auto-playing variant is deliberately omitted.
///
/// ```ignore
/// let (x, ctrl) = animated(0.0_f32, 100.0, AnimConfig::ease_out(300));
/// ctrl.forward(); // you decide when it runs
/// ```
pub fn animated<T: Animatable + PartialEq>(
    from: T,
    to: T,
    cfg: AnimConfig,
) -> (ReadSignal<T>, AnimationController) {
    let ctrl = AnimationController::new(cfg);
    let value = Tween::new(from, to).animate(&ctrl);
    (value, ctrl)
}
