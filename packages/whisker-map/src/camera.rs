//! Where the map looks from, and the matrices and tiles that follow from it.

use glam::{DMat4, DVec2, DVec3, DVec4};

use crate::geo::{
    LatLng, MAX_LATITUDE, PlacedTile, TILE_SIZE, TileId, globe_unit, mercator, unit_place,
    unmercator,
};
use crate::style::MapProjection;

const EARTH_CIRCUMFERENCE: f64 = 40_075_016.686;

/// MapLibre's vertical field of view.
const FOV_Y: f64 = 0.643_501_108_793_284_4;

/// The steepest tilt the camera takes, in degrees.
pub const MAX_PITCH: f32 = 85.0;
pub const MAX_ZOOM: f64 = 24.0;
/// The lowest zoom of the globe, where the world is 128 logical pixels across; flat Mercator
/// stops at 0.
pub const MIN_ZOOM: f64 = -2.0;

pub(crate) fn min_zoom(projection: MapProjection) -> f64 {
    match projection {
        MapProjection::Globe => MIN_ZOOM,
        _ => 0.0,
    }
}

/// MapLibre's globe: a full globe up to this zoom, turning into Mercator by `MERCATOR_ZOOM`.
const GLOBE_ZOOM: f64 = 11.0;
const MERCATOR_ZOOM: f64 = 12.0;

/// Where the map looks: its center, how far in, which way is up, and how far it tilts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapCamera {
    pub center: LatLng,
    /// MapLibre's zoom: the world is 512 logical pixels across at zoom 0.
    pub zoom: f64,
    /// Degrees clockwise from north that point up the screen.
    pub bearing: f32,
    /// Degrees away from looking straight down.
    pub pitch: f32,
}

impl Default for MapCamera {
    fn default() -> Self {
        MapCamera {
            center: LatLng::new(0.0, 0.0),
            zoom: 1.0,
            bearing: 0.0,
            pitch: 0.0,
        }
    }
}

/// A camera resolved against a viewport: matrices and conversions between screen and world.
///
/// World coordinates are Mercator pixels at the camera's zoom, 0 to `world_size` across.
#[derive(Clone, Debug)]
pub(crate) struct View {
    pub camera: MapCamera,
    pub width: f64,
    pub height: f64,
    pub world_size: f64,
    pub center: DVec2,
    pub camera_to_center: f64,
    pub proj: DMat4,
    /// World pixels to the camera's view space, and view space to clip space; `proj` is their
    /// product.
    pub eye: DMat4,
    pub perspective: DMat4,
    pub inverse: DMat4,
    pub projection: MapProjection,
    /// How far the map has turned into a globe, from 0 for flat Mercator to 1.
    pub globe: f64,
    pub globe_radius: f64,
    /// Takes unit vectors from the Earth's center, in Earth-fixed axes, to clip space.
    pub globe_matrix: DMat4,
    pub globe_inverse: DMat4,
    /// Earth-fixed unit vectors to world pixels.
    pub globe_world: DMat4,
    /// The unit direction from the Earth's center to the camera.
    pub camera_dir: DVec3,
    /// The dot product with `camera_dir` below which the globe is past the horizon.
    pub horizon: f64,
}

impl View {
    /// A flat Mercator view; `width` and `height` are logical pixels.
    #[cfg(test)]
    pub fn new(camera: MapCamera, width: f64, height: f64) -> View {
        View::projected(camera, width, height, MapProjection::Mercator)
    }

    /// The same viewport and projection looking from `camera`.
    pub fn at(&self, camera: MapCamera) -> View {
        View::projected(camera, self.width, self.height, self.projection)
    }

