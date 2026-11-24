//! The features drawn under a point of the map.

use glam::DVec2;

use crate::camera::View;
use crate::geo::{LatLng, PlacedTile};
use crate::map::StyleState;
use crate::render::{PlacedLabel, TileSet};
use crate::style::LayerKind;
use crate::style::expr::Context;
use crate::tiles::TileContent;
use crate::tiles::bucket::{TileFeature, passes};
use crate::tiles::geojson::GEOJSON_LAYER;
use crate::tiles::mvt::{Feature, GeometryKind};
use crate::value::Value;

/// A tap on the map that was not part of a pan, pinch, or double tap.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct MapTap {
    /// The place under the tap, if the ground is visible there.
    pub position: Option<LatLng>,
    /// The tap in logical pixels from the map's top left.
    pub x: f64,
    pub y: f64,
    /// The features drawn under the tap, topmost first: labels and icons, then style layers in
    /// reverse drawing order.
    pub features: Vec<MapFeature>,
}

/// One feature of a style layer, as its source describes it.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct MapFeature {
    /// The style layer's `id`.
    pub layer: String,
    pub source: String,
    pub source_layer: Option<String>,
    pub id: Option<u64>,
    pub properties: serde_json::Map<String, serde_json::Value>,
}

/// Handles [`MapTap`]s; a closure converts into it.
#[derive(Clone)]
pub struct TapCallback(pub(crate) std::rc::Rc<dyn Fn(MapTap)>);

impl<F: Fn(MapTap) + 'static> From<F> for TapCallback {
    fn from(f: F) -> Self {
        TapCallback(std::rc::Rc::new(f))
    }
}

pub(crate) fn features_at(
    view: &View,
    style: &StyleState,
    tiles: &TileSet,
    labels: &[PlacedLabel],
    point: DVec2,
) -> Vec<MapFeature> {
    let mut out = Vec::new();
    let (px, py) = (point.x as f32, point.y as f32);
    for label in labels {
        let b = label.bounds;
        if px < b[0] || px > b[2] || py < b[1] || py > b[3] {
            continue;
        }
        let TileContent::Vector {
            symbols, features, ..
        } = &label.tile.content
        else {
            continue;
        };
        let layers = features.layers();
        let Some(symbol) = symbols.get(label.symbol) else {
            continue;
        };
        let (Some(layer), Some(source_layer)) = (
            style.layers.get(symbol.layer),
            layers.get(symbol.source_layer),
        ) else {
            continue;
        };
        if let Some(feature) = source_layer.features.get(symbol.feature) {
            out.push(map_feature(
                &layer.id,
                &label.source,
                Some(&source_layer.name),
                feature,
            ));
        }
    }

    let Some(world) = view.screen_to_world(point) else {
        return out;
    };
    let zoom = view.zoom();
    for layer in style.layers.iter().rev() {
        if !matches!(
            layer.kind,
            LayerKind::Fill | LayerKind::FillExtrusion | LayerKind::Line | LayerKind::Circle
        ) || !layer.shown_at(zoom)
        {
            continue;
        }
        let Some(source_id) = layer.source.as_deref() else {
            continue;
        };
        let Some((ideal, source_tiles)) = tiles.get(source_id) else {
            continue;
        };
        if layer.kind == LayerKind::FillExtrusion {
            for (placed, tile) in source_tiles.iter().filter(|(p, _)| p.id.z == *ideal) {
                let TileContent::Vector { features, .. } = &tile.content else {
                    continue;
                };
                let layers = features.layers();
                let Some(source_layer) = layers
                    .iter()
                    .find(|l| l.name == layer.source_layer.as_deref().unwrap_or(GEOJSON_LAYER))
                else {
                    continue;
                };
                for feature in &source_layer.features {
                    let view_feature = TileFeature { feature };
                    if feature.kind != GeometryKind::Polygon || !passes(layer, &view_feature, zoom)
                    {
                        continue;
                    }
                    let ctx = Context::feature(zoom, &view_feature);
                    let top = layer
                        .get("fill-extrusion-height")
                        .number(&ctx, 0.0)
                        .max(0.0);
                    let base = layer
                        .get("fill-extrusion-base")
                        .number(&ctx, 0.0)
                        .clamp(0.0, top);
                    let extent = f64::from(source_layer.extent);
                    if extrusion_hits(view, *placed, extent, feature, [base, top], point)
                        && !out.iter().any(|f: &MapFeature| {
                            f.layer == layer.id && feature.id.is_some() && f.id == feature.id
                        })
                    {
                        out.push(map_feature(
                            &layer.id,
                            source_id,
                            layer.source_layer.as_deref(),
                            feature,
                        ));
                    }
                }
            }
            continue;
        }
        let Some((placed, local, tile)) = source_tiles
            .iter()
            .filter_map(|(placed, tile)| {
                tile_local(view, world, *placed).map(|local| (placed, local, tile))
            })
            .max_by_key(|(placed, _, _)| placed.id.z)
        else {
            continue;
        };
        let TileContent::Vector { features, .. } = &tile.content else {
            continue;
        };
        let layers = features.layers();
        let Some(source_layer) = layers
            .iter()
            .find(|l| l.name == layer.source_layer.as_deref().unwrap_or(GEOJSON_LAYER))
        else {
            continue;
        };
        let extent = f64::from(source_layer.extent);
        let units = extent / (512.0 * 2f64.powf(zoom - f64::from(placed.id.z)));
        let p = [local[0] * extent, local[1] * extent];
        for feature in &source_layer.features {
            let view_feature = TileFeature { feature };
            if !passes(layer, &view_feature, zoom) {
                continue;
            }
            let ctx = Context::feature(zoom, &view_feature);
            let reach = match layer.kind {
                LayerKind::Line => {
                    let width = layer.get("line-width").number(&ctx, 1.0);
                    let gap = layer.get("line-gap-width").number(&ctx, 0.0);
                    (if gap > 0.0 {
                        gap / 2.0 + width
                    } else {
                        width / 2.0
                    }) * units
                }
                LayerKind::Circle => {
                    (layer.get("circle-radius").number(&ctx, 5.0)
                        + layer.get("circle-stroke-width").number(&ctx, 0.0))
                        * units
                }
                _ => 0.0,
            };
            if hits(layer.kind, feature, p, reach) {
                out.push(map_feature(
                    &layer.id,
                    source_id,
                    layer.source_layer.as_deref(),
                    feature,
                ));
            }
        }
    }
    out
}

