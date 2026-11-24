//! GeoJSON data cut into vector tiles, with optional point clustering.

use std::collections::HashMap;
use std::sync::Arc;

use glam::DVec2;
use serde_json::Value as Json;

use super::mvt::{Feature, GeometryKind, Layer, TilePoint, signed_area};
use crate::geo::{LatLng, TileId, mercator};
use crate::style::GeoJsonOptions;
use crate::value::Value;

/// The source-layer name that a GeoJSON source's style layers read, which have none of their own.
pub(crate) const GEOJSON_LAYER: &str = "_geojson";

const TILE_EXTENT: u32 = 4096;

#[derive(Clone, Debug)]
enum Shape {
    Points(Vec<DVec2>),
    Lines(Vec<Vec<DVec2>>),
    /// Polygons, each an outer ring followed by its holes.
    Polygons(Vec<Vec<Vec<DVec2>>>),
}

#[derive(Clone, Debug)]
struct Item {
    id: Option<u64>,
    properties: Arc<HashMap<Arc<str>, Value>>,
    shape: Shape,
    /// `[min_x, min_y, max_x, max_y]` in world units, 0 to 1 across.
    bbox: [f64; 4],
}

/// A point or cluster at one zoom of the cluster hierarchy.
#[derive(Clone, Debug)]
struct Node {
    position: DVec2,
    count: usize,
    /// The input point's index when `count` is 1, else the cluster's id.
    id: u64,
}

/// A GeoJSON source's features, ready to be cut into any tile.
pub(crate) struct GeoJsonIndex {
    items: Vec<Item>,
    /// Points and clusters per zoom, up to `cluster_max_zoom`; empty without clustering.
    levels: Vec<Vec<Node>>,
    points: Vec<Item>,
    options: GeoJsonOptions,
}

impl GeoJsonIndex {
    pub fn new(data: &Json, options: GeoJsonOptions) -> GeoJsonIndex {
        let mut items = Vec::new();
        collect(data, None, &Arc::new(HashMap::new()), &mut items);
        let (points, items): (Vec<Item>, Vec<Item>) = if options.cluster {
            items
                .into_iter()
                .partition(|item| matches!(item.shape, Shape::Points(ref p) if p.len() == 1))
        } else {
            (Vec::new(), items)
        };
        let levels = if options.cluster {
            cluster(&points, &options)
        } else {
            Vec::new()
        };
        GeoJsonIndex {
            items,
            levels,
            points,
            options,
        }
    }

    /// The tile's features as one MVT layer named [`GEOJSON_LAYER`].
    pub fn tile(&self, id: TileId) -> Layer {
        let n = f64::from(1u32 << id.z);
        let buffer = self.options.buffer / 512.0 / n;
        let bounds = [
            f64::from(id.x) / n - buffer,
            f64::from(id.y) / n - buffer,
            f64::from(id.x + 1) / n + buffer,
            f64::from(id.y + 1) / n + buffer,
        ];
        let tolerance = self.options.tolerance / 512.0 / n;
        let to_tile = |p: DVec2| -> TilePoint {
            let x = (p.x * n - f64::from(id.x)) * f64::from(TILE_EXTENT);
            let y = (p.y * n - f64::from(id.y)) * f64::from(TILE_EXTENT);
            [x.round() as i32, y.round() as i32]
        };
        let mut features = Vec::new();
        let overlaps = |b: [f64; 4]| {
            b[0] <= bounds[2] && b[2] >= bounds[0] && b[1] <= bounds[3] && b[3] >= bounds[1]
        };
        let unclustered: &[Item] =
            if self.levels.is_empty() || usize::from(id.z) >= self.levels.len() {
                &self.points
            } else {
                &[]
            };
        for item in self
            .items
            .iter()
            .chain(unclustered)
            .filter(|item| overlaps(item.bbox))
        {
            if let Some(feature) = cut(item, bounds, tolerance, &to_tile) {
                features.push(feature);
            }
        }
        if let Some(level) = self.levels.get(usize::from(id.z)) {
            for node in level {
                let p = node.position;
                if p.x < bounds[0] || p.x > bounds[2] || p.y < bounds[1] || p.y > bounds[3] {
                    continue;
                }
                if node.count == 1 {
                    let item = &self.points[node.id as usize];
                    features.push(Feature {
                        id: item.id,
                        kind: GeometryKind::Point,
                        properties: (*item.properties).clone(),
                        geometry: vec![vec![to_tile(p)]],
                    });
                } else {
                    features.push(Feature {
                        id: Some(node.id),
                        kind: GeometryKind::Point,
                        properties: cluster_properties(node),
                        geometry: vec![vec![to_tile(p)]],
                    });
                }
            }
        }
        Layer {
            name: GEOJSON_LAYER.to_string(),
            extent: TILE_EXTENT,
            features,
        }
    }
}

