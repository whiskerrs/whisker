//! Data types Lynx uses inline inside property pages but does not
//! document independently. Adding them as Whisker types gives the
//! property builders a strongly-typed argument surface — for
//! example `z-index` is an `<integer>` even though Lynx never spells
//! `<integer>` out on a dedicated data-type page.

mod easing;
mod integer;
mod position;

pub use easing::{EasingFunction, StepPosition};
pub use integer::Integer;
pub use position::{Position, PositionKeyword};