/// Where `world` falls in the tile, as fractions of its size, if it falls inside.
fn tile_local(view: &View, world: DVec2, tile: PlacedTile) -> Option<[f64; 2]> {
    let n = f64::from(1u32 << tile.id.z);
    let scale = view.world_size / n;
    let x = world.x / scale - (f64::from(tile.id.x) + f64::from(tile.wrap) * n);
    let y = world.y / scale - f64::from(tile.id.y);
    ((0.0..1.0).contains(&x) && (0.0..1.0).contains(&y)).then_some([x, y])
}

/// Whether a layer of `kind` draws `feature` over `p`, with lines and circles reaching `reach` from
/// their geometry; all in the feature's tile units.
pub(crate) fn hits(kind: LayerKind, feature: &Feature, p: [f64; 2], reach: f64) -> bool {
    let near = |a: [i32; 2], b: [i32; 2]| segment_distance(p, a, b) <= reach;
    match kind {
        LayerKind::Fill | LayerKind::FillExtrusion => {
            feature.kind == GeometryKind::Polygon && {
                let rings: Vec<Vec<DVec2>> = feature
                    .geometry
                    .iter()
                    .map(|ring| {
                        ring.iter()
                            .map(|q| DVec2::new(f64::from(q[0]), f64::from(q[1])))
                            .collect()
                    })
                    .collect();
                inside(&rings, DVec2::new(p[0], p[1]))
            }
        }
        LayerKind::Line => {
            feature.kind != GeometryKind::Point
                && feature
                    .geometry
                    .iter()
                    .any(|line| line.windows(2).any(|w| near(w[0], w[1])))
        }
        LayerKind::Circle => feature.geometry.iter().flatten().any(|q| near(*q, *q)),
        _ => false,
    }
}

