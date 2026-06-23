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

use std::rc::Rc;

use whisker::AnimConfig;

/// A user-extensible screen transition: how a route enters and leaves when
/// it becomes / stops being the top of its stack.
///
/// A transition is defined by a [`TransitionImpl`] — its timing
/// ([`config`](TransitionImpl::config), duration + easing) and its
/// `progress → CSS` posers ([`pose`](TransitionImpl::pose)). The built-ins
/// ([`slide`](Transition::slide), [`fade`](Transition::fade),
/// [`modal`](Transition::modal), [`none`](Transition::none)) are just
/// in-crate impls; apps add their own with [`Transition::custom`].
///
/// `Rc`-backed so it is cheap to [`Clone`] and store per-route. Edge-swipe
/// back is **not** part of a transition — it is enabled by mounting a
/// gesture component (`SwipeBack` / `AndroidPredictiveBack`).
#[derive(Clone)]
pub struct Transition(Rc<dyn TransitionImpl>);

/// Defines a [`Transition`]'s timing and per-progress poses. Implement this
/// on your own type and wrap it with [`Transition::custom`] to add a custom
/// screen transition (cf. Flutter's `PageTransitionsBuilder`).
pub trait TransitionImpl {
    /// The animation config (duration + easing) for the non-interactive
    /// push/pop run. Duration is owned by the transition here (unlike
    /// Flutter, which keeps it on the route).
    fn config(&self) -> AnimConfig;

    /// The CSS [`Pose`] for `role` at `progress` (the "presence of the top
    /// screen": `1.0` = top fully present, `0.0` = top fully off). `Top` is
    /// the entering/leaving screen, `Under` the one beneath it.
    fn pose(&self, role: Role, progress: f32) -> Pose;

    /// Whether the swap is instant (no animation) — the screen is exchanged
    /// in one frame and the controller is settled synchronously. Default
    /// `false`.
    fn is_instant(&self) -> bool {
        false
    }

    /// A short identifier for debugging / tests (e.g. `"slide"`).
    fn name(&self) -> &'static str {
        "custom"
    }
}

impl Transition {
    /// Wrap a custom [`TransitionImpl`] as a [`Transition`].
    pub fn custom(imp: impl TransitionImpl + 'static) -> Self {
        Transition(Rc::new(imp))
    }

    /// Horizontal iOS slide (the iOS default): the incoming screen slides
    /// in from the right, the covered screen parallax-slides left + dims.
    pub fn slide() -> Self {
        Transition::custom(Slide)
    }

    /// Cross-fade opacity, no translation.
    pub fn fade() -> Self {
        Transition::custom(Fade)
    }

    /// No animation — the screen swaps in one frame.
    pub fn none() -> Self {
        Transition::custom(NoneTransition)
    }

    /// Slide up from the bottom (modal presentation).
    pub fn modal() -> Self {
        Transition::custom(Modal)
    }

    /// The Android default: a small horizontal slide combined with a fade
    /// (Material shared-axis feel) — subtler than the full iOS slide.
    pub fn android_default() -> Self {
        Transition::custom(SmallSlideFade)
    }

    /// The animation config (duration + easing) for this transition.
    pub fn config(&self) -> AnimConfig {
        self.0.config()
    }

    /// The pose for `role` at `progress`.
    pub fn pose(&self, role: Role, progress: f32) -> Pose {
        self.0.pose(role, progress)
    }

    /// Whether the swap is instant (no animation).
    pub fn is_instant(&self) -> bool {
        self.0.is_instant()
    }

    /// The transition's short identifier (debugging / tests).
    pub fn name(&self) -> &'static str {
        self.0.name()
    }
}

impl std::fmt::Debug for Transition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Transition({})", self.name())
    }
}

