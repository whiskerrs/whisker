//! [`Fade`] — opacity cross-fade between two stack entries.

use whisker::runtime::view::Element;
use whisker::{AnimateOptions, animate_start};

use super::{Direction, Side, StackTransition};

const DEFAULT_DURATION_MS: u32 = 320;

/// Cross-fade transition: incoming opacity 0→1, outgoing 1→0.
///
/// Layering is cosmetic in a cross-fade (both screens are partially
/// transparent throughout) so [`foreground`](Self::foreground) is
/// always [`Side::Incoming`] — the incoming screen ends fully opaque
/// on top of the outgoing.
#[derive(Copy, Clone, Debug)]
pub struct Fade {
    /// Duration in milliseconds. Default 320ms.
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
            &AnimateOptions::new()
                .duration_ms(self.duration_ms)
                .easing(self.easing)
                .fill("forwards"),
        );
    }

    fn foreground(&self, _direction: Direction) -> Side {
        // Cross-fade is symmetric optically; keep incoming on top so
        // its final fully-opaque state covers correctly.
        Side::Incoming
    }
}
