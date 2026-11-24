//! Vector maps for Whisker, drawn on the GPU from MapLibre styles.
//!
//! ```ignore
//! let camera = signal(MapCamera { center: LatLng::new(35.68, 139.76), zoom: 13.0, ..Default::default() });
//! render! {
//!     MapView(
//!         map_style: MapStyle::Url("https://tiles.openfreemap.org/styles/liberty".into()),
//!         camera: camera,
//!         style: css!(flex_grow: 1.0),
//!     ) {
//!         Marker(position: LatLng::new(35.681, 139.767)) { View(style: pin) }
//!     }
//! }
//! ```

mod animation;
mod cache;
mod camera;
mod components;
mod geo;
mod gesture;
mod handle;
mod map;
mod net;
mod query;
mod raster;
mod render;
mod sprite;
mod style;
mod symbols;
mod text;
mod tiles;
mod value;
#[cfg(any(target_arch = "wasm32", test))]
mod wire;
mod work;
#[cfg(target_arch = "wasm32")]
mod worker;

pub use camera::{MAX_PITCH, MAX_ZOOM, MIN_ZOOM, MapCamera};
pub use components::{MapView, Marker};
pub use geo::LatLng;
pub use handle::MapRef;
pub use query::{MapFeature, MapTap, TapCallback};
pub use style::{MapProjection, MapStyle};
