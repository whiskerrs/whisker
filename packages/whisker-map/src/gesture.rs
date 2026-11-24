//! Turns pointer streams into camera moves.

use std::time::Duration;

use glam::DVec2;

use crate::animation::{Animation, Motion, pan, rotate_around, zoom_around};
use crate::camera::MAX_PITCH;
use crate::map::MapInner;

/// How far a pointer may move, in logical pixels, and still count as a tap.
const TAP_SLOP: f64 = 8.0;
const DOUBLE_TAP_WINDOW_MS: f64 = 300.0;
const DOUBLE_TAP_SLOP: f64 = 30.0;
/// Releases slower than this, in logical pixels per second, stop dead.
const FLING_MIN_SPEED: f64 = 250.0;
const FLING_MAX_SPEED: f64 = 4000.0;
/// Degrees two fingers must turn before the map starts rotating with them.
const ROTATE_THRESHOLD: f64 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PointerKind {
    Touch,
    Mouse,
}

#[derive(Clone, Copy, Debug)]
struct Pointer {
    id: i64,
    position: DVec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TwoFinger {
    Undecided,
    Transform,
    Pitch,
}

/// The gesture state of one map.
#[derive(Default)]
pub(crate) struct Gestures {
    /// Whether gestures move the camera; taps are reported either way.
    moves: bool,
    pointers: Vec<Pointer>,
    /// Where the current single-pointer stream went down, and whether it moved too far to tap.
    down: Option<(DVec2, f64)>,
    moved: bool,
    last_tap: Option<(DVec2, f64)>,
    samples: Vec<(f64, DVec2)>,
    two: Option<TwoState>,
    /// A mouse stream that rotates and tilts instead of panning.
    turning: bool,
}

#[derive(Clone, Copy, Debug)]
struct TwoState {
    mode: TwoFinger,
    start_distance: f64,
    start_angle: f64,
    start_points: [DVec2; 2],
    rotating: bool,
}

/// One pointer sample, in logical pixels relative to the map's top left.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    pub id: i64,
    pub position: DVec2,
    pub time_ms: f64,
    pub kind: PointerKind,
    /// Mouse buttons held, as the DOM's `buttons` bitset.
    pub buttons: u32,
}

impl Gestures {
    /// Whether a finger or mouse button is down on the map.
    pub fn is_active(&self) -> bool {
        !self.pointers.is_empty()
    }

    pub fn new(moves: bool) -> Gestures {
        Gestures {
            moves,
            ..Gestures::default()
        }
    }

    pub fn down(&mut self, map: &MapInner, sample: Sample) {
        if self.moves {
            map.stop_animation();
        }
        self.pointers.retain(|p| p.id != sample.id);
        self.pointers.push(Pointer {
            id: sample.id,
            position: sample.position,
        });
        match self.pointers.len() {
            1 => {
                self.down = Some((sample.position, sample.time_ms));
                self.moved = false;
                self.samples.clear();
                self.samples.push((sample.time_ms, sample.position));
                self.turning = sample.kind == PointerKind::Mouse && sample.buttons & 2 != 0;
                self.two = None;
            }
            2 => {
                self.moved = true;
                self.start_two();
            }
            _ => {}
        }
    }

    fn start_two(&mut self) {
        let [a, b] = [self.pointers[0].position, self.pointers[1].position];
        self.two = Some(TwoState {
            mode: TwoFinger::Undecided,
            start_distance: (b - a).length().max(1.0),
            start_angle: (b - a).y.atan2((b - a).x).to_degrees(),
            start_points: [a, b],
            rotating: false,
        });
    }

    pub fn moved(&mut self, map: &MapInner, sample: Sample) {
        let Some(index) = self.pointers.iter().position(|p| p.id == sample.id) else {
            return;
        };
        let previous = self.pointers[index].position;
        if previous == sample.position {
            return;
        }
        match self.pointers.len() {
            1 => {
                if let Some((start, _)) = self.down
                    && (sample.position - start).length() > TAP_SLOP
                {
                    self.moved = true;
                }
                self.pointers[index].position = sample.position;
                if !self.moved || !self.moves {
                    return;
                }
                let view = map.view();
                if self.turning {
                    let delta = sample.position - previous;
                    let mut camera = view.camera;
                    camera.bearing += (delta.x * 0.8) as f32;
                    camera.pitch = (camera.pitch - (delta.y * 0.5) as f32).clamp(0.0, MAX_PITCH);
                    map.move_camera(camera);
                } else {
                    map.move_camera(pan(&view, previous, sample.position));
                    self.samples.push((sample.time_ms, sample.position));
                    let cutoff = sample.time_ms - 100.0;
                    self.samples.retain(|(t, _)| *t >= cutoff);
                }
            }
            2 => {
                let before = [self.pointers[0].position, self.pointers[1].position];
                self.pointers[index].position = sample.position;
                let after = [self.pointers[0].position, self.pointers[1].position];
                if self.moves {
                    self.two_finger(map, before, after);
                }
            }
            _ => {
                self.pointers[index].position = sample.position;
            }
        }
    }

