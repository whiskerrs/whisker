//! Camera motions played over time.

use std::time::Duration;

use glam::DVec2;

use crate::camera::{MapCamera, View};
use crate::geo::{LatLng, mercator, unmercator};

/// How a camera moves from one place to another.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Motion {
    /// Interpolates every camera field along an ease-in-out curve.
    Ease { from: MapCamera, to: MapCamera },
    /// Zooms out and back in along van Wijk and Nuij's path, like MapLibre's `flyTo`.
    Fly {
        from: MapCamera,
        to: MapCamera,
        path: FlyPath,
    },
    /// Keeps the place under `anchor` still while zooming by `delta`.
    ZoomAround {
        from: MapCamera,
        delta: f64,
        anchor: DVec2,
    },
    /// Continues a released pan, slowing to a stop.
    Fling { from: MapCamera, velocity: DVec2 },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Animation {
    pub motion: Motion,
    pub duration: Duration,
    pub start: Option<Duration>,
}

const FLING_TIME_CONSTANT: f64 = 0.325;

impl Animation {
    pub fn new(motion: Motion, duration: Duration) -> Animation {
        Animation {
            motion,
            duration,
            start: None,
        }
    }

    /// A fling for a release `velocity` in logical pixels per second.
    pub fn fling(from: MapCamera, velocity: DVec2) -> Animation {
        Animation::new(
            Motion::Fling { from, velocity },
            Duration::from_secs_f64(FLING_TIME_CONSTANT * 5.0),
        )
    }

    /// The camera at `now`, and whether the motion is over.
    /// `view` gives the viewport and projection the motion runs in.
    pub fn sample(&mut self, now: Duration, view: &View) -> (MapCamera, bool) {
        let (width, height) = (view.width, view.height);
        let start = *self.start.get_or_insert(now);
        let elapsed = now.saturating_sub(start).as_secs_f64();
        let total = self.duration.as_secs_f64();
        let t = if total <= 0.0 {
            1.0
        } else {
            (elapsed / total).min(1.0)
        };
        let done = t >= 1.0;
        let camera = match self.motion {
            Motion::Ease { from, to } => interpolate(from, to, ease(t), ease(t)),
            Motion::Fly { from, to, path } => {
                let s = ease(t) * path.length;
                let (width, along) = path.at(s);
                let mut camera = interpolate(from, to, along, ease(t));
                camera.zoom = from.zoom - width.log2();
                if done {
                    camera = to;
                }
                camera
            }
            Motion::ZoomAround {
                from,
                delta,
                anchor,
            } => zoom_around(&view.at(from), from.zoom + delta * ease(t), anchor),
            Motion::Fling { from, velocity } => {
                let k = 1.0 - (-elapsed / FLING_TIME_CONSTANT).exp();
                let offset = velocity * FLING_TIME_CONSTANT * k;
                let center = DVec2::new(width / 2.0, height / 2.0);
                pan(&view.at(from), center, center + offset)
            }
        };
        (camera, done)
    }
}

/// MapLibre's default easing, CSS `ease`.
pub(crate) fn ease(t: f64) -> f64 {
    cubic_bezier(0.25, 0.1, 0.25, 1.0, t)
}

fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let bezier = |a: f64, b: f64, t: f64| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    let mut t = x;
    for _ in 0..30 {
        let value = bezier(x1, x2, t);
        if (value - x).abs() < 1e-7 {
            break;
        }
        if value < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) / 2.0;
    }
    bezier(y1, y2, t)
}

/// The shortest way around from `from` to `to` in degrees of longitude.
fn lng_delta(from: f64, to: f64) -> f64 {
    let d = (to - from) % 360.0;
    if d > 180.0 {
        d - 360.0
    } else if d < -180.0 {
        d + 360.0
    } else {
        d
    }
}

fn angle_delta(from: f32, to: f32) -> f32 {
    lng_delta(f64::from(from), f64::from(to)) as f32
}

/// `along` moves the center and zoom, `turn` the bearing and pitch.
fn interpolate(from: MapCamera, to: MapCamera, along: f64, turn: f64) -> MapCamera {
    let (fx, fy) = mercator(from.center);
    let to_lng = from.center.lng + lng_delta(from.center.lng, to.center.lng);
    let (tx, ty) = mercator(LatLng::new(to.center.lat, to_lng));
    let center = unmercator(fx + (tx - fx) * along, fy + (ty - fy) * along);
    MapCamera {
        center: LatLng::new(center.lat, crate::geo::wrap_lng(center.lng)),
        zoom: from.zoom + (to.zoom - from.zoom) * along,
        bearing: from.bearing + angle_delta(from.bearing, to.bearing) * turn as f32,
        pitch: from.pitch + (to.pitch - from.pitch) * turn as f32,
    }
}

/// van Wijk and Nuij's optimal path through zoom and pan.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlyPath {
    w0: f64,
    w1: f64,
    u1: f64,
    r0: f64,
    rho: f64,
    pub length: f64,
}

