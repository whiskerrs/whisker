//! Turns a tile's features into vertices, one bucket per style layer.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use crate::style::color::Rgba;
use crate::style::expr::{Context, FeatureView};
use crate::style::{Layer, LayerKind};
use crate::tiles::mvt::{Feature, GeometryKind, TilePoint};
use crate::value::Value;

/// Every tile's geometry is scaled to this many units across.
pub(crate) const EXTENT: f32 = 4096.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, Default)]
pub(crate) struct FillVertex {
    pub pos: [f32; 2],
    /// Premultiplied color for layers whose color reads the feature; unused otherwise.
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, Default)]
pub(crate) struct LineVertex {
    pub pos: [f32; 2],
    /// Offset from the center line in half widths.
    pub extrude: [f32; 2],
    /// x distance along the line in tile units, y which side (-1 or 1), z per-feature width or 0,
    /// w how far along the whole line, 0 to 1, for `line-gradient`.
    pub data: [f32; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, Default)]
pub(crate) struct CircleVertex {
    pub pos: [f32; 2],
    pub corner: [f32; 2],
    pub color: [f32; 4],
    /// x per-feature radius or 0.
    pub data: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, Default)]
pub(crate) struct ExtrusionVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

#[derive(Default, Debug)]
pub(crate) struct Mesh<V> {
    pub vertices: Vec<V>,
    pub indices: Vec<u32>,
}

impl<V> Mesh<V> {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    fn base(&self) -> u32 {
        self.vertices.len() as u32
    }
}

#[derive(Debug)]
pub(crate) enum Geometry {
    Fill(Mesh<FillVertex>),
    Line(Mesh<LineVertex>),
    Circle(Mesh<CircleVertex>),
    Extrusion(Mesh<ExtrusionVertex>),
}

