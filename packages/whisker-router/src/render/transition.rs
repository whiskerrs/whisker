//! Float-`Tween` screen transitions built on `whisker-animation`.
//!
//! A transition here is **not** a Lynx keyframe animation (see
//! `docs/animation-design.md` for why the router uses the continuous
//! engine instead). It is a pair of pure `progress → CSS` posers driven
//! by an [`AnimationController`]'s `0..1` value, composed into the screen
//! wrapper's inline `transform` / `opacity` by a `computed` — exactly
//! the proven `format!("translateX({}px)", x.get())` pattern.
//!
//! ## Progress convention
//!
//! For a wrapper, **`progress` is "how present the top screen is"**:
//!
//! - `1.0` → the top (incoming) screen fully on screen, the screen
//!   beneath it parallaxed/dimmed out of the way.
//! - `0.0` → the top screen fully off screen (its enter-from edge), the
//!   screen beneath it back at rest.
//!
//! So a **push** drives a fresh top wrapper `0.0 → 1.0`; a **pop** (or
//! swipe-back) drives the leaving top wrapper `1.0 → 0.0`, and the
//! covered wrapper interpolates from "parallaxed" back to "rest" over
//! the same progress. One controller, two posers — the
//! "a gesture spans two routes" problem solved by composition.

use whisker::AnimConfig;

use crate::render::registry::Transition;

/// The visual role of a wrapper within a transition pair.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Role {
    /// The top screen — the one entering on push / leaving on pop.
    Top,
    /// The screen directly beneath the top — covered on push, revealed
    /// on pop.
    Under,
}

/// How far the covered screen parallax-slides while the top screen is
/// fully present (fraction of width). Mirrors `IosSlide`'s 30% feel.
const PARALLAX: f32 = 0.30;

/// How much the covered screen dims at full cover (0 = none, 1 = black).
const DIM: f32 = 0.12;

/// The animation config a [`Transition`] uses for its non-interactive
/// push/pop run.
pub fn config(transition: Transition) -> AnimConfig {
    match transition {
        // A snappy iOS-ish ease for slides; modal a touch longer.
        Transition::Slide => AnimConfig::ease_out(300),
        Transition::Fade => AnimConfig::ease_out(220),
        Transition::Modal => AnimConfig::ease_out(340),
        // `None` still uses a (instant) controller so the wiring is
        // uniform; a 1ms duration settles next frame.
        Transition::None => AnimConfig::ease_out(1),
    }
}

/// The CSS pose (a `transform` value + an `opacity`) for `role` at
/// `progress` under `transition`.
///
/// Returned as `(transform, opacity)` strings so the caller can drop
/// them straight into a `computed` style. `progress` is the
/// "presence of the top screen" described in the module docs.
pub fn pose(transition: Transition, role: Role, progress: f32) -> (String, f32) {
    let p = progress.clamp(0.0, 1.0);
    match transition {
        Transition::Slide => slide_pose(role, p),
        Transition::Modal => modal_pose(role, p),
        Transition::Fade => fade_pose(role, p),
        Transition::None => {
            // No motion; just hide the top until it is at least half in
            // so a swap doesn't flash an off-screen frame.
            match role {
                Role::Top => (
                    "translateX(0px)".to_string(),
                    if p > 0.0 { 1.0 } else { 0.0 },
                ),
                Role::Under => ("translateX(0px)".to_string(), 1.0),
            }
        }
    }
}

/// Horizontal iOS slide: top enters from the right (100% → 0%), under
/// parallaxes left (0% → -30%) and dims slightly.
fn slide_pose(role: Role, p: f32) -> (String, f32) {
    match role {
        Role::Top => {
            // p=0 → fully right (off), p=1 → centred.
            let x = (1.0 - p) * 100.0;
            (format!("translateX({x}%)"), 1.0)
        }
        Role::Under => {
            // p=0 → at rest, p=1 → parallaxed left + dimmed.
            let x = -(p * PARALLAX * 100.0);
            let opacity = 1.0 - p * DIM;
            (format!("translateX({x}%)"), opacity)
        }
    }
}

/// Modal: top slides up from the bottom (100% → 0% on Y); the under
/// screen stays put (a modal covers without parallax).
fn modal_pose(role: Role, p: f32) -> (String, f32) {
    match role {
        Role::Top => {
            let y = (1.0 - p) * 100.0;
            (format!("translateY({y}%)"), 1.0)
        }
        Role::Under => ("translateX(0px)".to_string(), 1.0),
    }
}

/// Cross-fade: top fades in (opacity 0 → 1), no translation; under
/// fades out a touch so the swap reads as a dissolve.
fn fade_pose(role: Role, p: f32) -> (String, f32) {
    match role {
        Role::Top => ("translateX(0px)".to_string(), p),
        Role::Under => ("translateX(0px)".to_string(), 1.0 - p * 0.5),
    }
}

/// Whether this transition can be driven by an edge swipe-back gesture.
/// Modal dismissal is a *downward* swipe (not wired this phase); fade /
/// none have no spatial back affordance. Only the horizontal slide gets
/// the iOS edge swipe.
pub fn supports_edge_swipe(transition: Transition) -> bool {
    matches!(transition, Transition::Slide)
}