fn cluster_properties(node: &Node) -> HashMap<Arc<str>, Value> {
    let count = node.count;
    let abbreviated = if count >= 10_000 {
        format!("{}k", (count as f64 / 1000.0).round())
    } else if count >= 1000 {
        format!("{:.1}k", count as f64 / 1000.0)
    } else {
        count.to_string()
    };
    HashMap::from([
        (Arc::from("cluster"), Value::Bool(true)),
        (Arc::from("cluster_id"), Value::Number(node.id as f64)),
        (Arc::from("point_count"), Value::Number(count as f64)),
        (
            Arc::from("point_count_abbreviated"),
            Value::String(abbreviated.into()),
        ),
    ])
}

/// Groups points per zoom as supercluster does: each zoom merges the previous zoom's nodes that lie
/// within `cluster_radius` pixels of one another.
fn cluster(points: &[Item], options: &GeoJsonOptions) -> Vec<Vec<Node>> {
    let max = usize::from(options.cluster_max_zoom.min(24));
    let mut level: Vec<Node> = points
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match &item.shape {
            Shape::Points(p) => Some(Node {
                position: p[0],
                count: 1,
                id: i as u64,
            }),
            _ => None,
        })
        .collect();
    let mut levels = vec![Vec::new(); max + 1];
    let mut next_id = 0u64;
    for z in (0..=max).rev() {
        let radius = options.cluster_radius / (512.0 * f64::from(1u32 << z));
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        let cell = |p: DVec2| ((p.x / radius).floor() as i64, (p.y / radius).floor() as i64);
        for (i, node) in level.iter().enumerate() {
            grid.entry(cell(node.position)).or_default().push(i);
        }
        let mut taken = vec![false; level.len()];
        let mut merged = Vec::with_capacity(level.len());
        for i in 0..level.len() {
            if taken[i] {
                continue;
            }
            taken[i] = true;
            let center = level[i].position;
            let (cx, cy) = cell(center);
            let mut neighbors = Vec::new();
            for gx in cx - 1..=cx + 1 {
                for gy in cy - 1..=cy + 1 {
                    for &j in grid.get(&(gx, gy)).map_or(&[][..], Vec::as_slice) {
                        if !taken[j] && (level[j].position - center).length() <= radius {
                            neighbors.push(j);
                        }
                    }
                }
            }
            let count = level[i].count + neighbors.iter().map(|&j| level[j].count).sum::<usize>();
            if neighbors.is_empty() || count < options.cluster_min_points {
                merged.push(level[i].clone());
                continue;
            }
            let mut weighted = center * level[i].count as f64;
            for &j in &neighbors {
                taken[j] = true;
                weighted += level[j].position * level[j].count as f64;
            }
            next_id += 1;
            merged.push(Node {
                position: weighted / count as f64,
                count,
                id: (next_id << 5) | z as u64,
            });
        }
        levels[z] = merged.clone();
        level = merged;
    }
    levels
}

fn cut(
    item: &Item,
    bounds: [f64; 4],
    tolerance: f64,
    to_tile: &dyn Fn(DVec2) -> TilePoint,
) -> Option<Feature> {
    let inside =
        |p: &DVec2| p.x >= bounds[0] && p.x <= bounds[2] && p.y >= bounds[1] && p.y <= bounds[3];
    let (kind, geometry): (GeometryKind, Vec<Vec<TilePoint>>) = match &item.shape {
        Shape::Points(points) => (
            GeometryKind::Point,
            vec![
                points
                    .iter()
                    .filter(|p| inside(p))
                    .map(|p| to_tile(*p))
                    .collect(),
            ],
        ),
        Shape::Lines(lines) => {
            let mut out = Vec::new();
            for line in lines {
                for piece in clip_line(&simplify(line, tolerance), bounds) {
                    let piece = dedup(piece.into_iter().map(to_tile).collect());
                    if piece.len() >= 2 {
                        out.push(piece);
                    }
                }
            }
            (GeometryKind::Line, out)
        }
        Shape::Polygons(polygons) => {
            let mut out = Vec::new();
            for polygon in polygons {
                for (k, ring) in polygon.iter().enumerate() {
                    let clipped = clip_ring(&simplify(ring, tolerance), bounds);
                    let mut ring = dedup(clipped.into_iter().map(to_tile).collect());
                    if ring.len() < 3 {
                        if k == 0 {
                            break;
                        }
                        continue;
                    }
                    ring.push(ring[0]);
                    let area = signed_area(&ring);
                    if (k == 0) != (area > 0.0) {
                        ring.reverse();
                    }
                    out.push(ring);
                }
            }
            (GeometryKind::Polygon, out)
        }
    };
    if geometry.iter().all(Vec::is_empty) {
        return None;
    }
    Some(Feature {
        id: item.id,
        kind,
        properties: (*item.properties).clone(),
        geometry,
    })
}