    pub fn projected(
        camera: MapCamera,
        width: f64,
        height: f64,
        projection: MapProjection,
    ) -> View {
        let width = width.max(1.0);
        let height = height.max(1.0);
        let zoom = camera.zoom.clamp(min_zoom(projection), MAX_ZOOM);
        let world_size = TILE_SIZE * 2f64.powf(zoom);
        let (mx, my) = mercator(camera.center);
        let center = DVec2::new(mx * world_size, my * world_size);
        let pitch = f64::from(camera.pitch.clamp(0.0, MAX_PITCH)).to_radians();
        let bearing = f64::from(camera.bearing).to_radians();
        let camera_to_center = 0.5 / (FOV_Y / 2.0).tan() * height;

        // The far plane reaches the top of the screen where it meets the ground.
        let ground_angle = std::f64::consts::FRAC_PI_2 + pitch;
        let top_half = (FOV_Y / 2.0).sin() * camera_to_center
            / (std::f64::consts::PI - ground_angle - FOV_Y / 2.0)
                .clamp(0.01, std::f64::consts::PI - 0.01)
                .sin();
        let furthest = (std::f64::consts::FRAC_PI_2 - pitch).cos() * top_half + camera_to_center;
        let far = (furthest * 1.01).min(camera_to_center * 200.0);
        let near = height / 50.0;

        let eye = DMat4::from_scale(DVec3::new(1.0, -1.0, 1.0))
            * DMat4::from_translation(DVec3::new(0.0, 0.0, -camera_to_center))
            * DMat4::from_rotation_x(pitch)
            * DMat4::from_rotation_z(-bearing)
            * DMat4::from_translation(DVec3::new(-center.x, -center.y, 0.0));
        let perspective = DMat4::perspective_rh(FOV_Y, width / height, near, far);
        let proj = perspective * eye;

        let globe = match projection {
            MapProjection::Mercator => 0.0,
            MapProjection::Globe => {
                ((MERCATOR_ZOOM - zoom) / (MERCATOR_ZOOM - GLOBE_ZOOM)).clamp(0.0, 1.0)
            }
        };
        // The scale at the center matches Mercator's, so the two meet without a jump.
        let lat = camera
            .center
            .lat
            .clamp(-MAX_LATITUDE, MAX_LATITUDE)
            .to_radians();
        let globe_radius = world_size / (2.0 * std::f64::consts::PI * lat.cos());
        let up = globe_unit(mx, my);
        let east = DVec3::new(-up.y, up.x, 0.0).normalize_or(DVec3::Y);
        let north = up.cross(east);
        // Earth-fixed unit vectors to world pixels, with the center touching the ground plane.
        let to_world = DMat4::from_cols(
            (east * globe_radius).extend(center.x),
            (-north * globe_radius).extend(center.y),
            (up * globe_radius).extend(-globe_radius),
            DVec4::W,
        )
        .transpose();
        let globe_matrix = proj * to_world;
        let eye_world = eye.inverse().transform_point3(DVec3::ZERO);
        let eye_unit = to_world.inverse().transform_point3(eye_world);
        let distance = eye_unit.length().max(1.0 + 1e-9);
        View {
            camera: MapCamera { zoom, ..camera },
            width,
            height,
            world_size,
            center,
            camera_to_center,
            proj,
            eye,
            perspective,
            inverse: proj.inverse(),
            projection,
            globe,
            globe_radius,
            globe_matrix,
            globe_inverse: globe_matrix.inverse(),
            globe_world: to_world,
            camera_dir: eye_unit / distance,
            horizon: 1.0 / distance,
        }
    }

    pub fn zoom(&self) -> f64 {
        self.camera.zoom
    }

    /// The matrix that takes a tile's own units, 0 to `extent` across, to clip space.
    pub fn tile_matrix(&self, tile: PlacedTile, extent: f64) -> DMat4 {
        let scale = self.world_size / f64::from(1u32 << tile.id.z);
        let x =
            (f64::from(tile.id.x) + f64::from(tile.wrap) * f64::from(1u32 << tile.id.z)) * scale;
        let y = f64::from(tile.id.y) * scale;
        self.proj
            * DMat4::from_translation(DVec3::new(x, y, 0.0))
            * DMat4::from_scale(DVec3::new(scale / extent, scale / extent, 1.0))
    }