/// The platform default: iOS gets the full [`slide`](Transition::slide);
/// Android gets the subtler [`android_default`](Transition::android_default)
/// (small slide + fade). This is whisker's analogue of Flutter's
/// `PageTransitionsTheme` per-`TargetPlatform` default.
impl Default for Transition {
    fn default() -> Self {
        #[cfg(target_os = "android")]
        {
            Transition::android_default()
        }
        #[cfg(not(target_os = "android"))]
        {
            Transition::slide()
        }
    }
}

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
/// Default corner radius (dp) the top card reaches at full progress, used
/// until [`set_device_corner_radius`] overrides it with the real device
/// display radius. iOS keeps this default.
const PB_DEFAULT_RADIUS: f32 = 24.0;
/// How far (fraction of width) a left-edge swipe nudges the top card to
/// the right at full progress.
const PB_EDGE_SHIFT: f32 = 0.06;
/// Max dim of the backdrop behind the top card at full progress.
pub const PB_MAX_DIM: f32 = 0.30;

/// The corner radius (dp) the predictive-back card rounds to at full
/// progress, stored as `f32` bits. Defaults to [`PB_DEFAULT_RADIUS`]; the
/// Android gesture overrides it once with the real display radius (queried
/// from the `PredictiveBack` module's `getDeviceCornerRadius`).
///
/// **Global** (not thread-local) so a value installed from the gesture
/// event handler is visible to the pose `computed` even if they run on
/// different threads — a thread-local would silently fail to propagate.
static DEVICE_CORNER_RADIUS_BITS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(PB_DEFAULT_RADIUS.to_bits());

