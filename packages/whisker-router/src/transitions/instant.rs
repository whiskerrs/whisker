//! [`Instant`] — no-op transition: entries swap in a single frame.

use whisker::runtime::view::Element;

use super::{Direction, Side, StackTransition};

/// No animation: entries swap immediately. Useful for programmatic
/// resets (`replace_all`) or unit-test paths.
#[derive(Copy, Clone, Debug, Default)]
pub struct Instant;

impl StackTransition for Instant {
    fn animate(&self, _element: Element, _side: Side, _direction: Direction) {}
}
