//! [`Instant`] — no-op transition: entries swap in a single frame.

use whisker::runtime::view::Element;

use super::{Direction, Side, StackTransition};

/// No animation — entries swap immediately.
///
/// Useful for programmatic resets (`replace_all`), unit tests, or
/// platforms where animation is unwanted. `animate` is a no-op so
/// the wrapper paints at its settled pose with no transient frame.
#[derive(Copy, Clone, Debug, Default)]
pub struct Instant;

impl StackTransition for Instant {
    fn animate(&self, _element: Element, _side: Side, _direction: Direction) {}
}