/// Set the device's display corner radius (dp) used for the predictive
/// card. Called once by the Android gesture after querying the host.
pub(crate) fn set_device_corner_radius(dp: f32) {
    if dp.is_finite() && dp >= 0.0 {
        DEVICE_CORNER_RADIUS_BITS.store(dp.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

/// The current device display corner radius (dp).
pub(crate) fn max_corner_radius() -> f32 {
    f32::from_bits(DEVICE_CORNER_RADIUS_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

/// The corner radius (dp) every router screen is clipped to — the
/// router's layout-level rounding. Defaults to the device display radius
/// ([`max_corner_radius`]); a future router-layout config can override it
/// per app (the `screen_corner_radius` extension point). Read by the
/// stack wrapper's constant clip in [`crate::render::node`].
pub(crate) fn screen_corner_radius() -> f32 {
    if let Some(r) = SCREEN_CORNER_RADIUS_OVERRIDE_BITS
        .load(std::sync::atomic::Ordering::Relaxed)
        .checked_sub(1)
    {
        return f32::from_bits(r);
    }
    max_corner_radius()
}

/// User override for the screen corner radius (dp), stored as `f32` bits
/// **+ 1** so `0` means "unset → fall back to the device radius". Lets an
/// app pin a fixed router screen rounding regardless of device.
static SCREEN_CORNER_RADIUS_OVERRIDE_BITS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

/// Override the router's screen corner radius (dp); pass `None` to revert
/// to the device display radius. The extensible hook behind
/// [`screen_corner_radius`].
pub fn set_screen_corner_radius(dp: Option<f32>) {
    let stored = match dp {
        Some(v) if v.is_finite() && v >= 0.0 => v.to_bits().wrapping_add(1),
        _ => 0,
    };
    SCREEN_CORNER_RADIUS_OVERRIDE_BITS.store(stored, std::sync::atomic::Ordering::Relaxed);
}

/// How far the covered screen parallax-slides while the top screen is
/// fully present (fraction of width). Mirrors `IosSlide`'s 30% feel.
const PARALLAX: f32 = 0.30;

/// How much the covered screen dims at full cover (0 = none, 1 = black).
const DIM: f32 = 0.12;

/// How a wrapper poses itself for a given progress: either the normal
/// route [`Transition`] (push/pop/button-back), or the Material
/// **predictive-back** preview (driven live by a back gesture).
///
/// `Clone` (not `Copy`) since [`Transition`] is `Rc`-backed.
#[derive(Clone, Debug)]
pub enum PoseMode {
    /// Normal push/pop/replace transition for this route.
    Transition(Transition),
    /// Interactive predictive-back preview from `edge`.
    Predictive(SwipeEdge),
}

/// Resolve a wrapper's [`Pose`] for `role` at `progress` under `mode`.
pub fn pose_for(mode: &PoseMode, role: Role, progress: f32) -> Pose {
    match mode {
        PoseMode::Transition(t) => t.pose(role, progress),
        PoseMode::Predictive(edge) => predictive_pose(role, progress, *edge),
    }
}

/// A computed CSS pose for one wrapper: a `transform`, an `opacity`, and
/// the `border-radius` (px) the wrapper's clip view rounds to.
///
/// The corner radius **animates with the gesture** (Material predictive
/// back): `0` at rest (square screen), growing to the device radius as the
/// card shrinks. Normal route transitions keep it `0`. The actual clipping
/// is applied on the inner clip view (see [`crate::render::node`]), which
/// needs the `clip-radius` Lynx attribute for the rounding to clip children.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    /// The `transform` CSS value.
    pub transform: String,
    /// The `opacity` (0..1).
    pub opacity: f32,
    /// The `border-radius` in px the clip view rounds to (0 = square).
    pub radius_px: f32,
}

impl Pose {
    /// A pose with no corner rounding (`radius_px = 0`). The common
    /// constructor for custom [`TransitionImpl::pose`] implementations.
    pub fn new(transform: String, opacity: f32) -> Self {
        Pose {
            transform,
            opacity,
            radius_px: 0.0,
        }
    }

    /// A pose that also rounds the clip view to `radius_px` (used by the
    /// predictive-back card).
    pub fn with_radius(transform: String, opacity: f32, radius_px: f32) -> Self {
        Pose {
            transform,
            opacity,
            radius_px,
        }
    }
}

/// The Material-style **predictive-back** pose for `role` at `progress`
/// (the gesture's "presence of the top", 1.0 = at rest, →0.0 = dismissed).
///
/// The shrink amount `back = 1 - progress` drives both cards (the corner
/// radius is the wrapper's constant clip, not animated here):
/// - **Top**: scales `1 → ~0.9` and (on a left-edge swipe) nudges right; a
///   right-edge swipe stays centred.
/// - **Under**: the same scale, sliding in from the left so it sits as an
///   equal-size card just to the left of the top (peeking the previous
///   screen), per the OS preview.
pub fn predictive_pose(role: Role, progress: f32, edge: SwipeEdge) -> Pose {
    let p = progress.clamp(0.0, 1.0);
    let back = 1.0 - p; // 0 at rest, 1 fully dismissed
    let scale = 1.0 - back * (1.0 - PB_MIN_SCALE);
    // Corner radius animates with the gesture: square at rest (back=0),
    // rounding to the device radius as the card shrinks (back=1).
    let radius = back * screen_corner_radius();
    match role {
        Role::Top => {
            let x = match edge {
                // Left swipe: the card slides toward the right edge.
                SwipeEdge::Left => back * PB_EDGE_SHIFT * 100.0,
                // Right swipe: shrink in place, centred.
                SwipeEdge::Right => 0.0,
            };
            Pose::with_radius(format!("translateX({x}%) scale({scale})"), 1.0, radius)
        }
        Role::Under => {
            // Same scale; enters from the left. At rest (p=1) it sits just
            // off the left edge (hidden behind the top); as the gesture
            // progresses it slides right to peek beside the shrinking top.
            // `-100%` at p=1 (fully off-left), easing toward `-~60%`.
            let x = -100.0 + back * 40.0;
            Pose::with_radius(format!("translateX({x}%) scale({scale})"), 1.0, radius)
        }
    }
}

// ----- Built-in transitions -------------------------------------------

/// Horizontal iOS slide: top enters from the right (100% → 0%), under
/// parallaxes left (0% → -30%) and dims slightly.
struct Slide;
impl TransitionImpl for Slide {
    fn config(&self) -> AnimConfig {
        AnimConfig::ease_out(300)
    }
    fn name(&self) -> &'static str {
        "slide"
    }
    fn pose(&self, role: Role, progress: f32) -> Pose {
        let p = progress.clamp(0.0, 1.0);
        match role {
            Role::Top => {
                // p=0 → fully right (off), p=1 → centred.
                let x = (1.0 - p) * 100.0;
                Pose::new(format!("translateX({x}%)"), 1.0)
            }
            Role::Under => {
                // p=0 → at rest, p=1 → parallaxed left + dimmed.
                let x = -(p * PARALLAX * 100.0);
                Pose::new(format!("translateX({x}%)"), 1.0 - p * DIM)
            }
        }
    }
}

/// The Android default: a small horizontal slide + a fade (Material
/// shared-axis feel). The top slides only a short distance from the right
/// while fading in; the under fades out a touch without a big parallax.
struct SmallSlideFade;
impl TransitionImpl for SmallSlideFade {
    fn config(&self) -> AnimConfig {
        AnimConfig::ease_out(280)
    }
    fn name(&self) -> &'static str {
        "android-default"
    }
    fn pose(&self, role: Role, progress: f32) -> Pose {
        let p = progress.clamp(0.0, 1.0);
        match role {
            Role::Top => {
                // Small slide: ~8% of width from the right, plus a fade in.
                let x = (1.0 - p) * 8.0;
                Pose::new(format!("translateX({x}%)"), p)
            }
            Role::Under => {
                // A slight reverse slide + fade out so the swap reads.
                let x = -((1.0 - (1.0 - p)) * 4.0); // = -(p * 4.0)
                Pose::new(format!("translateX({x}%)"), 1.0 - p * 0.3)
            }
        }
    }
}

/// Modal: top slides up from the bottom (100% → 0% on Y); the under
/// screen stays put (a modal covers without parallax).
struct Modal;
impl TransitionImpl for Modal {
    fn config(&self) -> AnimConfig {
        AnimConfig::ease_out(340)
    }
    fn name(&self) -> &'static str {
        "modal"
    }
    fn pose(&self, role: Role, progress: f32) -> Pose {
        let p = progress.clamp(0.0, 1.0);
        match role {
            Role::Top => {
                let y = (1.0 - p) * 100.0;
                Pose::new(format!("translateY({y}%)"), 1.0)
            }
            Role::Under => Pose::new("translateX(0px)".to_string(), 1.0),
        }
    }
}

/// Cross-fade: top fades in (opacity 0 → 1), no translation; under
/// fades out a touch so the swap reads as a dissolve.
struct Fade;
impl TransitionImpl for Fade {
    fn config(&self) -> AnimConfig {
        AnimConfig::ease_out(220)
    }
    fn name(&self) -> &'static str {
        "fade"
    }
    fn pose(&self, role: Role, progress: f32) -> Pose {
        let p = progress.clamp(0.0, 1.0);
        match role {
            Role::Top => Pose::new("translateX(0px)".to_string(), p),
            Role::Under => Pose::new("translateX(0px)".to_string(), 1.0 - p * 0.5),
        }
    }
}

/// No animation — the screen swaps in one frame. Marked
/// [`is_instant`](TransitionImpl::is_instant) so the reconcile skips the
/// animated run entirely.
struct NoneTransition;
impl TransitionImpl for NoneTransition {
    fn config(&self) -> AnimConfig {
        // A (near-)instant controller so the wiring stays uniform.
        AnimConfig::ease_out(1)
    }
    fn name(&self) -> &'static str {
        "none"
    }
    fn is_instant(&self) -> bool {
        true
    }
    fn pose(&self, role: Role, progress: f32) -> Pose {
        let p = progress.clamp(0.0, 1.0);
        match role {
            // Hide the top until it is at least half in so a swap doesn't
            // flash an off-screen frame.
            Role::Top => Pose::new(
                "translateX(0px)".to_string(),
                if p > 0.0 { 1.0 } else { 0.0 },
            ),
            Role::Under => Pose::new("translateX(0px)".to_string(), 1.0),
        }
    }
}
