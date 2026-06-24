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
/// A transition is defined by a [`Transition`] — its timing
/// ([`config`](Transition::config), duration + easing) and its
/// `progress → CSS` posers ([`pose`](Transition::pose)). The built-ins
/// ([`slide`](RouteTransition::slide), [`fade`](RouteTransition::fade),
/// [`modal`](RouteTransition::modal), [`none`](RouteTransition::none)) are just
/// in-crate impls; apps add their own with [`RouteTransition::custom`].
///
/// `Rc`-backed so it is cheap to [`Clone`] and store per-route. Edge-swipe
/// back is **not** part of a transition — it is enabled by mounting a
/// gesture component (`SwipeBack` / `AndroidPredictiveBack`).
#[derive(Clone)]
pub struct RouteTransition(Rc<dyn Transition>);

/// Defines a [`RouteTransition`]'s timing and per-progress poses. Implement this
/// on your own type and wrap it with [`RouteTransition::custom`] to add a custom
/// screen transition (cf. Flutter's `PageTransitionsBuilder`).
pub trait Transition {
    /// The animation config (duration + easing) for the non-interactive
    /// push/pop run. Duration is owned by the transition here (unlike
    /// Flutter, which keeps it on the route).
    fn config(&self) -> AnimConfig;

    /// The CSS [`Pose`] for the screen described by `ctx`: its [`Role`]
    /// (`Top` = the entering/leaving screen, `Under` = the one beneath), the
    /// `progress` ("presence of the top screen": `1.0` = top fully present,
    /// `0.0` = top fully off), and the [`Direction`] (`Push` = forward,
    /// `Pop` = back).
    ///
    /// `role × direction` gives the four directional cases — `(Top, Push)`
    /// enter, `(Under, Push)` exit, `(Top, Pop)` pop-exit, `(Under, Pop)`
    /// pop-enter — so a single transition can be fully asymmetric. A symmetric
    /// transition (a plain slide/fade) simply ignores `ctx.direction`.
    fn pose(&self, ctx: PoseContext) -> Pose;

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

impl RouteTransition {
    /// Wrap a custom [`Transition`] as a [`RouteTransition`].
    pub fn custom(imp: impl Transition + 'static) -> Self {
        RouteTransition(Rc::new(imp))
    }

    /// Horizontal iOS slide (the iOS default): the incoming screen slides
    /// in from the right, the covered screen parallax-slides left + dims.
    pub fn slide() -> Self {
        RouteTransition::custom(Slide)
    }

    /// Cross-fade opacity, no translation.
    pub fn fade() -> Self {
        RouteTransition::custom(Fade)
    }

    /// No animation — the screen swaps in one frame.
    pub fn none() -> Self {
        RouteTransition::custom(NoneTransition)
    }

    /// Slide up from the bottom (modal presentation).
    pub fn modal() -> Self {
        RouteTransition::custom(Modal)
    }

    /// The Android default: a small horizontal slide combined with a fade
    /// (Material shared-axis feel) — subtler than the full iOS slide.
    pub fn android_default() -> Self {
        RouteTransition::custom(SmallSlideFade)
    }

    /// The animation config (duration + easing) for this transition.
    pub fn config(&self) -> AnimConfig {
        self.0.config()
    }

