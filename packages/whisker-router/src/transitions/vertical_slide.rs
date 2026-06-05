//! [`VerticalSlide`] — Y-axis analogue of
//! [`super::IosSlide`] for stack-driven modal-style presentations.

use whisker::runtime::view::Element;
use whisker::{animate_start, AnimateOptions};

use super::{Direction, Side, StackTransition};

const DEFAULT_DURATION_MS: u32 = 320;
const DEFAULT_EASING: &str = "ease-in-out";

/// Vertical analogue of [`super::IosSlide`] — incoming slides up
/// from below on push, parallaxes down on pop.
///
/// Useful for stack-driven sheet-style presentations where the
/// horizontal slide of [`super::IosSlide`] doesn't fit the visual
/// model.
#[derive(Copy, Clone, Debug)]
pub struct VerticalSlide {
    /// Duration in milliseconds. Default 320ms.
    pub duration_ms: u32,
    /// Easing function string. Default `"ease-in-out"`.
    pub easing: &'static str,
}

impl Default for VerticalSlide {
    fn default() -> Self {
        Self {
            duration_ms: DEFAULT_DURATION_MS,
            easing: DEFAULT_EASING,
        }
    }
}

impl StackTransition for VerticalSlide {
    fn animate(&self, element: Element, side: Side, direction: Direction) {
        let (name, from, to) = match (side, direction) {
            (Side::Incoming, Direction::Forward) => (
                "stack-vertical-incoming-forward",
                "translateY(100%)",
                "translateY(0%)",
            ),
            (Side::Incoming, Direction::Backward) => (
                "stack-vertical-incoming-backward",
                "translateY(-30%)",
                "translateY(0%)",
            ),
            (Side::Outgoing, Direction::Forward) => (
                "stack-vertical-outgoing-forward",
                "translateY(0%)",
                "translateY(-30%)",
            ),
            (Side::Outgoing, Direction::Backward) => (
                "stack-vertical-outgoing-backward",
                "translateY(0%)",
                "translateY(100%)",
            ),
            (_, Direction::None) => return,
        };
        let _ = animate_start(
            element,
            name,
            &[
                ("0%", &[("transform", from)]),
                ("100%", &[("transform", to)]),
            ],
            &AnimateOptions {
                duration_ms: self.duration_ms,
                easing: self.easing.into(),
                fill: "forwards".into(),
                ..Default::default()
            },
        );
    }
}
