//! A handle that moves and queries a mounted `MapView` from app code.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use glam::DVec2;
use whisker::prelude::*;

use crate::animation::{Animation, FlyPath, Motion};
use crate::camera::MapCamera;
use crate::geo::LatLng;
use crate::map::{MapInner, StyleEdit};
use crate::style::MapProjection;

/// Passed as a `MapView`'s `map_ref:`, it reaches that map once it is mounted. Style and data
/// changes made before then wait for the map; camera moves and queries do nothing.
///
/// `Copy`, so one handle can be moved into several event closures.
#[derive(Clone, Copy)]
pub struct MapRef {
    map: StoredValue<RefCell<Slot>>,
}

type Change = Box<dyn FnOnce(&Rc<MapInner>)>;

#[derive(Default)]
struct Slot {
    map: Weak<MapInner>,
    waiting: Vec<Change>,
}

impl Default for MapRef {
    fn default() -> Self {
        MapRef::new()
    }
}

impl MapRef {
    /// Allocates an unbound handle in the current owner.
    pub fn new() -> MapRef {
        MapRef {
            map: StoredValue::new(RefCell::new(Slot::default())),
        }
    }

    pub(crate) fn bind(&self, map: &Rc<MapInner>) {
        let waiting = self.map.with(|slot| {
            let mut slot = slot.borrow_mut();
            slot.map = Rc::downgrade(map);
            std::mem::take(&mut slot.waiting)
        });
        for change in waiting {
            change(map);
        }
    }

    fn map(&self) -> Option<Rc<MapInner>> {
        self.map.with(|slot| slot.borrow().map.upgrade())
    }

    fn change(&self, change: impl FnOnce(&Rc<MapInner>) + 'static) {
        match self.map() {
            Some(map) => change(&map),
            None => self
                .map
                .with(|slot| slot.borrow_mut().waiting.push(Box::new(change))),
        }
    }

    /// Flies to `camera`, zooming out and back in along MapLibre's `flyTo` path, for a duration
    /// that grows with the distance.
    pub fn fly_to(&self, camera: MapCamera) {
        let Some(map) = self.map() else { return };
        let (width, height) = map.logical_size();
        let from = map.camera();
        let path = FlyPath::new(from, camera, width, height, 1.42);
        let duration = path.duration();
        map.animate(Animation::new(
            Motion::Fly {
                from,
                to: camera,
                path,
            },
            duration,
        ));
    }

    /// Moves to `camera` along an ease-in-out curve over `duration`.
    pub fn ease_to(&self, camera: MapCamera, duration: Duration) {
        let Some(map) = self.map() else { return };
        let from = map.camera();
        map.animate(Animation::new(Motion::Ease { from, to: camera }, duration));
    }

    /// Replaces the data of the style's GeoJSON source `source` with `data`, a GeoJSON object or
    /// the URL of one.
    pub fn set_geojson(&self, source: &str, data: serde_json::Value) {
        let source = source.to_string();
        self.change(move |map| map.set_geojson(&source, data));
    }

    /// Adds or replaces the style source `id`, given as style JSON.
    pub fn add_source(&self, id: &str, source: serde_json::Value) {
        let edit = StyleEdit::AddSource(id.to_string(), source);
        self.change(move |map| map.edit_style(edit));
    }

    /// Adds `layer`, given as style JSON, under the layer `before` or on top of every layer; a
    /// layer with the same id is replaced.
    pub fn add_layer(&self, layer: serde_json::Value, before: Option<&str>) {
        let edit = StyleEdit::AddLayer(layer, before.map(str::to_string));
        self.change(move |map| map.edit_style(edit));
    }

    /// Lays the map out as `projection`, whatever the style asks for.
    pub fn set_projection(&self, projection: MapProjection) {
        self.change(move |map| map.set_projection(projection));
    }

    /// Where `place` is drawn, in logical pixels from the map's top left; `None` when it is behind
    /// the camera.
    pub fn to_screen(&self, place: LatLng) -> Option<(f64, f64)> {
        let point = self.map()?.view().project(place)?;
        Some((point.x, point.y))
    }

    /// The place drawn at `(x, y)` logical pixels from the map's top left; `None` above the
    /// horizon.
    pub fn to_latlng(&self, x: f64, y: f64) -> Option<LatLng> {
        self.map()?.view().unproject(DVec2::new(x, y))
    }
}
