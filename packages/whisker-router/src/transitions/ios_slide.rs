//! [`IosSlide`] — UINavigationController-style horizontal slide
//! with parallax, leading-edge shadow on the moving screen, and a
//! subtle brightness dim on the parallaxed background screen.
//!
//! Default transition for [`StackLayout`](crate::StackLayout).
//!
//! For the iOS-native edge swipe-back gesture, mount
//! [`IosSwipeBack`](crate::IosSwipeBack) as a child of the layout —
//! the gesture is intentionally a separate composable component,
//! not part of this transition trait, so you can mix transitions
//! and gestures freely.

use whisker::runtime::view::Element;
use whisker::{animate_start, AnimateOptions, Style};

use super::{Direction, Side, StackTransition};

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
}

/// Sample the iOS slide pose at `progress ∈ [0.0, 1.0]`. Shared
/// with the [`IosSwipeBack`](crate::IosSwipeBack) gesture so the
/// gesture's per-frame scrub stays in sync with the natural
/// animation's endpoints.
pub(crate) fn pose(side: Side, direction: Direction, progress: f32) -> Vec<(&'static str, String)> {
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
