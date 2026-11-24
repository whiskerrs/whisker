//! Coordinates: latitude and longitude, Web Mercator, and tile addresses.

use std::f64::consts::PI;

/// The latitude Web Mercator stops at, where the world becomes square.
pub const MAX_LATITUDE: f64 = 85.051_128_779_806_59;

/// Pixels across the whole world at zoom 0, as in MapLibre.
pub const TILE_SIZE: f64 = 512.0;

/// A place on Earth in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LatLng {
    pub lat: f64,
    pub lng: f64,
}

impl LatLng {
    pub const fn new(lat: f64, lng: f64) -> LatLng {
        LatLng { lat, lng }
    }
}

/// Mercator x and y of a place, each 0 to 1 across the world with y growing south.
pub(crate) fn mercator(place: LatLng) -> (f64, f64) {
    let lat = place.lat.clamp(-MAX_LATITUDE, MAX_LATITUDE).to_radians();
    let x = (place.lng + 180.0) / 360.0;
    let y = 0.5 - ((PI / 4.0 + lat / 2.0).tan().ln()) / (2.0 * PI);
    (x, y)
}

pub(crate) fn unmercator(x: f64, y: f64) -> LatLng {
    let lng = x * 360.0 - 180.0;
    let n = PI - 2.0 * PI * y;
    let lat = n.sinh().atan().to_degrees();
    LatLng { lat, lng }
}

/// How many cells each way a tile at `z` is cut into so that its geometry follows the globe.
pub(crate) fn globe_cells(z: u8) -> u32 {
    (32u32 >> (z / 2)).max(1)
}

/// The unit vector from the Earth's center through a Mercator point; points past the top and
/// bottom of the world land on the poles.
pub(crate) fn globe_unit(x: f64, y: f64) -> glam::DVec3 {
    let lng = x * 2.0 * PI - PI;
    let lat = if y <= 0.0 {
        PI / 2.0
    } else if y >= 1.0 {
        -PI / 2.0
    } else {
        (PI * (1.0 - 2.0 * y)).sinh().atan()
    };
    glam::DVec3::new(lat.cos() * lng.cos(), lat.cos() * lng.sin(), lat.sin())
}

pub(crate) fn unit_place(unit: glam::DVec3) -> LatLng {
    LatLng::new(
        unit.z.clamp(-1.0, 1.0).asin().to_degrees(),
        unit.y.atan2(unit.x).to_degrees(),
    )
}

/// Wraps a longitude into -180 to 180.
pub(crate) fn wrap_lng(lng: f64) -> f64 {
    let wrapped = (lng + 180.0).rem_euclid(360.0) - 180.0;
    if wrapped == -180.0 && lng > 0.0 {
        180.0
    } else {
        wrapped
    }
}

/// A tile in the XYZ scheme: x grows east and y south.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl TileId {
    pub fn new(z: u8, x: u32, y: u32) -> TileId {
        TileId { z, x, y }
    }

    pub fn parent(self) -> Option<TileId> {
        (self.z > 0).then(|| TileId::new(self.z - 1, self.x / 2, self.y / 2))
    }

    /// The ancestor at `z`, or the tile itself when `z` is not above it.
    pub fn ancestor(self, z: u8) -> TileId {
        if z >= self.z {
            return self;
        }
        let shift = self.z - z;
        TileId::new(z, self.x >> shift, self.y >> shift)
    }

    pub fn children(self) -> [TileId; 4] {
        let (z, x, y) = (self.z + 1, self.x * 2, self.y * 2);
        [
            TileId::new(z, x, y),
            TileId::new(z, x + 1, y),
            TileId::new(z, x, y + 1),
            TileId::new(z, x + 1, y + 1),
        ]
    }

    /// Fills a `{z}/{x}/{y}` URL template, with `{-y}` for the TMS scheme and `{quadkey}`.
    pub fn url(self, template: &str, tms: bool) -> String {
        let y = if tms {
            (1 << self.z) - 1 - self.y
        } else {
            self.y
        };
        let mut url = template
            .replace("{z}", &self.z.to_string())
            .replace("{x}", &self.x.to_string())
            .replace("{y}", &y.to_string())
            .replace("{-y}", &((1u32 << self.z) - 1 - self.y).to_string());
        if url.contains("{quadkey}") {
            let mut key = String::new();
            for i in (1..=self.z).rev() {
                let mask = 1 << (i - 1);
                let digit = u8::from(self.x & mask != 0) + 2 * u8::from(self.y & mask != 0);
                key.push(char::from(b'0' + digit));
            }
            url = url.replace("{quadkey}", &key);
        }
        url
    }
}

/// A tile placed in the world, with how many whole worlds east of the first it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct PlacedTile {
    pub id: TileId,
    pub wrap: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mercator_round_trips() {
        for place in [
            LatLng::new(35.68, 139.76),
            LatLng::new(-33.86, 151.2),
            LatLng::new(0.0, 0.0),
        ] {
            let (x, y) = mercator(place);
            let back = unmercator(x, y);
            assert!((back.lat - place.lat).abs() < 1e-9);
            assert!((back.lng - place.lng).abs() < 1e-9);
        }
        assert_eq!(mercator(LatLng::new(0.0, 0.0)), (0.5, 0.5));
    }

    #[test]
    fn globe_units_round_trip() {
        let place = LatLng::new(35.68, 139.76);
        let (x, y) = mercator(place);
        let back = unit_place(globe_unit(x, y));
        assert!((back.lat - place.lat).abs() < 1e-9 && (back.lng - place.lng).abs() < 1e-9);
        assert_eq!(globe_unit(0.3, -0.01).z, 1.0);
    }

    #[test]
    fn wraps_longitudes() {
        assert_eq!(wrap_lng(190.0), -170.0);
        assert_eq!(wrap_lng(-190.0), 170.0);
        assert_eq!(wrap_lng(180.0), 180.0);
    }

    #[test]
    fn fills_templates() {
        let tile = TileId::new(2, 1, 3);
        assert_eq!(tile.url("t/{z}/{x}/{y}.pbf", false), "t/2/1/3.pbf");
        assert_eq!(tile.url("t/{z}/{x}/{y}.pbf", true), "t/2/1/0.pbf");
        assert_eq!(tile.url("q/{quadkey}", false), "q/23");
    }

    #[test]
    fn tile_family() {
        let tile = TileId::new(3, 5, 6);
        assert_eq!(tile.parent(), Some(TileId::new(2, 2, 3)));
        assert_eq!(tile.ancestor(1), TileId::new(1, 1, 1));
        assert!(tile.children().iter().all(|c| c.parent() == Some(tile)));
    }
}
