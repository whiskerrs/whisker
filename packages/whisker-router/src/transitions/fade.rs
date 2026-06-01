//! [`Fade`] — cross-fade transition between two stack entries.

use whisker::runtime::view::Element;
use whisker::{animate_start, AnimateOptions};

use super::{Direction, Side, StackTransition};

const DEFAULT_DURATION_MS: u32 = 320;

/// Cross-fade. Incoming opacity 0→1, outgoing 1→0.
#[derive(Copy, Clone, Debug)]
pub struct Fade {
    /// Duration in milliseconds.
    pub duration_ms: u32,
    /// Easing function string. Default `"linear"` — fades feel
    /// most natural without acceleration.
    pub easing: &'static str,
}

impl Default for Fade {
    fn default() -> Self {
        Self {
            duration_ms: DEFAULT_DURATION_MS,
            easing: "linear",
        }
    }
}

impl StackTransition for Fade {
    fn animate(&self, element: Element, side: Side, direction: Direction) {
        if direction == Direction::None {
            return;
        }
        let (name, from, to) = match side {
            Side::Incoming => ("stack-fade-incoming", "0", "1"),
            Side::Outgoing => ("stack-fade-outgoing", "1", "0"),
        };
        let _ = animate_start(
            element,
            name,
            &[("0%", &[("opacity", from)]), ("100%", &[("opacity", to)])],
            &AnimateOptions {
                duration_ms: self.duration_ms,
                easing: self.easing.into(),
                fill: "forwards".into(),
                ..Default::default()
            },
        );
    }

    fn foreground(&self, _direction: Direction) -> Side {
        // Crossfade doesn't depend on layering — both screens are
        // partially transparent throughout. Keep incoming on top so
        // its final fully-opaque state covers correctly at the end.
        Side::Incoming
    }
}