impl FlyPath {
    /// `curve` is MapLibre's `curve` option, 1.42 by default.
    pub fn new(from: MapCamera, to: MapCamera, width: f64, height: f64, curve: f64) -> FlyPath {
        let rho = curve;
        let rho2 = rho * rho;
        let world = crate::geo::TILE_SIZE * 2f64.powf(from.zoom);
        let (fx, fy) = mercator(from.center);
        let to_lng = from.center.lng + lng_delta(from.center.lng, to.center.lng);
        let (tx, ty) = mercator(LatLng::new(to.center.lat, to_lng));
        let u1 = DVec2::new((tx - fx) * world, (ty - fy) * world).length();
        let w0 = width.max(height);
        let w1 = w0 / 2f64.powf(to.zoom - from.zoom);
        if u1 < 1e-6 {
            let length = (w1 / w0).ln().abs() / rho;
            return FlyPath {
                w0,
                w1,
                u1: 0.0,
                r0: 0.0,
                rho,
                length,
            };
        }
        let b = |i: f64, w: f64| {
            (w1 * w1 - w0 * w0 + i * rho2 * rho2 * u1 * u1) / (2.0 * w * rho2 * u1)
        };
        let r = |b: f64| ((b * b + 1.0).sqrt() - b).ln();
        let r0 = r(b(1.0, w0));
        let r1 = r(b(-1.0, w1));
        FlyPath {
            w0,
            w1,
            u1,
            r0,
            rho,
            length: (r1 - r0) / rho,
        }
    }

    /// The visible span relative to the start, and the fraction of the way to the target.
    fn at(&self, s: f64) -> (f64, f64) {
        if self.u1 == 0.0 {
            let k = if self.w1 < self.w0 { -1.0 } else { 1.0 };
            return ((k * self.rho * s).exp(), 0.0);
        }
        let rho2 = self.rho * self.rho;
        let w = self.r0.cosh() / (self.rho * s + self.r0).cosh();
        let u =
            self.w0 * (self.r0.cosh() * (self.rho * s + self.r0).tanh() - self.r0.sinh()) / rho2;
        (w, (u / self.u1).clamp(0.0, 1.0))
    }

    /// MapLibre's default duration for a speed of 1.2 screenfuls per second.
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64((self.length / 1.2).max(0.0))
    }
}

/// The camera after dragging the place under `from` to `to`, in logical pixels.
pub(crate) fn pan(view: &View, from: DVec2, to: DVec2) -> MapCamera {
    let mut camera = view.camera;
    if view.globe >= 0.5
        && let (Some(a), Some(b)) = (view.screen_to_unit(from), view.screen_to_unit(to))
    {
        // Turning the globe so the place under `from` comes to `to` turns the center alike.
        let (x, y) = mercator(camera.center);
        let center = glam::DQuat::from_rotation_arc(b, a) * crate::geo::globe_unit(x, y);
        camera.center = clamp_center(crate::geo::unit_place(center));
        camera.zoom += globe_zoom_change(view, camera.center.lat);
        return camera;
    }
    let (Some(a), Some(b)) = (view.screen_to_world(from), view.screen_to_world(to)) else {
        return camera;
    };
    let center = view.center + (a - b);
    camera.center = clamp_center(view.place_at_world(center));
    camera.zoom += globe_zoom_change(view, camera.center.lat);
    camera
}

/// The zoom change that keeps the globe's size while its center moves to `lat`, as the globe's
/// radius at a zoom grows with the center's latitude.
fn globe_zoom_change(view: &View, lat: f64) -> f64 {
    if view.globe <= 0.0 {
        return 0.0;
    }
    let cos = |lat: f64| lat.to_radians().cos().max(1e-6);
    (cos(lat) / cos(view.camera.center.lat)).log2() * view.globe
}

/// The camera at `zoom` that keeps the place under `anchor` where it is.
pub(crate) fn zoom_around(view: &View, zoom: f64, anchor: DVec2) -> MapCamera {
    let mut camera = view.camera;
    let Some(place) = view.unproject(anchor) else {
        camera.zoom = zoom;
        return camera;
    };
    camera.zoom = zoom.clamp(
        crate::camera::min_zoom(view.projection),
        crate::camera::MAX_ZOOM,
    );
    keep_place(&view.at(camera), place, anchor)
}

/// The camera turned to `bearing` around the place under `anchor`.
pub(crate) fn rotate_around(view: &View, bearing: f32, anchor: DVec2) -> MapCamera {
    let mut camera = view.camera;
    let Some(place) = view.unproject(anchor) else {
        return camera;
    };
    camera.bearing = normalize_bearing(bearing);
    keep_place(&view.at(camera), place, anchor)
}