fn dedup(mut points: Vec<TilePoint>) -> Vec<TilePoint> {
    points.dedup();
    points
}

/// Douglas–Peucker simplification to `tolerance` world units.
fn simplify(points: &[DVec2], tolerance: f64) -> Vec<DVec2> {
    if points.len() <= 2 || tolerance <= 0.0 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        let (a, b) = (points[first], points[last]);
        let mut farthest = (0.0, first);
        for (i, p) in points.iter().enumerate().take(last).skip(first + 1) {
            let d = segment_distance(*p, a, b);
            if d > farthest.0 {
                farthest = (d, i);
            }
        }
        if farthest.0 > tolerance {
            keep[farthest.1] = true;
            stack.push((first, farthest.1));
            stack.push((farthest.1, last));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| *p)
        .collect()
}

fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let d = b - a;
    let t = if d.length_squared() > 0.0 {
        ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t - p).length()
}

/// The pieces of `line` inside `bounds`.
fn clip_line(line: &[DVec2], bounds: [f64; 4]) -> Vec<Vec<DVec2>> {
    let mut pieces = Vec::new();
    let mut current: Vec<DVec2> = Vec::new();
    for w in line.windows(2) {
        match clip_segment(w[0], w[1], bounds) {
            Some((a, b)) => {
                if current.last() != Some(&a) {
                    if current.len() >= 2 {
                        pieces.push(std::mem::take(&mut current));
                    }
                    current.clear();
                    current.push(a);
                }
                current.push(b);
                if b != w[1] {
                    pieces.push(std::mem::take(&mut current));
                }
            }
            None => {
                if current.len() >= 2 {
                    pieces.push(std::mem::take(&mut current));
                }
                current.clear();
            }
        }
    }
    if current.len() >= 2 {
        pieces.push(current);
    }
    pieces
}

/// Liang–Barsky clipping of one segment.
fn clip_segment(a: DVec2, b: DVec2, bounds: [f64; 4]) -> Option<(DVec2, DVec2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.x, a.x - bounds[0]),
        (d.x, bounds[2] - a.x),
        (-d.y, a.y - bounds[1]),
        (d.y, bounds[3] - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then(|| (a + d * t0, a + d * t1))
}

/// Sutherland–Hodgman clipping of a closed ring.
fn clip_ring(ring: &[DVec2], bounds: [f64; 4]) -> Vec<DVec2> {
    let mut points: Vec<DVec2> = ring.to_vec();
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    let edges: [(usize, f64, bool); 4] = [
        (0, bounds[0], true),
        (0, bounds[2], false),
        (1, bounds[1], true),
        (1, bounds[3], false),
    ];
    for (axis, value, keep_greater) in edges {
        if points.is_empty() {
            break;
        }
        let inside = |p: DVec2| {
            if keep_greater {
                p[axis] >= value
            } else {
                p[axis] <= value
            }
        };
        let mut out = Vec::with_capacity(points.len() + 4);
        for i in 0..points.len() {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            if inside(a) {
                out.push(a);
            }
            if inside(a) != inside(b) {
                let t = (value - a[axis]) / (b[axis] - a[axis]);
                out.push(a + (b - a) * t);
            }
        }
        points = out;
    }
    points
}

