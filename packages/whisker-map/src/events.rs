//! Callbacks a `MapView` makes as its camera settles and its sources load.

use std::rc::Rc;

/// Handles an event that carries nothing; a closure converts into it.
#[derive(Clone)]
pub struct EventCallback(pub(crate) Rc<dyn Fn()>);

impl<F: Fn() + 'static> From<F> for EventCallback {
    fn from(f: F) -> Self {
        EventCallback(Rc::new(f))
    }
}

/// A source whose data or tiles changed during the last frame.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct MapSourceData {
    /// The style source's id.
    pub source: String,
    /// Whether every tile the map asked the source for has arrived.
    pub loaded: bool,
}

/// Handles [`MapSourceData`]; a closure converts into it.
#[derive(Clone)]
pub struct SourceDataCallback(pub(crate) Rc<dyn Fn(MapSourceData)>);

impl<F: Fn(MapSourceData) + 'static> From<F> for SourceDataCallback {
    fn from(f: F) -> Self {
        SourceDataCallback(Rc::new(f))
    }
}

#[derive(Clone, Default)]
pub(crate) struct MapEvents {
    pub move_end: Option<EventCallback>,
    pub idle: Option<EventCallback>,
    pub source_data: Option<SourceDataCallback>,
}
