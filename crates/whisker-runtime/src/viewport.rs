//! The Host viewport as a reactive value.

use crate::reactive::{ReadSignal, RwSignal};
use whisker_style::StyleEnvironment;

/// The Host viewport in logical pixels, plus the physical pixels per logical pixel.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

crate::runtime_local! {
    static VIEWPORT: RwSignal<Viewport> = RwSignal::new(Viewport::default());
}

/// Reactive accessor for the current runtime's viewport.
///
/// The value changes when the Host resizes, for example on rotation.
/// Must be called on the runtime's UI thread with that runtime entered.
pub fn viewport() -> ReadSignal<Viewport> {
    VIEWPORT.with(|signal| signal.read_only())
}

pub(crate) fn publish(environment: StyleEnvironment) {
    let next = Viewport {
        width: environment.viewport_width(),
        height: environment.viewport_height(),
        scale: environment.scale_factor(),
    };
    VIEWPORT.with(|signal| {
        if signal.get_untracked() != next {
            signal.set(next);
        }
    });
}