/// Whether `feature` extruded from `heights[0]` to `heights[1]` meters covers the screen `point`.
fn extrusion_hits(
    view: &View,
    tile: PlacedTile,
    extent: f64,
    feature: &Feature,
    heights: [f64; 2],
    point: DVec2,
) -> bool {
    let n = f64::from(1u32 << tile.id.z);
    let scale = view.world_size / n;
    let origin = DVec2::new(
        f64::from(tile.id.x) + f64::from(tile.wrap) * n,
        f64::from(tile.id.y),
    );
    let meters = view.pixels_per_meter();
    let project = |q: [i32; 2], height: f64| {
        let world = (origin + DVec2::new(f64::from(q[0]), f64::from(q[1])) / extent) * scale;
        view.world_to_screen(world.extend(height * meters))
    };
    let mut floors = [Vec::new(), Vec::new()];
    for ring in &feature.geometry {
        for (floor, height) in floors.iter_mut().zip(heights) {
            let Some(projected) = ring
                .iter()
                .map(|q| project(*q, height))
                .collect::<Option<Vec<DVec2>>>()
            else {
                return false;
            };
            floor.push(projected);
        }
    }
    if floors.iter().any(|rings| inside(rings, point)) {
        return true;
    }
    floors[0].iter().zip(&floors[1]).any(|(bottom, top)| {
        bottom
            .windows(2)
            .zip(top.windows(2))
            .any(|(b, t)| inside(&[vec![b[0], b[1], t[1], t[0], b[0]]], point))
    })
}

/// Even-odd containment of `p` in closed rings.
fn inside(rings: &[Vec<DVec2>], p: DVec2) -> bool {
    let mut inside = false;
    for ring in rings {
        for w in ring.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
    }
    inside
}

fn segment_distance(p: [f64; 2], a: [i32; 2], b: [i32; 2]) -> f64 {
    let a = DVec2::new(f64::from(a[0]), f64::from(a[1]));
    let b = DVec2::new(f64::from(b[0]), f64::from(b[1]));
    let p = DVec2::new(p[0], p[1]);
    let d = b - a;
    let t = if d.length_squared() > 0.0 {
        ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t - p).length()
}

fn map_feature(
    layer: &str,
    source: &str,
    source_layer: Option<&str>,
    feature: &Feature,
) -> MapFeature {
    MapFeature {
        layer: layer.to_string(),
        source: source.to_string(),
        source_layer: source_layer.map(str::to_string),
        id: feature.id,
        properties: feature
            .properties
            .iter()
            .map(|(key, value)| (key.to_string(), json(value)))
            .collect(),
    }
}

fn json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Number(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => {
            serde_json::Value::from(*n as i64)
        }
        Value::Number(n) => serde_json::Number::from_f64(*n)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::String(s) => serde_json::Value::String(s.to_string()),
        Value::Color(c) => serde_json::Value::String(format!("{c:?}")),
        Value::Array(items) => serde_json::Value::Array(items.iter().map(json).collect()),
        Value::Object(map) => serde_json::Value::Object((**map).clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn feature(kind: GeometryKind, geometry: Vec<Vec<[i32; 2]>>) -> Feature {
        Feature {
            id: None,
            kind,
            properties: HashMap::new(),
            geometry,
        }
    }

    #[test]
    fn fills_hit_inside_and_miss_holes() {
        let square = feature(
            GeometryKind::Polygon,
            vec![
                vec![[0, 0], [100, 0], [100, 100], [0, 100], [0, 0]],
                vec![[40, 40], [40, 60], [60, 60], [60, 40], [40, 40]],
            ],
        );
        assert!(hits(LayerKind::Fill, &square, [10.0, 10.0], 0.0));
        assert!(!hits(LayerKind::Fill, &square, [50.0, 50.0], 0.0));
        assert!(!hits(LayerKind::Fill, &square, [150.0, 50.0], 0.0));
    }

    #[test]
    fn lines_and_circles_reach_their_width() {
        let line = feature(GeometryKind::Line, vec![vec![[0, 0], [100, 0]]]);
        assert!(hits(LayerKind::Line, &line, [50.0, 4.0], 5.0));
        assert!(!hits(LayerKind::Line, &line, [50.0, 6.0], 5.0));
        let point = feature(GeometryKind::Point, vec![vec![[10, 10]]]);
        assert!(hits(LayerKind::Circle, &point, [13.0, 14.0], 5.0));
        assert!(!hits(LayerKind::Circle, &point, [20.0, 20.0], 5.0));
    }
}