    fn two_finger(&mut self, map: &MapInner, before: [DVec2; 2], after: [DVec2; 2]) {
        let Some(mut two) = self.two else { return };
        if two.mode == TwoFinger::Undecided {
            let d0 = after[0] - two.start_points[0];
            let d1 = after[1] - two.start_points[1];
            let distance = (after[1] - after[0]).length();
            let vertical = |d: DVec2| d.y.abs() > 2.0 * d.x.abs();
            let side_by_side = {
                let v = two.start_points[1] - two.start_points[0];
                v.y.abs() < v.x.abs()
            };
            if side_by_side
                && vertical(d0)
                && vertical(d1)
                && d0.y.signum() == d1.y.signum()
                && d0.y.abs() > 6.0
                && d1.y.abs() > 6.0
                && (distance / two.start_distance - 1.0).abs() < 0.1
            {
                two.mode = TwoFinger::Pitch;
            } else if (after[0] - two.start_points[0]).length() > 4.0
                || (after[1] - two.start_points[1]).length() > 4.0
            {
                two.mode = TwoFinger::Transform;
            }
        }
        match two.mode {
            TwoFinger::Undecided => {}
            TwoFinger::Pitch => {
                let dy = ((after[0].y - before[0].y) + (after[1].y - before[1].y)) / 2.0;
                let mut camera = map.camera();
                camera.pitch = (camera.pitch - (dy * 0.5) as f32).clamp(0.0, MAX_PITCH);
                map.move_camera(camera);
            }
            TwoFinger::Transform => {
                let c0 = (before[0] + before[1]) / 2.0;
                let c1 = (after[0] + after[1]) / 2.0;
                let d0 = (before[1] - before[0]).length().max(1.0);
                let d1 = (after[1] - after[0]).length().max(1.0);
                let angle = |p: [DVec2; 2]| (p[1] - p[0]).y.atan2((p[1] - p[0]).x).to_degrees();
                let view = map.view();
                let mut camera = pan(&view, c0, c1);
                let view = view.at(camera);
                camera = zoom_around(&view, camera.zoom + (d1 / d0).log2(), c1);
                if !two.rotating {
                    let turned = angle_difference(angle(after), two.start_angle);
                    if turned.abs() > ROTATE_THRESHOLD {
                        two.rotating = true;
                    }
                }
                if two.rotating {
                    let delta = angle_difference(angle(after), angle(before));
                    let view = view.at(camera);
                    camera = rotate_around(&view, camera.bearing - delta as f32, c1);
                }
                map.move_camera(camera);
            }
        }
        self.two = Some(two);
    }

    /// Returns where a single tap landed, when this release ends one.
    pub fn up(&mut self, map: &MapInner, sample: Sample) -> Option<DVec2> {
        let index = self.pointers.iter().position(|p| p.id == sample.id)?;
        self.pointers.remove(index);
        match self.pointers.len() {
            0 => return self.release(map, sample),
            1 => {
                // The remaining finger pans from where it is, without jumping.
                self.two = None;
                self.samples.clear();
                self.down = None;
                self.moved = true;
            }
            2 => self.start_two(),
            _ => {}
        }
        None
    }

    pub fn cancel(&mut self, sample: Sample) {
        self.pointers.retain(|p| p.id != sample.id);
        if self.pointers.is_empty() {
            self.two = None;
            self.down = None;
            self.samples.clear();
        }
    }

    fn release(&mut self, map: &MapInner, sample: Sample) -> Option<DVec2> {
        let mut tap = None;
        let tapped = !self.moved && self.down.is_some_and(|(_, t)| sample.time_ms - t < 300.0);
        if tapped {
            let double = self.moves
                && self.last_tap.is_some_and(|(p, t)| {
                    sample.time_ms - t < DOUBLE_TAP_WINDOW_MS
                        && (sample.position - p).length() < DOUBLE_TAP_SLOP
                });
            if double {
                self.last_tap = None;
                let from = map.camera();
                map.animate(Animation::new(
                    Motion::ZoomAround {
                        from,
                        delta: 1.0,
                        anchor: sample.position,
                    },
                    Duration::from_millis(300),
                ));
            } else {
                self.last_tap = Some((sample.position, sample.time_ms));
                tap = Some(sample.position);
            }
        } else if self.moves
            && !self.turning
            && let Some(velocity) = self.velocity(sample.time_ms)
        {
            map.animate(Animation::fling(map.camera(), velocity));
        }
        self.down = None;
        self.two = None;
        self.samples.clear();
        self.turning = false;
        tap
    }