impl Geometry {
    pub fn is_empty(&self) -> bool {
        match self {
            Geometry::Fill(m) => m.is_empty(),
            Geometry::Line(m) => m.is_empty(),
            Geometry::Circle(m) => m.is_empty(),
            Geometry::Extrusion(m) => m.is_empty(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Bucket {
    pub layer: usize,
    pub geometry: Geometry,
}

/// A decoded feature seen through the style's expressions.
pub(crate) struct TileFeature<'a> {
    pub feature: &'a Feature,
}

impl FeatureView for TileFeature<'_> {
    fn property(&self, key: &str) -> Option<Value> {
        self.feature.property(key).cloned()
    }

    fn kind(&self) -> GeometryKind {
        self.feature.kind
    }

    fn id(&self) -> Option<Value> {
        self.feature.id.map(|id| Value::Number(id as f64))
    }

    fn properties(&self) -> Value {
        let map = self
            .feature
            .properties
            .iter()
            .map(|(k, v)| {
                let json = match v {
                    Value::Number(n) => serde_json::json!(n),
                    Value::Bool(b) => serde_json::json!(b),
                    other => serde_json::json!(other.to_text()),
                };
                (k.to_string(), json)
            })
            .collect();
        Value::Object(Arc::new(map))
    }
}

pub(crate) fn passes(layer: &Layer, feature: &dyn FeatureView, zoom: f64) -> bool {
    layer
        .filter
        .as_ref()
        .is_none_or(|filter| filter.bool(&Context::feature(zoom, feature)))
}

/// Builds the bucket for one style layer from the features of its source layer.
pub(crate) fn build(
    index: usize,
    layer: &Layer,
    features: &[Feature],
    extent: u32,
    zoom: f64,
) -> Option<Bucket> {
    let scale = EXTENT / extent as f32;
    let cell = EXTENT / crate::geo::globe_cells(zoom as u8) as f32;
    let geometry = match layer.kind {
        LayerKind::Fill => {
            let mut mesh = Mesh::default();
            let color = layer.get("fill-color");
            let opacity = layer.get("fill-opacity");
            let dynamic = color.feature_dependent || opacity.feature_dependent;
            for feature in features {
                if feature.kind != GeometryKind::Polygon {
                    continue;
                }
                let view = TileFeature { feature };
                if !passes(layer, &view, zoom) {
                    continue;
                }
                let rgba = if dynamic {
                    let ctx = Context::feature(zoom, &view);
                    color
                        .color(&ctx, Rgba::BLACK)
                        .premultiplied(opacity.number(&ctx, 1.0) as f32)
                } else {
                    [0.0; 4]
                };
                fill(&mut mesh, feature, scale, rgba, cell);
            }
            Geometry::Fill(mesh)
        }
        LayerKind::Line => {
            let mut mesh = Mesh::default();
            let color = layer.get("line-color");
            let opacity = layer.get("line-opacity");
            let width = layer.get("line-width");
            let cap = layer.get("line-cap").string(&Context::zoom(zoom));
            let join = layer.get("line-join").string(&Context::zoom(zoom));
            let miter_limit = layer.number("line-miter-limit", zoom);
            let dynamic_color = color.feature_dependent || opacity.feature_dependent;
            for feature in features {
                if feature.kind == GeometryKind::Point {
                    continue;
                }
                let view = TileFeature { feature };
                if !passes(layer, &view, zoom) {
                    continue;
                }
                let ctx = Context::feature(zoom, &view);
                let rgba = if dynamic_color {
                    color
                        .color(&ctx, Rgba::BLACK)
                        .premultiplied(opacity.number(&ctx, 1.0) as f32)
                } else {
                    [0.0; 4]
                };
                let line_width = if width.feature_dependent {
                    width.number(&ctx, 1.0) as f32
                } else {
                    0.0
                };
                let style = LineStyle {
                    cap: match cap.as_deref() {
                        Some("round") => Cap::Round,
                        Some("square") => Cap::Square,
                        _ => Cap::Butt,
                    },
                    join: match join.as_deref() {
                        Some("round") => Join::Round,
                        Some("bevel") => Join::Bevel,
                        _ => Join::Miter(miter_limit.max(1.0) as f32),
                    },
                    color: rgba,
                    width: line_width,
                    progress: [0.0, 0.0],
                };
                for (k, part) in feature.geometry.iter().enumerate() {
                    let points: Vec<[f32; 2]> = part
                        .iter()
                        .map(|p| [p[0] as f32 * scale, p[1] as f32 * scale])
                        .collect();
                    let closed = feature.kind == GeometryKind::Polygon;
                    let style = LineStyle {
                        progress: feature.progress.get(k).copied().unwrap_or([0.0, 0.0]),
                        ..style
                    };
                    line(
                        &mut mesh,
                        &split_at_cells(&points, closed, cell),
                        closed,
                        &style,
                    );
                }
            }
            Geometry::Line(mesh)
        }
        LayerKind::Circle => {
            let mut mesh = Mesh::default();
            let color = layer.get("circle-color");
            let opacity = layer.get("circle-opacity");
            let radius = layer.get("circle-radius");
            for feature in features {
                if feature.kind != GeometryKind::Point {
                    continue;
                }
                let view = TileFeature { feature };
                if !passes(layer, &view, zoom) {
                    continue;
                }
                let ctx = Context::feature(zoom, &view);
                let rgba = if color.feature_dependent || opacity.feature_dependent {
                    color
                        .color(&ctx, Rgba::BLACK)
                        .premultiplied(opacity.number(&ctx, 1.0) as f32)
                } else {
                    [0.0; 4]
                };
                let r = if radius.feature_dependent {
                    radius.number(&ctx, 5.0) as f32
                } else {
                    0.0
                };
                for part in &feature.geometry {
                    for p in part {
                        circle(
                            &mut mesh,
                            [p[0] as f32 * scale, p[1] as f32 * scale],
                            rgba,
                            r,
                        );
                    }
                }
            }
            Geometry::Circle(mesh)
        }
        LayerKind::FillExtrusion => {
            let mut mesh = Mesh::default();
            let color = layer.get("fill-extrusion-color");
            let height = layer.get("fill-extrusion-height");
            let base = layer.get("fill-extrusion-base");
            for feature in features {
                if feature.kind != GeometryKind::Polygon {
                    continue;
                }
                let view = TileFeature { feature };
                if !passes(layer, &view, zoom) {
                    continue;
                }
                let ctx = Context::feature(zoom, &view);
                let rgba = color.color(&ctx, Rgba::BLACK).0;
                let top = height.number(&ctx, 0.0).max(0.0) as f32;
                let bottom = base.number(&ctx, 0.0).clamp(0.0, f64::from(top)) as f32;
                extrusion(&mut mesh, feature, scale, bottom, top, rgba);
            }
            Geometry::Extrusion(mesh)
        }
        _ => return None,
    };
    (!geometry.is_empty()).then_some(Bucket {
        layer: index,
        geometry,
    })
}

/// `cell` is the size of the grid triangles are cut along, so that they can bend with the globe.
fn fill(mesh: &mut Mesh<FillVertex>, feature: &Feature, scale: f32, color: [f32; 4], cell: f32) {
    for polygon in feature.polygons() {
        let mut coords: Vec<f64> = Vec::new();
        let mut holes = Vec::new();
        for (i, ring) in polygon.iter().enumerate() {
            if i > 0 {
                holes.push(coords.len() / 2);
            }
            let ring = trim_closing(ring);
            for p in ring {
                coords.push(f64::from(p[0]));
                coords.push(f64::from(p[1]));
            }
        }
        let Ok(triangles) = earcutr::earcut(&coords, &holes, 2) else {
            continue;
        };
        let point = |i: usize| {
            [
                coords[2 * i] as f32 * scale,
                coords[2 * i + 1] as f32 * scale,
            ]
        };
        if cell >= EXTENT {
            let base = mesh.base();
            for i in 0..coords.len() / 2 {
                mesh.vertices.push(FillVertex {
                    pos: point(i),
                    color,
                });
            }
            mesh.indices
                .extend(triangles.into_iter().map(|i| base + i as u32));
            continue;
        }
        for triangle in triangles.chunks_exact(3) {
            let corners = [point(triangle[0]), point(triangle[1]), point(triangle[2])];
            for piece in cut_into_cells(&corners, cell) {
                let base = mesh.base();
                for p in &piece {
                    mesh.vertices.push(FillVertex { pos: *p, color });
                }
                for k in 1..piece.len() as u32 - 1 {
                    mesh.indices
                        .extend_from_slice(&[base, base + k, base + k + 1]);
                }
            }
        }
    }
}

/// The parts of a convex polygon inside each grid cell it crosses.
fn cut_into_cells(polygon: &[V2], cell: f32) -> Vec<Vec<V2>> {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in polygon {
        for c in 0..2 {
            lo[c] = lo[c].min(p[c]);
            hi[c] = hi[c].max(p[c]);
        }
    }
    let first = |c: usize| (lo[c] / cell).floor() as i32;
    let last = |c: usize| ((hi[c] / cell).ceil() as i32 - 1).max(first(c));
    if first(0) == last(0) && first(1) == last(1) {
        return vec![polygon.to_vec()];
    }
    let mut pieces = Vec::new();
    for j in first(1)..=last(1) {
        for i in first(0)..=last(0) {
            let mut piece = polygon.to_vec();
            for (c, bound, keep_below) in [
                (0, i as f32 * cell, false),
                (0, (i + 1) as f32 * cell, true),
                (1, j as f32 * cell, false),
                (1, (j + 1) as f32 * cell, true),
            ] {
                piece = clip_half(&piece, c, bound, keep_below);
                if piece.len() < 3 {
                    break;
                }
            }
            if piece.len() >= 3 {
                pieces.push(piece);
            }
        }
    }
    pieces
}

/// Sutherland–Hodgman against one axis-aligned line.
fn clip_half(polygon: &[V2], axis: usize, bound: f32, keep_below: bool) -> Vec<V2> {
    let inside = |p: &V2| {
        if keep_below {
            p[axis] <= bound
        } else {
            p[axis] >= bound
        }
    };
    let mut out = Vec::with_capacity(polygon.len() + 2);
    for (k, current) in polygon.iter().enumerate() {
        let previous = &polygon[(k + polygon.len() - 1) % polygon.len()];
        if inside(current) != inside(previous) {
            let t = (bound - previous[axis]) / (current[axis] - previous[axis]);
            out.push([
                previous[0] + (current[0] - previous[0]) * t,
                previous[1] + (current[1] - previous[1]) * t,
            ]);
        }
        if inside(current) {
            out.push(*current);
        }
    }
    out
}

/// `points` with a point added wherever a segment crosses a grid line `cell` apart.
fn split_at_cells(points: &[V2], closed: bool, cell: f32) -> Vec<V2> {
    if cell >= EXTENT || points.len() < 2 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len());
    let segments = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    for k in 0..segments {
        let (a, b) = (points[k], points[(k + 1) % points.len()]);
        out.push(a);
        let mut cuts: Vec<f32> = Vec::new();
        for c in 0..2 {
            if a[c] == b[c] {
                continue;
            }
            let (from, to) = (a[c].min(b[c]) / cell, a[c].max(b[c]) / cell);
            let mut line = from.floor() + 1.0;
            while line < to {
                cuts.push((line * cell - a[c]) / (b[c] - a[c]));
                line += 1.0;
            }
        }
        cuts.sort_by(f32::total_cmp);
        for t in cuts {
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    if !closed {
        out.push(points[points.len() - 1]);
    }
    out
}

fn trim_closing(ring: &[TilePoint]) -> &[TilePoint] {
    if ring.len() > 1 && ring.first() == ring.last() {
        &ring[..ring.len() - 1]
    } else {
        ring
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Join {
    Miter(f32),
    Bevel,
    Round,
}

#[derive(Clone, Copy)]
pub(crate) struct LineStyle {
    pub cap: Cap,
    pub join: Join,
    pub color: [f32; 4],
    pub width: f32,
    /// Where these points start and end along their whole line, 0 to 1.
    pub progress: [f32; 2],
}

type V2 = [f32; 2];

fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(a: V2) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

fn norm(a: V2) -> V2 {
    let l = len(a);
    if l == 0.0 {
        [0.0, 0.0]
    } else {
        [a[0] / l, a[1] / l]
    }
}

fn perp(a: V2) -> V2 {
    [-a[1], a[0]]
}

fn dot(a: V2, b: V2) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// Lays out a polyline as quads per segment, with joins and caps filled in between.
pub(crate) fn line(mesh: &mut Mesh<LineVertex>, points: &[V2], closed: bool, style: &LineStyle) {
    let mut pts: Vec<V2> = Vec::with_capacity(points.len());
    for p in points {
        if pts.last() != Some(p) {
            pts.push(*p);
        }
    }
    if closed && pts.len() > 2 && pts.first() == pts.last() {
        pts.pop();
    }
    if pts.len() < 2 {
        return;
    }
    let n = pts.len();
    let segments = if closed { n } else { n - 1 };
    let total: f32 = (0..segments)
        .map(|i| len(sub(pts[(i + 1) % n], pts[i])))
        .sum();
    let vertex = |pos: V2, extrude: V2, distance: f32, side: f32| LineVertex {
        pos,
        extrude,
        data: [
            distance,
            side,
            style.width,
            progress(style, distance, total),
        ],
        color: style.color,
    };
    let mut distance = 0.0f32;
    let mut distances = Vec::with_capacity(segments + 1);
    for i in 0..segments {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        distances.push(distance);
        let dir = norm(sub(b, a));
        let normal = perp(dir);
        let seg_len = len(sub(b, a));
        let base = mesh.base();
        let ext = |s: f32| [normal[0] * s, normal[1] * s];
        mesh.vertices.extend([
            vertex(a, ext(1.0), distance, 1.0),
            vertex(a, ext(-1.0), distance, -1.0),
            vertex(b, ext(1.0), distance + seg_len, 1.0),
            vertex(b, ext(-1.0), distance + seg_len, -1.0),
        ]);
        mesh.indices
            .extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        distance += seg_len;
    }
    distances.push(distance);

    let joins: Box<dyn Iterator<Item = usize>> = if closed {
        Box::new(0..n)
    } else {
        Box::new(1..n - 1)
    };
    for i in joins {
        let prev = pts[(i + n - 1) % n];
        let p = pts[i];
        let next = pts[(i + 1) % n];
        let d0 = norm(sub(p, prev));
        let d1 = norm(sub(next, p));
        let n0 = perp(d0);
        let n1 = perp(d1);
        let turn = d0[0] * d1[1] - d0[1] * d1[0];
        if turn.abs() < 1e-6 && dot(d0, d1) > 0.0 {
            continue;
        }
        // The outer side of the turn is opposite the direction the line bends toward.
        let outer = if turn > 0.0 { -1.0 } else { 1.0 };
        let e0 = [n0[0] * outer, n0[1] * outer];
        let e1 = [n1[0] * outer, n1[1] * outer];
        let at = distances[i % distances.len()];
        let center = mesh.base();
        mesh.vertices.push(vertex(p, [0.0, 0.0], at, 0.0));
        match style.join {
            Join::Round => {
                let a0 = e0[1].atan2(e0[0]);
                let mut a1 = e1[1].atan2(e1[0]);
                let mut delta = a1 - a0;
                if delta > std::f32::consts::PI {
                    a1 -= std::f32::consts::TAU;
                } else if delta < -std::f32::consts::PI {
                    a1 += std::f32::consts::TAU;
                }
                delta = a1 - a0;
                let steps = ((delta.abs() / 0.35).ceil() as u32).clamp(1, 16);
                let first = mesh.base();
                for s in 0..=steps {
                    let a = a0 + delta * s as f32 / steps as f32;
                    mesh.vertices.push(vertex(p, [a.cos(), a.sin()], at, outer));
                }
                for s in 0..steps {
                    mesh.indices.extend([center, first + s, first + s + 1]);
                }
            }
            Join::Miter(limit) => {
                let miter = norm([e0[0] + e1[0], e0[1] + e1[1]]);
                let cos_half = dot(miter, e0);
                let length = if cos_half > 1e-3 {
                    1.0 / cos_half
                } else {
                    f32::MAX
                };
                let first = mesh.base();
                mesh.vertices.push(vertex(p, e0, at, outer));
                mesh.vertices.push(vertex(p, e1, at, outer));
                if length <= limit {
                    mesh.vertices.push(vertex(
                        p,
                        [miter[0] * length, miter[1] * length],
                        at,
                        outer,
                    ));
                    mesh.indices
                        .extend([center, first, first + 2, center, first + 2, first + 1]);
                } else {
                    mesh.indices.extend([center, first, first + 1]);
                }
            }
            Join::Bevel => {
                let first = mesh.base();
                mesh.vertices.push(vertex(p, e0, at, outer));
                mesh.vertices.push(vertex(p, e1, at, outer));
                mesh.indices.extend([center, first, first + 1]);
            }
        }
    }

    if !closed {
        let first_dir = norm(sub(pts[1], pts[0]));
        let last_dir = norm(sub(pts[n - 1], pts[n - 2]));
        cap(
            mesh,
            pts[0],
            [-first_dir[0], -first_dir[1]],
            0.0,
            style,
            total,
        );
        cap(mesh, pts[n - 1], last_dir, distance, style, total);
    }
}

fn progress(style: &LineStyle, distance: f32, total: f32) -> f32 {
    let [start, end] = style.progress;
    let t = if total > 0.0 { distance / total } else { 0.0 };
    start + (end - start) * t
}

fn cap(mesh: &mut Mesh<LineVertex>, p: V2, out: V2, distance: f32, style: &LineStyle, total: f32) {
    let normal = perp(out);
    let vertex = |extrude: V2, side: f32| LineVertex {
        pos: p,
        extrude,
        data: [
            distance,
            side,
            style.width,
            progress(style, distance, total),
        ],
        color: style.color,
    };
    match style.cap {
        Cap::Butt => {}
        Cap::Square => {
            let base = mesh.base();
            mesh.vertices.extend([
                vertex(normal, 1.0),
                vertex([-normal[0], -normal[1]], -1.0),
                vertex([normal[0] + out[0], normal[1] + out[1]], 1.0),
                vertex([-normal[0] + out[0], -normal[1] + out[1]], -1.0),
            ]);
            mesh.indices
                .extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
        Cap::Round => {
            let center = mesh.base();
            mesh.vertices.push(vertex([0.0, 0.0], 0.0));
            let a0 = normal[1].atan2(normal[0]);
            let steps = 8;
            let first = mesh.base();
            for s in 0..=steps {
                // Sweeps from the left normal through the outward direction to the right normal.
                let a = a0 - std::f32::consts::PI * s as f32 / steps as f32;
                let dir = [a.cos(), a.sin()];
                let side = if dot(dir, normal) >= 0.0 { 1.0 } else { -1.0 };
                mesh.vertices.push(vertex(dir, side));
            }
            for s in 0..steps {
                mesh.indices.extend([center, first + s, first + s + 1]);
            }
        }
    }
}

fn circle(mesh: &mut Mesh<CircleVertex>, p: V2, color: [f32; 4], radius: f32) {
    let base = mesh.base();
    for corner in [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]] {
        mesh.vertices.push(CircleVertex {
            pos: p,
            corner,
            color,
            data: [radius, 0.0, 0.0, 0.0],
        });
    }
    mesh.indices
        .extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
}

fn extrusion(
    mesh: &mut Mesh<ExtrusionVertex>,
    feature: &Feature,
    scale: f32,
    bottom: f32,
    top: f32,
    color: [f32; 4],
) {
    for polygon in feature.polygons() {
        let mut coords: Vec<f64> = Vec::new();
        let mut holes = Vec::new();
        for (i, ring) in polygon.iter().enumerate() {
            if i > 0 {
                holes.push(coords.len() / 2);
            }
            let ring = trim_closing(ring);
            for p in ring {
                coords.push(f64::from(p[0]));
                coords.push(f64::from(p[1]));
            }
            // Walls, one quad per edge, facing outward.
            for k in 0..ring.len() {
                let a = ring[k];
                let b = ring[(k + 1) % ring.len()];
                let a = [a[0] as f32 * scale, a[1] as f32 * scale];
                let b = [b[0] as f32 * scale, b[1] as f32 * scale];
                // Skip edges along the tile's border, which neighbouring tiles share.
                let on_border =
                    |c: usize| (a[c] <= 0.0 && b[c] <= 0.0) || (a[c] >= EXTENT && b[c] >= EXTENT);
                if on_border(0) || on_border(1) {
                    continue;
                }
                let d = norm(sub(b, a));
                // Outer rings wind clockwise with y down, so the outward normal is to the left.
                let n = [d[1], -d[0], 0.0];
                let base = mesh.base();
                // A normal's z marks the wall's top edge (0.25) for the shader's vertical gradient.
                for (p, z, edge) in [
                    (a, bottom, 0.0),
                    (b, bottom, 0.0),
                    (a, top, 0.25),
                    (b, top, 0.25),
                ] {
                    mesh.vertices.push(ExtrusionVertex {
                        pos: [p[0], p[1], z],
                        normal: [n[0], n[1], edge],
                        color,
                    });
                }
                mesh.indices
                    .extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
            }
        }
        let Ok(triangles) = earcutr::earcut(&coords, &holes, 2) else {
            continue;
        };
        let base = mesh.base();
        for pair in coords.chunks_exact(2) {
            mesh.vertices.push(ExtrusionVertex {
                pos: [pair[0] as f32 * scale, pair[1] as f32 * scale, top],
                normal: [0.0, 0.0, 1.0],
                color,
            });
        }
        // Roofs wind clockwise on screen, like the walls seen from outside.
        for t in triangles.chunks_exact(3) {
            let p = |i: usize| (coords[2 * t[i]], coords[2 * t[i] + 1]);
            let ((x0, y0), (x1, y1), (x2, y2)) = (p(0), p(1), p(2));
            let clockwise = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0) > 0.0;
            let (j, k) = if clockwise {
                (t[1], t[2])
            } else {
                (t[2], t[1])
            };
            mesh.indices
                .extend([base + t[0] as u32, base + j as u32, base + k as u32]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(join: Join, cap: Cap) -> LineStyle {
        LineStyle {
            cap,
            join,
            color: [0.0; 4],
            width: 0.0,
            progress: [0.0, 1.0],
        }
    }

    #[test]
    fn cells_cut_triangles_and_lines_without_losing_area() {
        let triangle = [[0.0, 0.0], [300.0, 0.0], [0.0, 300.0]];
        let area = |p: &[V2]| {
            (0..p.len())
                .map(|i| {
                    let (a, b) = (p[i], p[(i + 1) % p.len()]);
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum::<f32>()
                / 2.0
        };
        let pieces = cut_into_cells(&triangle, 128.0);
        let total: f32 = pieces.iter().map(|p| area(p)).sum();
        assert!((total - area(&triangle)).abs() < 1e-2, "{total}");
        assert!(pieces.len() >= 6);
        let split = split_at_cells(&[[10.0, 10.0], [300.0, 10.0]], false, 128.0);
        assert_eq!(
            split,
            vec![[10.0, 10.0], [128.0, 10.0], [256.0, 10.0], [300.0, 10.0]]
        );
    }

    #[test]
    fn straight_lines_are_one_quad() {
        let mut mesh = Mesh::default();
        line(
            &mut mesh,
            &[[0.0, 0.0], [10.0, 0.0]],
            false,
            &style(Join::Miter(2.0), Cap::Butt),
        );
        assert_eq!(mesh.indices.len(), 6);
        assert_eq!(mesh.vertices[3].data[0], 10.0);
    }

    #[test]
    fn progress_runs_from_the_parts_start_to_its_end() {
        let mut mesh = Mesh::default();
        let style = LineStyle {
            progress: [0.5, 1.0],
            ..style(Join::Bevel, Cap::Butt)
        };
        line(
            &mut mesh,
            &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
            false,
            &style,
        );
        let at = |distance: f32| {
            mesh.vertices
                .iter()
                .find(|v| v.data[0] == distance)
                .map(|v| v.data[3])
        };
        assert_eq!(at(0.0), Some(0.5));
        assert_eq!(at(10.0), Some(0.75));
        assert_eq!(at(20.0), Some(1.0));
    }

    #[test]
    fn corners_get_joins() {
        let points = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let mut miter = Mesh::default();
        line(
            &mut miter,
            &points,
            false,
            &style(Join::Miter(2.0), Cap::Butt),
        );
        assert_eq!(miter.indices.len(), 12 + 6);
        let tip = miter
            .vertices
            .iter()
            .map(|v| len(v.extrude))
            .fold(0.0, f32::max);
        assert!((tip - std::f32::consts::SQRT_2).abs() < 1e-4, "{tip}");
        let mut round = Mesh::default();
        line(&mut round, &points, false, &style(Join::Round, Cap::Round));
        assert!(round.indices.len() > miter.indices.len());
    }

    #[test]
    fn duplicate_points_are_skipped() {
        let mut mesh = Mesh::default();
        line(
            &mut mesh,
            &[[0.0, 0.0], [0.0, 0.0]],
            false,
            &style(Join::Bevel, Cap::Butt),
        );
        assert!(mesh.is_empty());
    }
}