    /// The screen point of a point in a tile's own units, 0 to `extent` across.
    pub fn tile_point_to_screen(
        &self,
        tile: PlacedTile,
        point: [f32; 2],
        extent: f64,
    ) -> Option<DVec2> {
        let n = f64::from(1u32 << tile.id.z);
        let scale = self.world_size / n;
        let x = (f64::from(tile.id.x) + f64::from(tile.wrap) * n + f64::from(point[0]) / extent)
            * scale;
        let y = (f64::from(tile.id.y) + f64::from(point[1]) / extent) * scale;
        self.world_to_screen(DVec3::new(x, y, 0.0))
    }

    /// Takes a layer's local space, meters with +X east, +Y up and -Z north on the ground at
    /// `anchor`, to the camera's view space; `None` while `anchor` is past the globe's horizon.
    pub fn layer_view(&self, anchor: LatLng) -> Option<DMat4> {
        let (mx, my) = mercator(anchor);
        let up = globe_unit(mx, my);
        if self.globe > 0.0 && up.dot(self.camera_dir) < self.horizon {
            return None;
        }
        let meters = self.world_size / (EARTH_CIRCUMFERENCE * anchor.lat.to_radians().cos());
        let flat =
            DMat4::from_translation(DVec3::new(mx * self.world_size, my * self.world_size, 0.0))
                * DMat4::from_cols(
                    DVec4::new(meters, 0.0, 0.0, 0.0),
                    DVec4::new(0.0, 0.0, meters, 0.0),
                    DVec4::new(0.0, meters, 0.0, 0.0),
                    DVec4::W,
                );
        if self.globe <= 0.0 {
            return Some(self.eye * flat);
        }
        let east = DVec3::new(-up.y, up.x, 0.0).normalize_or(DVec3::Y);
        let north = up.cross(east);
        let unit = 2.0 * std::f64::consts::PI / EARTH_CIRCUMFERENCE;
        let round = self.globe_world
            * DMat4::from_cols(
                (east * unit).extend(0.0),
                (up * unit).extend(0.0),
                (-north * unit).extend(0.0),
                up.extend(1.0),
            );
        let blend = |a: DMat4, b: DMat4| a * (1.0 - self.globe) + b * self.globe;
        Some(self.eye * blend(flat, round))
    }

    /// World pixels per meter at the center's latitude.
    pub fn pixels_per_meter(&self) -> f64 {
        let lat = self.camera.center.lat.to_radians();
        self.world_size / (EARTH_CIRCUMFERENCE * lat.cos())
    }

    /// The screen point of a world point, or `None` behind the camera or past the horizon.
    pub fn world_to_screen(&self, world: DVec3) -> Option<DVec2> {
        let mut clip = self.proj * world.extend(1.0);
        if self.globe > 0.0 {
            let unit = globe_unit(world.x / self.world_size, world.y / self.world_size);
            if unit.dot(self.camera_dir) < self.horizon {
                return None;
            }
            let lift = 1.0 + world.z / self.globe_radius;
            let globe = self.globe_matrix * (unit * lift).extend(1.0);
            clip = clip.lerp(globe, self.globe);
        }
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(DVec2::new(
            (ndc.x + 1.0) / 2.0 * self.width,
            (1.0 - ndc.y) / 2.0 * self.height,
        ))
    }

    pub fn project(&self, place: LatLng) -> Option<DVec2> {
        self.world_to_screen(self.world_point(place).extend(0.0))
    }

    /// The place a screen point lands on, or `None` above the horizon.
    pub fn unproject(&self, point: DVec2) -> Option<LatLng> {
        let world = self.screen_to_world(point)?;
        Some(unmercator(
            world.x / self.world_size,
            world.y / self.world_size,
        ))
    }