    /// The release velocity of a single-pointer pan, if it is fast enough to fling.
    fn velocity(&self, now: f64) -> Option<DVec2> {
        let (t0, p0) = *self.samples.first()?;
        let (t1, p1) = *self.samples.last()?;
        if now - t1 > 50.0 || t1 - t0 < 10.0 {
            return None;
        }
        let v = (p1 - p0) / ((t1 - t0) / 1000.0);
        let speed = v.length();
        if speed < FLING_MIN_SPEED {
            return None;
        }
        Some(v * (speed.min(FLING_MAX_SPEED) / speed))
    }
}

fn angle_difference(a: f64, b: f64) -> f64 {
    let d = (a - b) % 360.0;
    if d > 180.0 {
        d - 360.0
    } else if d < -180.0 {
        d + 360.0
    } else {
        d
    }
}

/// Zooms around `anchor` for a scroll of `delta_y` logical pixels, at MapLibre's rates: large steps
/// are mouse wheel notches, while small steps and pinches (`ctrl`) come from a trackpad.
pub(crate) fn wheel(map: &MapInner, anchor: DVec2, delta_y: f64, ctrl: bool) {
    if delta_y == 0.0 {
        return;
    }
    map.stop_animation();
    let rate = if !ctrl && delta_y.abs() >= 50.0 {
        1.0 / 450.0
    } else {
        1.0 / 100.0
    };
    let change = wheel_zoom(delta_y, rate);
    let view = map.view();
    map.move_camera(zoom_around(&view, view.camera.zoom + change, anchor));
}

fn wheel_zoom(delta_y: f64, rate: f64) -> f64 {
    let scale = 2.0 / (1.0 + (-delta_y.abs() * rate).exp());
    scale.log2() * -delta_y.signum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::MapCamera;
    use crate::geo::LatLng;
    use whisker_surface::SurfaceSize;

    fn map() -> std::rc::Rc<MapInner> {
        let map = MapInner::new(MapCamera {
            center: LatLng::new(35.0, 139.0),
            zoom: 10.0,
            ..Default::default()
        });
        map.set_size(SurfaceSize {
            width: 800,
            height: 600,
            scale: 1.0,
        });
        map
    }

    fn sample(id: i64, x: f64, y: f64, time_ms: f64) -> Sample {
        Sample {
            id,
            position: DVec2::new(x, y),
            time_ms,
            kind: PointerKind::Touch,
            buttons: 1,
        }
    }

    #[test]
    fn drag_pans_the_place_under_the_finger() {
        let map = map();
        let mut g = Gestures::new(true);
        let place = map.view().unproject(DVec2::new(400.0, 300.0)).unwrap();
        g.down(&map, sample(1, 400.0, 300.0, 0.0));
        g.moved(&map, sample(1, 420.0, 300.0, 16.0));
        g.moved(&map, sample(1, 450.0, 340.0, 32.0));
        let after = map.view().project(place).unwrap();
        assert!(
            (after - DVec2::new(450.0, 340.0)).length() < 1e-6,
            "{after:?}"
        );
    }

    #[test]
    fn pinch_zooms_by_the_distance_ratio() {
        let map = map();
        let mut g = Gestures::new(true);
        g.down(&map, sample(1, 300.0, 300.0, 0.0));
        g.down(&map, sample(2, 500.0, 300.0, 0.0));
        g.moved(&map, sample(1, 200.0, 300.0, 16.0));
        g.moved(&map, sample(2, 600.0, 300.0, 16.0));
        let zoom = map.camera().zoom;
        assert!((zoom - 11.0).abs() < 1e-6, "{zoom}");
    }

    #[test]
    fn double_tap_starts_a_zoom() {
        let map = map();
        let mut g = Gestures::new(true);
        g.down(&map, sample(1, 100.0, 100.0, 0.0));
        assert!(g.up(&map, sample(1, 100.0, 100.0, 50.0)).is_some());
        g.down(&map, sample(2, 102.0, 101.0, 150.0));
        assert_eq!(g.up(&map, sample(2, 102.0, 101.0, 200.0)), None);
        assert!(map.is_animating());
    }

    #[test]
    fn wheel_notches_zoom_like_maplibre() {
        // MapLibre 4.7 zooms 1.426 for five 200 px notches.
        let total: f64 = (0..5).map(|_| wheel_zoom(-200.0, 1.0 / 450.0)).sum();
        assert!((total - 1.426).abs() < 0.01, "{total}");
        assert!(wheel_zoom(10.0, 1.0 / 100.0) < 0.0);
    }
}