fn collect(
    json: &Json,
    id: Option<u64>,
    properties: &Arc<HashMap<Arc<str>, Value>>,
    out: &mut Vec<Item>,
) {
    match json.get("type").and_then(Json::as_str) {
        Some("FeatureCollection") => {
            for feature in json
                .get("features")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
            {
                collect(feature, None, properties, out);
            }
        }
        Some("Feature") => {
            let properties = Arc::new(
                json.get("properties")
                    .and_then(Json::as_object)
                    .map(|map| {
                        map.iter()
                            .map(|(k, v)| (Arc::from(k.as_str()), Value::from_json(v)))
                            .collect()
                    })
                    .unwrap_or_default(),
            );
            let id = json.get("id").and_then(Json::as_u64);
            if let Some(geometry) = json.get("geometry") {
                collect(geometry, id, &properties, out);
            }
        }
        Some("GeometryCollection") => {
            for geometry in json
                .get("geometries")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
            {
                collect(geometry, id, properties, out);
            }
        }
        Some(kind) => {
            let Some(coordinates) = json.get("coordinates") else {
                return;
            };
            let point = |c: &Json| -> Option<DVec2> {
                let c = c.as_array()?;
                let (x, y) = mercator(LatLng::new(c.get(1)?.as_f64()?, c.first()?.as_f64()?));
                Some(DVec2::new(x, y))
            };
            let line = |c: &Json| -> Vec<DVec2> {
                c.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(point)
                    .collect()
            };
            let polygon = |c: &Json| -> Vec<Vec<DVec2>> {
                c.as_array().into_iter().flatten().map(line).collect()
            };
            let list = |c: &Json| c.as_array().cloned().unwrap_or_default();
            let shape = match kind {
                "Point" => Shape::Points(point(coordinates).into_iter().collect()),
                "MultiPoint" => Shape::Points(line(coordinates)),
                "LineString" => Shape::Lines(vec![line(coordinates)]),
                "MultiLineString" => Shape::Lines(list(coordinates).iter().map(line).collect()),
                "Polygon" => Shape::Polygons(vec![polygon(coordinates)]),
                "MultiPolygon" => Shape::Polygons(list(coordinates).iter().map(polygon).collect()),
                _ => return,
            };
            let all: Vec<DVec2> = match &shape {
                Shape::Points(p) => p.clone(),
                Shape::Lines(l) => l.iter().flatten().copied().collect(),
                Shape::Polygons(p) => p.iter().flatten().flatten().copied().collect(),
            };
            if all.is_empty() {
                return;
            }
            let bbox = all
                .iter()
                .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| {
                    [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)]
                });
            out.push(Item {
                id,
                properties: properties.clone(),
                shape,
                bbox,
            });
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(cluster: bool) -> GeoJsonOptions {
        GeoJsonOptions {
            cluster,
            cluster_radius: 50.0,
            cluster_max_zoom: 14,
            cluster_min_points: 2,
            max_zoom: 18,
            tolerance: 0.375,
            buffer: 128.0,
        }
    }

    #[test]
    fn polygons_are_clipped_to_the_tile_with_outer_rings_clockwise() {
        let data = serde_json::json!({
            "type": "Feature",
            "properties": {"name": "box"},
            "geometry": {"type": "Polygon", "coordinates": [[[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0], [-10.0, -10.0]]]}
        });
        let index = GeoJsonIndex::new(&data, options(false));
        let layer = index.tile(TileId::new(1, 1, 1));
        assert_eq!(layer.features.len(), 1);
        let ring = &layer.features[0].geometry[0];
        assert!(signed_area(ring) > 0.0);
        assert!(ring.iter().all(|p| p[0] >= -1024 && p[1] <= 4096 + 1024));
        assert!(index.tile(TileId::new(2, 0, 0)).features.is_empty());
    }

    #[test]
    fn nearby_points_cluster_until_they_separate() {
        let points: Vec<Json> = (0..10)
            .map(|i| serde_json::json!({"type": "Feature", "properties": {}, "geometry": {"type": "Point", "coordinates": [139.7 + f64::from(i) * 0.001, 35.68]}}))
            .collect();
        let data = serde_json::json!({"type": "FeatureCollection", "features": points});
        let index = GeoJsonIndex::new(&data, options(true));
        let world = index.tile(TileId::new(0, 0, 0));
        assert_eq!(world.features.len(), 1);
        assert_eq!(
            world.features[0]
                .property("point_count")
                .and_then(Value::as_f64),
            Some(10.0)
        );
        let (x, y) = mercator(LatLng::new(35.68, 139.7));
        let z = 18u8;
        let n = f64::from(1u32 << z);
        let close = index.tile(TileId::new(z, (x * n) as u32, (y * n) as u32));
        assert!(
            close
                .features
                .iter()
                .all(|f| f.property("cluster").is_none())
        );
    }

    #[test]
    fn lines_split_where_they_leave_the_tile() {
        let pieces = clip_line(
            &[
                DVec2::new(-1.0, 0.5),
                DVec2::new(0.5, 0.5),
                DVec2::new(0.5, 2.0),
                DVec2::new(0.6, 0.5),
            ],
            [0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(pieces.len(), 2);
        assert_eq!(
            pieces[0],
            vec![
                DVec2::new(0.0, 0.5),
                DVec2::new(0.5, 0.5),
                DVec2::new(0.5, 1.0)
            ]
        );
    }
}