    /// Where the ray through a screen point meets the ground, in world pixels.
    pub fn screen_to_world(&self, point: DVec2) -> Option<DVec2> {
        if self.globe >= 0.5 {
            let unit = self.screen_to_unit(point)?;
            let (x, y) = mercator(unit_place(unit));
            let x = x + ((self.center.x / self.world_size) - x + 0.5).floor();
            return Some(DVec2::new(x * self.world_size, y * self.world_size));
        }
        self.screen_to_plane(point)
    }

    /// Where the ray through a screen point first meets the globe, as an Earth-fixed unit vector.
    pub fn screen_to_unit(&self, point: DVec2) -> Option<DVec3> {
        let ndc_x = point.x / self.width * 2.0 - 1.0;
        let ndc_y = 1.0 - point.y / self.height * 2.0;
        let a = self.globe_inverse * DVec4::new(ndc_x, ndc_y, 0.0, 1.0);
        let b = self.globe_inverse * DVec4::new(ndc_x, ndc_y, 1.0, 1.0);
        let origin = a.truncate() / a.w;
        let dir = (b.truncate() / b.w - origin).normalize();
        let half_b = origin.dot(dir);
        let discriminant = half_b * half_b - (origin.length_squared() - 1.0);
        if discriminant < 0.0 {
            return None;
        }
        let t = -half_b - discriminant.sqrt();
        (t >= 0.0).then(|| (origin + dir * t).normalize())
    }

    fn screen_to_plane(&self, point: DVec2) -> Option<DVec2> {
        let ndc_x = point.x / self.width * 2.0 - 1.0;
        let ndc_y = 1.0 - point.y / self.height * 2.0;
        let a = self.inverse * DVec4::new(ndc_x, ndc_y, 0.0, 1.0);
        let b = self.inverse * DVec4::new(ndc_x, ndc_y, 1.0, 1.0);
        let a = a.truncate() / a.w;
        let b = b.truncate() / b.w;
        let dz = b.z - a.z;
        if dz.abs() < 1e-12 {
            return None;
        }
        let t = -a.z / dz;
        if t < 0.0 {
            return None;
        }
        let hit = a + (b - a) * t;
        // Points on the far plane or past it have no ground under them.
        if t > 1.0 {
            return None;
        }
        Some(hit.truncate())
    }

    pub fn world_point(&self, place: LatLng) -> DVec2 {
        let (x, y) = mercator(place);
        DVec2::new(x * self.world_size, y * self.world_size)
    }

    pub fn place_at_world(&self, world: DVec2) -> LatLng {
        unmercator(world.x / self.world_size, world.y / self.world_size)
    }

    /// The ground the screen shows, as world points around its edge.
    pub fn ground_outline(&self) -> Vec<DVec2> {
        let mut points = Vec::new();
        let steps = 8;
        let (w, h) = (self.width, self.height);
        let edge = |t: f64, side: usize| match side {
            0 => DVec2::new(w * t, 0.0),
            1 => DVec2::new(w, h * t),
            2 => DVec2::new(w * (1.0 - t), h),
            _ => DVec2::new(0.0, h * (1.0 - t)),
        };
        for side in 0..4 {
            for step in 0..steps {
                let point = edge(f64::from(step) / f64::from(steps), side);
                if let Some(world) = self.screen_to_plane(point) {
                    points.push(world);
                } else if let Some(world) = self.horizon_point(point) {
                    points.push(world);
                }
            }
        }
        points
    }

    /// For a point above the horizon, the farthest ground point straight below it on screen.
    fn horizon_point(&self, point: DVec2) -> Option<DVec2> {
        let mut low = point.y;
        let mut high = self.height;
        self.screen_to_plane(DVec2::new(point.x, high))?;
        for _ in 0..24 {
            let mid = (low + high) / 2.0;
            if self.screen_to_plane(DVec2::new(point.x, mid)).is_some() {
                high = mid;
            } else {
                low = mid;
            }
        }
        self.screen_to_plane(DVec2::new(point.x, high))
    }