fn keep_place(view: &View, place: LatLng, anchor: DVec2) -> MapCamera {
    // On the globe one Mercator step only gets close, so a few more settle it.
    let rounds = if view.globe > 0.0 { 4 } else { 1 };
    let mut view = view.clone();
    for _ in 0..rounds {
        let Some(under) = view.screen_to_world(anchor) else {
            break;
        };
        let target = view.world_point(place);
        let mut camera = view.camera;
        camera.center = clamp_center(view.place_at_world(view.center + (target - under)));
        view = view.at(camera);
    }
    view.camera
}

pub(crate) fn normalize_bearing(bearing: f32) -> f32 {
    let b = bearing.rem_euclid(360.0);
    if b > 180.0 { b - 360.0 } else { b }
}

pub(crate) fn clamp_center(place: LatLng) -> LatLng {
    LatLng::new(
        place
            .lat
            .clamp(-crate::geo::MAX_LATITUDE, crate::geo::MAX_LATITUDE),
        crate::geo::wrap_lng(place.lng),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(lat: f64, lng: f64, zoom: f64) -> MapCamera {
        MapCamera {
            center: LatLng::new(lat, lng),
            zoom,
            ..Default::default()
        }
    }

    #[test]
    fn ease_ends_at_both_ends() {
        assert!(ease(0.0).abs() < 1e-6);
        assert!((ease(1.0) - 1.0).abs() < 1e-6);
        assert!(ease(0.5) > 0.5);
    }

    #[test]
    fn zoom_around_keeps_the_anchor() {
        let view = View::new(camera(35.0, 139.0, 10.0), 800.0, 600.0);
        let anchor = DVec2::new(100.0, 150.0);
        let place = view.unproject(anchor).unwrap();
        let zoomed = zoom_around(&view, 11.5, anchor);
        let after = View::new(zoomed, 800.0, 600.0).project(place).unwrap();
        assert!((after - anchor).length() < 1e-6, "{after:?}");
    }

    #[test]
    fn zoom_around_keeps_the_anchor_on_the_globe() {
        let view = View::projected(
            camera(35.0, 139.0, 2.0),
            800.0,
            600.0,
            crate::style::MapProjection::Globe,
        );
        let anchor = DVec2::new(520.0, 380.0);
        let place = view.unproject(anchor).unwrap();
        let zoomed = zoom_around(&view, 3.0, anchor);
        let after = view.at(zoomed).project(place).unwrap();
        assert!((after - anchor).length() < 0.5, "{after:?}");
    }

    #[test]
    fn panning_the_globe_north_keeps_its_size() {
        let view = View::projected(
            camera(0.0, 0.0, 2.0),
            800.0,
            600.0,
            crate::style::MapProjection::Globe,
        );
        let (from, to) = (DVec2::new(430.0, 200.0), DVec2::new(400.0, 400.0));
        let place = view.unproject(from).unwrap();
        let moved = view.at(pan(&view, from, to));
        assert!(moved.camera.center.lat > 20.0);
        let after = moved.project(place).unwrap();
        assert!((after - to).length() < 1.0, "{after:?}");
        assert!((moved.globe_radius - view.globe_radius).abs() < 1e-6 * view.globe_radius);
    }

    #[test]
    fn pan_moves_the_place_with_the_pointer() {
        let view = View::new(camera(0.0, 0.0, 5.0), 400.0, 400.0);
        let from = DVec2::new(200.0, 200.0);
        let to = DVec2::new(250.0, 180.0);
        let place = view.unproject(from).unwrap();
        let moved = pan(&view, from, to);
        let after = View::new(moved, 400.0, 400.0).project(place).unwrap();
        assert!((after - to).length() < 1e-6, "{after:?}");
    }

    #[test]
    fn fly_reaches_the_target() {
        let from = camera(35.0, 139.0, 12.0);
        let to = camera(48.8, 2.3, 12.0);
        let path = FlyPath::new(from, to, 800.0, 600.0, 1.42);
        assert!(path.length > 0.0);
        let mut animation = Animation::new(Motion::Fly { from, to, path }, path.duration());
        let (start, _) = animation.sample(Duration::ZERO, &View::new(from, 800.0, 600.0));
        assert!((start.zoom - 12.0).abs() < 1e-6);
        let lowest = (1..20)
            .map(|i| {
                animation
                    .sample(path.duration() * i / 20, &View::new(from, 800.0, 600.0))
                    .0
                    .zoom
            })
            .fold(f64::MAX, f64::min);
        assert!(lowest < 4.0, "{lowest}");
        let (end, done) = animation.sample(path.duration(), &View::new(from, 800.0, 600.0));
        assert!(done);
        assert_eq!(end, to);
    }

    #[test]
    fn interpolation_crosses_the_antimeridian() {
        let c = interpolate(camera(0.0, 170.0, 3.0), camera(0.0, -170.0, 3.0), 0.5, 0.5);
        assert!((c.center.lng.abs() - 180.0).abs() < 1e-6, "{c:?}");
    }
}
