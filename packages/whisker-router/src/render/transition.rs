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

/// Which screen edge a back gesture started from. Drives the
/// Material-style predictive-back pose: a left-edge swipe shifts the
/// shrinking top card to the **right**; a right-edge swipe keeps it
/// centred. (iOS edge swipe is always treated as `Left`.)
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum SwipeEdge {
    /// Gesture from the left edge — Android `BackEventCompat.EDGE_LEFT`
    /// (`swipeEdge == 0`). The default.
    #[default]
    Left,
    /// Gesture from the right edge — Android `EDGE_RIGHT` (`swipeEdge == 1`).
    Right,
}

impl SwipeEdge {
    /// Decode Android's `BackEventCompat.swipeEdge` (0 = left, 1 = right).
    pub fn from_android(swipe_edge: i64) -> Self {
        if swipe_edge == 1 {
            SwipeEdge::Right
        } else {
            SwipeEdge::Left
        }
    }
}

// ----- Material predictive-back tunables ------------------------------

/// Minimum scale the cards shrink to at full back progress (Android's
/// system preview shrinks to ~0.9).
const PB_MIN_SCALE: f32 = 0.9;
/// Corner radius (px) the top card reaches at full progress.
const PB_MAX_RADIUS: f32 = 24.0;
/// How far (fraction of width) a left-edge swipe nudges the top card to
/// the right at full progress.
const PB_EDGE_SHIFT: f32 = 0.06;
/// Max dim of the backdrop behind the top card at full progress.
pub const PB_MAX_DIM: f32 = 0.30;

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

/// How a wrapper poses itself for a given progress: either the normal
/// route [`Transition`] (push/pop/button-back), or the Material
/// **predictive-back** preview (driven live by a back gesture).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PoseMode {
    /// Normal push/pop/replace transition for this route.
    Transition(Transition),
    /// Interactive predictive-back preview from `edge`.
    Predictive(SwipeEdge),
}

/// Resolve a wrapper's [`Pose`] for `role` at `progress` under `mode`.
pub fn pose_for(mode: PoseMode, role: Role, progress: f32) -> Pose {
    match mode {
        PoseMode::Transition(t) => pose(t, role, progress),
        PoseMode::Predictive(edge) => predictive_pose(role, progress, edge),
    }
}

/// A computed CSS pose for one wrapper: a `transform` value, an
/// `opacity`, and a corner radius (px). Border-radius is `0.0` for every
/// non-predictive transition.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    /// The `transform` CSS value.
    pub transform: String,
    /// The `opacity` (0..1).
    pub opacity: f32,
    /// The `border-radius` in px (Material predictive back rounds the
    /// shrinking card; everything else is `0`).
    pub radius_px: f32,
}

impl Pose {
    fn new(transform: String, opacity: f32) -> Self {
        Pose {
            transform,
            opacity,
            radius_px: 0.0,
        }
    }
}

/// The CSS [`Pose`] for `role` at `progress` under `transition`.
///
/// `progress` is the "presence of the top screen" described in the
/// module docs.
pub fn pose(transition: Transition, role: Role, progress: f32) -> Pose {
    let p = progress.clamp(0.0, 1.0);
    let (transform, opacity) = match transition {
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
    };
    Pose::new(transform, opacity)
}

/// The Material-style **predictive-back** pose for `role` at `progress`
/// (the gesture's "presence of the top", 1.0 = at rest, →0.0 = dismissed).
///
/// The shrink amount `back = 1 - progress` drives both cards:
/// - **Top**: scales `1 → ~0.9`, rounds its corners `0 → 24px`, and (on a
///   left-edge swipe) nudges right; a right-edge swipe stays centred.
/// - **Under**: the same scale, sliding in from the left so it sits as an
///   equal-size card just to the left of the top (peeking the previous
///   screen), per the OS preview.
pub fn predictive_pose(role: Role, progress: f32, edge: SwipeEdge) -> Pose {
    let p = progress.clamp(0.0, 1.0);
    let back = 1.0 - p; // 0 at rest, 1 fully dismissed
    let scale = 1.0 - back * (1.0 - PB_MIN_SCALE);
    match role {
        Role::Top => {
            let radius = back * PB_MAX_RADIUS;
            let x = match edge {
                // Left swipe: the card slides toward the right edge.
                SwipeEdge::Left => back * PB_EDGE_SHIFT * 100.0,
                // Right swipe: shrink in place, centred.
                SwipeEdge::Right => 0.0,
            };
            Pose {
                transform: format!("translateX({x}%) scale({scale})"),
                opacity: 1.0,
                radius_px: radius,
            }
        }
        Role::Under => {
            // Same scale; enters from the left. At rest (p=1) it sits just
            // off the left edge (hidden behind the top); as the gesture
            // progresses it slides right to peek beside the shrinking top.
            // `-100%` at p=1 (fully off-left), easing toward `-~60%`.
            let x = -100.0 + back * 40.0;
            Pose {
                transform: format!("translateX({x}%) scale({scale})"),
                opacity: 1.0,
                radius_px: back * PB_MAX_RADIUS,
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