    /// The tiles at `z` that cover the screen, nearest the center first.
    ///
    /// With a tilt, tiles far from the camera come from lower zooms, down to `min_z`.
    pub fn covering_tiles(&self, z: u8, min_z: u8, max_tiles: usize) -> Vec<PlacedTile> {
        let outline = self.ground_outline();
        if outline.len() < 3 {
            return Vec::new();
        }
        let tiles_at = |z: u8| f64::from(1u32 << z);
        let scale = tiles_at(z) / self.world_size;
        let (mut min, mut max) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
        for point in &outline {
            min = min.min(*point * scale);
            max = max.max(*point * scale);
        }
        let center_tile = self.center * scale;
        let count = tiles_at(z) as i64;
        let y0 = min.y.floor().max(0.0) as i64;
        let y1 = (max.y.floor() as i64).min(count - 1);
        let x0 = min.x.floor() as i64;
        let x1 = max.x.floor() as i64;
        let polygon: Vec<DVec2> = convex_hull(outline.iter().map(|p| *p * scale).collect());
        let camera_tile = self.camera_ground_position() * scale;
        let pitched = self.camera.pitch > 5.0;

        let mut tiles = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let lo = DVec2::new(x as f64, y as f64);
                if !square_meets_polygon(lo, 1.0, &polygon) {
                    continue;
                }
                let middle = lo + DVec2::splat(0.5);
                let tile_z = if pitched {
                    // Each halving of the distance from the camera earns one more zoom level.
                    let distance = (middle - camera_tile).length() / tiles_at(z) * self.world_size;
                    let near = self.camera_to_center.max(1.0);
                    let drop = (distance / near).log2().max(0.0).floor() as u8;
                    z.saturating_sub(drop).max(min_z)
                } else {
                    z
                };
                let wrap = x.div_euclid(count);
                let wx = x.rem_euclid(count) as u32;
                let id = TileId::new(z, wx, y as u32).ancestor(tile_z);
                let distance = (middle - center_tile).length_squared();
                tiles.push((
                    distance,
                    PlacedTile {
                        id,
                        wrap: wrap as i32,
                    },
                ));
            }
        }
        tiles.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<PlacedTile> = tiles
            .into_iter()
            .filter_map(|(_, tile)| seen.insert(tile).then_some(tile))
            .collect();
        out.truncate(max_tiles);
        out
    }

    /// The tiles at `z` on the visible side of the globe, nearest the center first.
    pub fn covering_tiles_globe(&self, z: u8, max_tiles: usize) -> Vec<PlacedTile> {
        let count = 1i64 << z;
        let tile_of = |unit: DVec3| {
            let (x, y) = mercator(unit_place(unit));
            let x = ((x * count as f64).floor() as i64).rem_euclid(count);
            let y = ((y * count as f64).floor() as i64).clamp(0, count - 1);
            (x, y)
        };
        let mut seen = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        let mut seed = |tile: (i64, i64), queue: &mut std::collections::VecDeque<(i64, i64)>| {
            if seen.insert(tile) {
                queue.push_back(tile);
            }
        };
        let (cx, cy) = mercator(self.camera.center);
        seed(tile_of(globe_unit(cx, cy)), &mut queue);
        let steps = 12;
        for i in 0..=steps {
            for j in 0..=steps {
                let point = DVec2::new(
                    self.width * f64::from(i) / f64::from(steps),
                    self.height * f64::from(j) / f64::from(steps),
                );
                if let Some(unit) = self.screen_to_unit(point) {
                    seed(tile_of(unit), &mut queue);
                }
            }
        }
        // Grow out to the neighbors that show, which catches tiles squeezed against the limb.
        let mut tiles = Vec::new();
        while let Some((x, y)) = queue.pop_front() {
            tiles.push((x, y));
            if tiles.len() >= max_tiles * 2 {
                break;
            }
            for (dx, dy) in [
                (-1, 0),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, -1),
                (1, 1),
                (-1, 1),
                (1, -1),
            ] {
                let neighbor = ((x + dx).rem_euclid(count), y + dy);
                if neighbor.1 < 0 || neighbor.1 >= count || seen.contains(&neighbor) {
                    continue;
                }
                if self.tile_shows_on_globe(z, neighbor) {
                    seen.insert(neighbor);
                    queue.push_back(neighbor);
                }
            }
        }
        let center = globe_unit(cx, cy);
        let n = count as f64;
        let mut placed: Vec<(f64, PlacedTile)> = tiles
            .into_iter()
            .map(|(x, y)| {
                let middle = globe_unit((x as f64 + 0.5) / n, (y as f64 + 0.5) / n);
                // The copy nearest the center, which Mercator draws while the globe flattens.
                let wrap = (cx - (x as f64 + 0.5) / n).round() as i32;
                let tile = PlacedTile {
                    id: TileId::new(z, x as u32, y as u32),
                    wrap,
                };
                (-middle.dot(center), tile)
            })
            .collect();
        placed.sort_by(|a, b| a.0.total_cmp(&b.0));
        placed.truncate(max_tiles);
        placed.into_iter().map(|(_, tile)| tile).collect()
    }

    /// Whether any of a grid of points in the tile is in front of the horizon and on screen.
    fn tile_shows_on_globe(&self, z: u8, (x, y): (i64, i64)) -> bool {
        let n = f64::from(1u32 << z);
        let margin = 0.25 * self.width.max(self.height);
        (0..=4).any(|i| {
            (0..=4).any(|j| {
                let mx = (x as f64 + f64::from(i) / 4.0) / n;
                let my = (y as f64 + f64::from(j) / 4.0) / n;
                let unit = globe_unit(mx, my);
                if unit.dot(self.camera_dir) < self.horizon {
                    return false;
                }
                let clip = self.globe_matrix * unit.extend(1.0);
                if clip.w <= 0.0 {
                    return false;
                }
                let x = (clip.x / clip.w + 1.0) / 2.0 * self.width;
                let y = (1.0 - clip.y / clip.w) / 2.0 * self.height;
                x > -margin && x < self.width + margin && y > -margin && y < self.height + margin
            })
        })
    }

    /// The point on the ground below the camera, in world pixels.
    pub fn camera_ground_position(&self) -> DVec2 {
        let pitch = f64::from(self.camera.pitch).to_radians();
        let bearing = f64::from(self.camera.bearing).to_radians();
        let back = pitch.sin() * self.camera_to_center;
        self.center - DVec2::new(bearing.sin(), -bearing.cos()) * back
    }
}