    /// The pose for the screen described by `ctx`.
    pub fn pose(&self, ctx: PoseContext) -> Pose {
        self.0.pose(ctx)
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

impl std::fmt::Debug for RouteTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RouteTransition({})", self.name())
    }
}

/// The platform default: iOS gets the full [`slide`](RouteTransition::slide);
/// Android gets the subtler [`android_default`](RouteTransition::android_default)
/// (small slide + fade). This is whisker's analogue of Flutter's
/// `PageTransitionsTheme` per-`TargetPlatform` default.
impl Default for RouteTransition {
    fn default() -> Self {
        #[cfg(target_os = "android")]
        {
            RouteTransition::android_default()
        }
        #[cfg(not(target_os = "android"))]
        {
            RouteTransition::slide()
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

/// The direction of a route-transition run: a forward navigation (`Push`)
/// or a back (`Pop`). Combined with [`Role`] it selects which of the four
/// directional cases a [`Transition`] poses (see [`Transition::pose`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Direction {
    /// A forward navigation — `navigate` / `replace` / `reset`.
    Push,
    /// A back — `back`, swipe-back, or predictive-back.
    Pop,
}

/// The input to [`Transition::pose`]: the screen's [`Role`], the run
/// `progress` (`0..1`), and the [`Direction`].
///
/// `#[non_exhaustive]` so future pose inputs (e.g. gesture velocity) can be
/// added without breaking custom [`Transition`] impls — read its fields and
/// construct via [`PoseContext::new`].
#[derive(Copy, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct PoseContext {
    /// `Top` (entering/leaving) or `Under` (the screen beneath).
    pub role: Role,
    /// Presence of the top screen: `1.0` fully present → `0.0` fully off.
    pub progress: f32,
    /// `Push` (forward) or `Pop` (back).
    pub direction: Direction,
}

impl PoseContext {
    /// Build a pose context.
    pub fn new(role: Role, progress: f32, direction: Direction) -> Self {
        PoseContext {
            role,
            progress,
            direction,
        }
    }
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
/// How far (fraction of width) the card shifts toward the swipe edge at
/// the preview max (Material shared-element x-shift).
const PB_EDGE_SHIFT: f32 = 0.06;
/// How far (fraction of height) the card's centre drifts toward the finger
/// at the preview max (Material shared-element y-shift / finger follow).
const PB_Y_FOLLOW: f32 = 0.08;
/// Opacity of the black backdrop scrim behind the top card while a
/// predictive-back drag is in progress (held constant during the drag, then
/// faded out on commit — see [`predictive_dim`]).
pub const PB_MAX_DIM: f32 = 0.5;

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

/// The finger's vertical position during a predictive-back gesture, as a
/// fraction of screen height (0 = top, 1 = bottom), stored as `f32` bits.
/// Defaults to centre (0.5). The Android gesture updates it each frame so
/// the Material shared-element card can follow the finger vertically.
static GESTURE_PIVOT_Y_BITS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new((0.5f32).to_bits());

/// Set the predictive-back gesture's vertical pivot (0..1 screen fraction).
pub(crate) fn set_gesture_pivot_y(frac: f32) {
    if frac.is_finite() {
        GESTURE_PIVOT_Y_BITS.store(
            frac.clamp(0.0, 1.0).to_bits(),
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

/// The current predictive-back gesture vertical pivot (0..1).
fn gesture_pivot_y() -> f32 {
    f32::from_bits(GESTURE_PIVOT_Y_BITS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Material's `STANDARD_DECELERATE` easing = `PathInterpolator(0, 0, 0, 1)`
/// (cubic-bezier with both control points pulling the curve up front), so
/// the gesture feedback is "more apparent in the beginning". For this exact
/// bezier the closed form is `y = 3·t^(2/3) − 2·t`.
fn decelerate(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (3.0 * t.powf(2.0 / 3.0) - 2.0 * t).clamp(0.0, 1.0)
}

/// How far the covered screen parallax-slides while the top screen is
/// fully present (fraction of width). Mirrors `IosSlide`'s 30% feel.
const PARALLAX: f32 = 0.30;

/// How much the covered screen dims at full cover (0 = none, 1 = black).
const DIM: f32 = 0.12;

/// How a wrapper poses itself for a given progress: either the normal
/// route [`RouteTransition`] (push/pop/button-back), or the Material
/// **predictive-back** preview (driven live by a back gesture).
///
/// `Clone` (not `Copy`) since [`RouteTransition`] is `Rc`-backed.
#[derive(Clone, Debug)]
pub enum PoseMode {
    /// Normal push/pop/replace transition for this route, in the given
    /// [`Direction`] (`Push` for a forward navigation, `Pop` for a back).
    Transition(RouteTransition, Direction),
    /// Interactive predictive-back preview from `edge`.
    Predictive(SwipeEdge),
}

/// Resolve a wrapper's [`Pose`] for `role` at `progress` under `mode`.
pub fn pose_for(mode: &PoseMode, role: Role, progress: f32) -> Pose {
    match mode {
        PoseMode::Transition(t, dir) => t.pose(PoseContext::new(role, progress, *dir)),
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
#[non_exhaustive] // a pose may grow fields (filter, scrim) — construct via the ctors
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
    /// constructor for custom [`Transition::pose`] implementations.
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

/// The Material-style **predictive-back** pose for `role` at controller
/// `value` (1.0 = top present, → 0.0 = committed/dismissed).
///
/// The controller value spans **two phases** that share one timeline (the
/// gesture scrubs only the upper half, the commit/cancel settle drives the
/// rest — see [`crate::render::gesture::scrub`]):
///
/// - **Preview** (`value` 1.0 → 0.5): present → the Material card preview.
///   Both cards shrink to `PB_MIN_SCALE`, corners round to the device
///   radius, the top nudges per swipe edge, the under peeks from the left.
/// - **Dismiss** (`value` 0.5 → 0.0): preview → committed. The under slides
///   to fully present (and un-rounds), the top fades + shrinks away. This is
///   what makes the *commit* animate smoothly into the previous screen
///   instead of snapping (the iOS slide gets this for free because its
///   `value = 0` already *is* the dismissed state; the Material preview's
///   `value = 0` is only the shrunk card, so it needs this second phase).
pub fn predictive_pose(role: Role, value: f32, edge: SwipeEdge) -> Pose {
    let v = value.clamp(0.0, 1.0);
    // Two phases share the timeline (the drag scrubs only the preview half):
    //  `preview` 0 at present → 1 at preview max (value 0.5), DECELERATED so
    //  the shrink is "more apparent in the beginning" (the official spec).
    //  `dismiss` 0 until value < 0.5 → 1 at value 0 (committed).
    let preview = decelerate(((1.0 - v) / 0.5).clamp(0.0, 1.0));
    let dismiss = ((0.5 - v) / 0.5).clamp(0.0, 1.0);
    let max_radius = screen_corner_radius();
    let shrink = 1.0 - PB_MIN_SCALE; // 0.1 → scales to 90%
    match role {
        Role::Top => {
            // Material **shared-element** preview: the leaving screen becomes
            // a card that shrinks to 90% and FOLLOWS THE FINGER — shifting
            // toward the swipe edge horizontally and toward the finger's Y
            // vertically. On commit it fades + drifts away.
            let scale = 1.0 - preview * shrink - dismiss * 0.04;
            let dir = match edge {
                SwipeEdge::Left => 1.0,   // left-edge swipe → card moves right
                SwipeEdge::Right => -1.0, // right-edge swipe → card moves left
            };
            let x = dir * preview * PB_EDGE_SHIFT * 100.0;
            // Follow the finger vertically: the card's centre drifts toward
            // the touch Y, up to ±PB_Y_FOLLOW, capped by the 8dp margin feel.
            let y = (gesture_pivot_y() - 0.5) * preview * PB_Y_FOLLOW * 100.0;
            let opacity = 1.0 - dismiss;
            let radius = max_radius * preview;
            Pose::with_radius(
                format!("translateX({x}%) translateY({y}%) scale({scale})"),
                opacity,
                radius,
            )
        }
        Role::Under => {
            // The entering (previous) screen scales together with the top
            // card during the drag (down to 0.9, in lockstep via the same
            // decelerated `preview`), held at a fixed left peek — **scale
            // only, no slide** while the finger is down. On commit it slides
            // in from the peek to fully present while growing back to full
            // screen.
            let scale = 1.0 - preview * shrink + dismiss * shrink; // 1 → 0.9 → 1
            // Fixed at -60% (peek) during the drag; -60% → 0% (present) on
            // commit. No `preview` term ⇒ the drag scales without sliding.
            let x = -60.0 + dismiss * 60.0;
            let radius = max_radius * preview * (1.0 - dismiss);
            Pose::with_radius(format!("translateX({x}%) scale({scale})"), 1.0, radius)
        }
    }
}

/// The Material predictive-back **backdrop dim** for the controller `value`
/// (1.0 = rest → 0.0 = committed). A black scrim sits behind the top card and
/// darkens the previous (under) screen *while the finger drags*, then fades
/// back out as the back commits and that screen slides forward to present — so
/// the revealed screen ends at full brightness, matching the official Android
/// behaviour (not darkened at the end).
///
/// Shares [`predictive_pose`]'s two-phase timeline but is **constant over the
/// drag, fading only on commit**: it holds at [`PB_MAX_DIM`] across the whole
/// preview half (value 1.0 → 0.5 — the scrim does not deepen as you drag
/// further), then **falls** back to 0 over the dismiss half (value 0.5 → 0,
/// the commit settle). Returns an `opacity` in `0..=PB_MAX_DIM`. (The scrim
/// only ever drives when a gesture is active — at rest its controller is
/// detached, so the layer is fully transparent regardless of this value.)
pub fn predictive_dim(value: f32) -> f32 {
    let v = value.clamp(0.0, 1.0);
    let dismiss = ((0.5 - v) / 0.5).clamp(0.0, 1.0);
    (1.0 - dismiss) * PB_MAX_DIM
}

// ----- Built-in transitions -------------------------------------------

/// Push/pop duration (ms) of the iOS [`Slide`].
const SLIDE_MS: u32 = 300;
/// Push/pop duration (ms) of the Android-default [`SmallSlideFade`].
const ANDROID_DEFAULT_MS: u32 = 280;
/// Push/pop duration (ms) of the [`Modal`] presentation.
const MODAL_MS: u32 = 340;
/// Cross-fade duration (ms) of the [`Fade`].
const FADE_MS: u32 = 220;
/// A (near-)instant controller for [`NoneTransition`] — the swap is settled
/// synchronously via [`Transition::is_instant`]; this only keeps the wiring
/// uniform.
const INSTANT_MS: u32 = 1;

/// Horizontal iOS slide: top enters from the right (100% → 0%), under
/// parallaxes left (0% → -30%) and dims slightly.
struct Slide;
impl Transition for Slide {
    fn config(&self) -> AnimConfig {
        AnimConfig::ease_out(SLIDE_MS)
    }
    fn name(&self) -> &'static str {
        "slide"
    }
    fn pose(&self, ctx: PoseContext) -> Pose {
        // The built-ins are symmetric: they ignore `ctx.direction`.
        let PoseContext { role, progress, .. } = ctx;
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
impl Transition for SmallSlideFade {
    fn config(&self) -> AnimConfig {
        AnimConfig::ease_out(ANDROID_DEFAULT_MS)
    }
    fn name(&self) -> &'static str {
        "android-default"
    }
    fn pose(&self, ctx: PoseContext) -> Pose {
        // The built-ins are symmetric: they ignore `ctx.direction`.
        let PoseContext { role, progress, .. } = ctx;
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
impl Transition for Modal {
    fn config(&self) -> AnimConfig {
        AnimConfig::ease_out(MODAL_MS)
    }
    fn name(&self) -> &'static str {
        "modal"
    }
    fn pose(&self, ctx: PoseContext) -> Pose {
        // The built-ins are symmetric: they ignore `ctx.direction`.
        let PoseContext { role, progress, .. } = ctx;
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
impl Transition for Fade {
    fn config(&self) -> AnimConfig {
        AnimConfig::ease_out(FADE_MS)
    }
    fn name(&self) -> &'static str {
        "fade"
    }
    fn pose(&self, ctx: PoseContext) -> Pose {
        // The built-ins are symmetric: they ignore `ctx.direction`.
        let PoseContext { role, progress, .. } = ctx;
        let p = progress.clamp(0.0, 1.0);
        match role {
            Role::Top => Pose::new("translateX(0px)".to_string(), p),
            Role::Under => Pose::new("translateX(0px)".to_string(), 1.0 - p * 0.5),
        }
    }
}

/// No animation — the screen swaps in one frame. Marked
/// [`is_instant`](Transition::is_instant) so the reconcile skips the
/// animated run entirely.
struct NoneTransition;
impl Transition for NoneTransition {
    fn config(&self) -> AnimConfig {
        // A (near-)instant controller so the wiring stays uniform.
        AnimConfig::ease_out(INSTANT_MS)
    }
    fn name(&self) -> &'static str {
        "none"
    }
    fn is_instant(&self) -> bool {
        true
    }
    fn pose(&self, ctx: PoseContext) -> Pose {
        // The built-ins are symmetric: they ignore `ctx.direction`.
        let PoseContext { role, progress, .. } = ctx;
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