fn convex_hull(mut points: Vec<DVec2>) -> Vec<DVec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<DVec2> = Vec::new();
    for &p in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<DVec2> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Whether an axis-aligned square meets a convex polygon, by separating axes.
fn square_meets_polygon(lo: DVec2, size: f64, polygon: &[DVec2]) -> bool {
    if polygon.len() < 3 {
        return true;
    }
    let square = [
        lo,
        lo + DVec2::new(size, 0.0),
        lo + DVec2::new(size, size),
        lo + DVec2::new(0.0, size),
    ];
    let mut axes = vec![DVec2::X, DVec2::Y];
    for i in 0..polygon.len() {
        let edge = polygon[(i + 1) % polygon.len()] - polygon[i];
        axes.push(edge.perp());
    }
    axes.iter().all(|axis| {
        let range = |points: &[DVec2]| {
            points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                let d = p.dot(*axis);
                (lo.min(d), hi.max(d))
            })
        };
        let (a0, a1) = range(&square);
        let (b0, b1) = range(polygon);
        a1 >= b0 && b1 >= a0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(zoom: f64, bearing: f32, pitch: f32) -> MapCamera {
        MapCamera {
            center: LatLng::new(35.68, 139.76),
            zoom,
            bearing,
            pitch,
        }
    }

    #[test]
    fn center_projects_to_screen_center() {
        for (bearing, pitch) in [(0.0, 0.0), (30.0, 0.0), (45.0, 60.0)] {
            let view = View::new(camera(12.0, bearing, pitch), 800.0, 600.0);
            let point = view.project(view.camera.center).unwrap();
            assert!((point.x - 400.0).abs() < 1e-6, "{point:?}");
            assert!((point.y - 300.0).abs() < 1e-6, "{point:?}");
        }
    }

    #[test]
    fn unproject_inverts_project() {
        let view = View::new(camera(14.0, 20.0, 50.0), 800.0, 600.0);
        let place = view.unproject(DVec2::new(100.0, 500.0)).unwrap();
        let back = view.project(place).unwrap();
        assert!((back - DVec2::new(100.0, 500.0)).length() < 1e-6);
    }

    #[test]
    fn one_zoom_level_doubles_distances() {
        let a = View::new(camera(10.0, 0.0, 0.0), 800.0, 600.0);
        let b = View::new(camera(11.0, 0.0, 0.0), 800.0, 600.0);
        let east = LatLng::new(35.68, 139.8);
        let da = a.project(east).unwrap().x - 400.0;
        let db = b.project(east).unwrap().x - 400.0;
        assert!((db / da - 2.0).abs() < 1e-9);
    }

    #[test]
    fn north_up_has_north_at_top() {
        let view = View::new(camera(10.0, 0.0, 0.0), 800.0, 600.0);
        let north = view.project(LatLng::new(35.8, 139.76)).unwrap();
        assert!(north.y < 300.0);
        let turned = View::new(camera(10.0, 90.0, 0.0), 800.0, 600.0);
        let north = turned.project(LatLng::new(35.8, 139.76)).unwrap();
        assert!(
            north.x < 400.0,
            "bearing 90 looks east, so north is left: {north:?}"
        );
    }

    fn globe(zoom: f64) -> View {
        View::projected(camera(zoom, 0.0, 0.0), 800.0, 600.0, MapProjection::Globe)
    }

    #[test]
    fn globe_center_and_nearby_places_match_mercator() {
        let flat = View::new(camera(11.0, 0.0, 0.0), 800.0, 600.0);
        let round = globe(11.0);
        assert_eq!(round.globe, 1.0);
        let center = round.project(round.camera.center).unwrap();
        assert!(
            (center - DVec2::new(400.0, 300.0)).length() < 1e-6,
            "{center:?}"
        );
        for place in [LatLng::new(35.68, 139.77), LatLng::new(35.69, 139.76)] {
            let a = flat.project(place).unwrap();
            let b = round.project(place).unwrap();
            assert!((a - b).length() < 0.5, "{a:?} {b:?}");
        }
        assert_eq!(globe(12.0).globe, 0.0);
        assert_eq!(globe(11.5).globe, 0.5);
    }

    #[test]
    fn globe_unproject_inverts_project_and_hides_the_far_side() {
        let view = globe(2.0);
        let place = view.unproject(DVec2::new(250.0, 380.0)).unwrap();
        let back = view.project(place).unwrap();
        assert!(
            (back - DVec2::new(250.0, 380.0)).length() < 1e-6,
            "{back:?}"
        );
        assert!(view.project(LatLng::new(-35.68, -40.24)).is_none());
        assert!(view.unproject(DVec2::new(0.0, 0.0)).is_none());
    }

    #[test]
    fn globe_covers_the_visible_side_once() {
        let view = globe(1.5);
        let tiles = view.covering_tiles_globe(1, 64);
        assert_eq!(tiles.len(), 4, "{tiles:?}");
        let tiles = globe(4.0).covering_tiles_globe(4, 200);
        let ids: std::collections::HashSet<_> = tiles.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), tiles.len());
        assert!(
            tiles.iter().any(|t| t.wrap == 1),
            "tiles past 180° sit east of the center"
        );
        assert!(
            tiles.contains(&PlacedTile {
                id: TileId::new(4, 14, 6),
                wrap: 0
            }),
            "{tiles:?}"
        );
    }

    #[test]
    fn covers_the_screen() {
        let view = View::new(camera(2.0, 0.0, 0.0), 1024.0, 1024.0);
        let tiles = view.covering_tiles(2, 0, 64);
        assert!(tiles.len() >= 4 && tiles.len() <= 16, "{tiles:?}");
        let pitched = View::new(camera(14.0, 0.0, 70.0), 800.0, 600.0);
        let tiles = pitched.covering_tiles(14, 0, 200);
        assert!(tiles.iter().any(|t| t.id.z < 14));
        assert!(tiles.iter().any(|t| t.id.z == 14));
    }

    /// Where `local`, in the layer space around `anchor`, lands on screen.
    fn layer_point(view: &View, anchor: LatLng, local: DVec3) -> DVec2 {
        let clip = view.perspective * view.layer_view(anchor).unwrap() * local.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        DVec2::new(
            (ndc.x + 1.0) / 2.0 * view.width,
            (1.0 - ndc.y) / 2.0 * view.height,
        )
    }

    /// `place` moved `east` and `north` meters, on a sphere.
    fn offset(place: LatLng, east: f64, north: f64) -> LatLng {
        let degrees = 360.0 / EARTH_CIRCUMFERENCE;
        LatLng::new(
            place.lat + north * degrees,
            place.lng + east * degrees / place.lat.to_radians().cos(),
        )
    }

    #[test]
    fn layer_space_is_meters_east_up_and_south_of_the_anchor() {
        let view = View::new(camera(16.0, 30.0, 50.0), 800.0, 600.0);
        let anchor = offset(view.camera.center, 120.0, -80.0);
        for (local, east, north) in [
            (DVec3::ZERO, 0.0, 0.0),
            (DVec3::new(100.0, 0.0, 0.0), 100.0, 0.0),
            (DVec3::new(0.0, 0.0, -100.0), 0.0, 100.0),
            (DVec3::new(-60.0, 0.0, 40.0), -60.0, -40.0),
        ] {
            let drawn = layer_point(&view, anchor, local);
            let expected = view.project(offset(anchor, east, north)).unwrap();
            assert!(
                drawn.distance(expected) < 0.05,
                "{local}: {drawn} vs {expected}"
            );
        }
    }

    #[test]
    fn layer_space_winds_like_a_scene_and_scales_evenly() {
        let view = View::new(camera(17.0, -40.0, 60.0), 800.0, 600.0);
        let local = view.layer_view(view.camera.center).unwrap();
        assert!(local.determinant() > 0.0);
        let lengths =
            [local.x_axis, local.y_axis, local.z_axis].map(|axis| axis.truncate().length());
        assert!(
            (lengths[0] - lengths[1]).abs() < lengths[0] * 1e-9,
            "{lengths:?}"
        );
        assert!(
            (lengths[0] - lengths[2]).abs() < lengths[0] * 1e-9,
            "{lengths:?}"
        );
    }

    #[test]
    fn on_the_globe_layer_space_sits_on_the_tangent_plane_and_hides_past_the_horizon() {
        let view = View::projected(camera(3.0, 20.0, 0.0), 800.0, 600.0, MapProjection::Globe);
        assert_eq!(view.globe, 1.0);
        let anchor = offset(view.camera.center, 300_000.0, 200_000.0);
        for (local, east, north) in [
            (DVec3::ZERO, 0.0, 0.0),
            (DVec3::new(2_000.0, 0.0, 0.0), 2_000.0, 0.0),
            (DVec3::new(0.0, 0.0, -2_000.0), 0.0, 2_000.0),
        ] {
            let drawn = layer_point(&view, anchor, local);
            let expected = view.project(offset(anchor, east, north)).unwrap();
            assert!(
                drawn.distance(expected) < 0.01,
                "{local}: {drawn} vs {expected}"
            );
        }
        let far_side = LatLng::new(-view.camera.center.lat, view.camera.center.lng - 180.0);
        assert!(view.layer_view(far_side).is_none());
    }
}
