//! Symbol features of one tile: text, icons, and the anchors they may be placed at.

use std::cell::{OnceCell, RefCell};
use std::sync::Arc;

use crate::style::Layer;
use crate::style::color::Rgba;
use crate::style::expr::Context;
use crate::text::{Anchor, Layout, Shaping};
use crate::tiles::bucket::{EXTENT, TileFeature, passes};
use crate::tiles::mvt::{Feature, GeometryKind};

#[derive(Clone, Copy, Debug)]
pub(crate) struct LineAnchor {
    /// Tile units scaled to `EXTENT`.
    pub point: [f32; 2],
    /// The line segment the anchor lies on, starting at `line[segment]`.
    pub segment: usize,
}

/// Layout values evaluated for one feature at the tile's zoom.
#[derive(Clone, Copy)]
pub(crate) struct TextLayout {
    pub max_width: f32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub justify: f32,
    pub anchor: Anchor,
    pub offset: [f32; 2],
}

pub(crate) struct SymbolFeature {
    pub layer: usize,
    pub source_layer: usize,
    pub feature: usize,
    pub along_line: bool,
    /// Point anchors; anchors along `line` come from `line_anchors`.
    pub anchors: Vec<LineAnchor>,
    pub line: Option<Arc<Vec<[f32; 2]>>>,
    /// `symbol-spacing` in pixels.
    pub spacing: f32,
    /// Anchors along `line` for the overscale they were computed at.
    pub line_cache: RefCell<Option<(u32, Vec<LineAnchor>)>>,
    pub text: Option<String>,
    pub font: Arc<str>,
    pub icon: Option<String>,
    pub sort_key: f64,
    /// Values that vary by feature, evaluated at the tile's zoom; otherwise taken from the layer.
    pub text_size: Option<f32>,
    pub icon_size: Option<f32>,
    pub text_color: Option<Rgba>,
    pub layout: TextLayout,
    pub shaping: OnceCell<Arc<Shaping>>,
}

/// The tile's symbol features for one style layer, in the order they should be placed.
pub(crate) fn build(
    index: usize,
    layer: &Layer,
    source_layer: usize,
    features: &[Feature],
    extent: u32,
    zoom: f64,
) -> Vec<SymbolFeature> {
    let scale = EXTENT / extent as f32;
    let mut out = Vec::new();
    let has_text = layer.has("text-field");
    let has_icon = layer.has("icon-image");
    for (feature_index, feature) in features.iter().enumerate() {
        let view = TileFeature { feature };
        if !passes(layer, &view, zoom) {
            continue;
        }
        let ctx = Context::feature(zoom, &view);
        let text = has_text
            .then(|| layer.get("text-field").string(&ctx))
            .flatten()
            .map(
                |text| match layer.get("text-transform").string(&ctx).as_deref() {
                    Some("uppercase") => text.to_uppercase(),
                    Some("lowercase") => text.to_lowercase(),
                    _ => text,
                },
            )
            .filter(|text| !text.trim().is_empty());
        let icon = has_icon
            .then(|| layer.get("icon-image").string(&ctx))
            .flatten()
            .filter(|icon| !icon.is_empty());
        if text.is_none() && icon.is_none() {
            continue;
        }
        let font: Arc<str> = layer.get("text-font").strings(&ctx).join(",").into();
        let along_line = layer.get("symbol-placement").string(&ctx).as_deref() != Some("point")
            && feature.kind != GeometryKind::Point;
        let text_size_property = layer.get("text-size");
        let text_size = text_size_property
            .feature_dependent
            .then(|| text_size_property.number(&ctx, 16.0) as f32);
        let icon_size_property = layer.get("icon-size");
        let icon_size = icon_size_property
            .feature_dependent
            .then(|| icon_size_property.number(&ctx, 1.0) as f32);
        let color = layer.get("text-color");
        let text_color = color
            .feature_dependent
            .then(|| color.color(&ctx, Rgba::BLACK));
        let offset = layer.get("text-offset").numbers(&ctx);
        let justify = match layer.get("text-justify").string(&ctx).as_deref() {
            Some("left") => 0.0,
            Some("right") => 1.0,
            _ => 0.5,
        };
        let layout = TextLayout {
            max_width: layer.get("text-max-width").number(&ctx, 10.0) as f32,
            line_height: layer.get("text-line-height").number(&ctx, 1.2) as f32,
            letter_spacing: layer.get("text-letter-spacing").number(&ctx, 0.0) as f32,
            justify,
            anchor: Anchor::parse(&layer.get("text-anchor").string(&ctx).unwrap_or_default()),
            offset: [
                offset.first().copied().unwrap_or(0.0) as f32,
                offset.get(1).copied().unwrap_or(0.0) as f32,
            ],
        };
        let sort_key = layer
            .get("symbol-sort-key")
            .number(&ctx, feature_index as f64);
        let scaled = |p: &[i32; 2]| [p[0] as f32 * scale, p[1] as f32 * scale];
        let inside = |p: [f32; 2]| (0.0..EXTENT).contains(&p[0]) && (0.0..EXTENT).contains(&p[1]);
        let spacing = layer.get("symbol-spacing").number(&ctx, 250.0) as f32;
        let mut push = |anchors: Vec<LineAnchor>, line: Option<Arc<Vec<[f32; 2]>>>| {
            if anchors.is_empty() && line.is_none() {
                return;
            }
            out.push(SymbolFeature {
                layer: index,
                source_layer,
                feature: feature_index,
                along_line: line.is_some(),
                anchors,
                line,
                spacing,
                line_cache: RefCell::new(None),
                text: text.clone(),
                font: font.clone(),
                icon: icon.clone(),
                sort_key,
                text_size,
                icon_size,
                text_color,
                layout,
                shaping: OnceCell::new(),
            });
        };
        if along_line {
            for line in &feature.geometry {
                let line: Vec<[f32; 2]> = line.iter().map(scaled).collect();
                if line.len() >= 2 {
                    push(Vec::new(), Some(Arc::new(line)));
                }
            }
        } else {
            match feature.kind {
                GeometryKind::Point => {
                    for points in &feature.geometry {
                        for p in points {
                            let p = scaled(p);
                            if inside(p) {
                                push(
                                    vec![LineAnchor {
                                        point: p,
                                        segment: 0,
                                    }],
                                    None,
                                );
                            }
                        }
                    }
                }
                GeometryKind::Polygon => {
                    for polygon in feature.polygons() {
                        if let Some(p) = polygon.first().map(|ring| centroid(ring, scale))
                            && inside(p)
                        {
                            push(
                                vec![LineAnchor {
                                    point: p,
                                    segment: 0,
                                }],
                                None,
                            );
                        }
                    }
                }
                GeometryKind::Line => {
                    for line in &feature.geometry {
                        let line: Vec<[f32; 2]> = line.iter().map(scaled).collect();
                        if let Some(anchor) = line_anchors(&line, f32::MAX, 0.0).into_iter().next()
                            && inside(anchor.point)
                        {
                            push(vec![anchor], None);
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.sort_key.total_cmp(&b.sort_key));
    out
}

impl SymbolFeature {
    /// Anchors along the line for a tile drawn `overscale` times its size, keeping labels
    /// `spacing` pixels apart and room for `label_length` pixels.
    pub fn anchors_along(&self, overscale: u32, label_length: f32) -> Vec<LineAnchor> {
        let Some(line) = &self.line else {
            return self.anchors.clone();
        };
        if let Some((cached, anchors)) = &*self.line_cache.borrow()
            && *cached == overscale
        {
            return anchors.clone();
        }
        let units = EXTENT / (512.0 * overscale as f32);
        let inside = |p: [f32; 2]| (0.0..EXTENT).contains(&p[0]) && (0.0..EXTENT).contains(&p[1]);
        let anchors: Vec<LineAnchor> =
            line_anchors(line, self.spacing * units, label_length * units)
                .into_iter()
                .filter(|a| inside(a.point))
                .collect();
        *self.line_cache.borrow_mut() = Some((overscale, anchors.clone()));
        anchors
    }
}

impl TextLayout {
    pub fn for_shaping(&self, wrap: bool) -> Layout {
        Layout {
            max_width: self.max_width,
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            justify: self.justify,
            anchor: if wrap {
                self.anchor
            } else {
                Anchor::parse("center")
            },
            offset: if wrap { self.offset } else { [0.0, 0.0] },
            wrap,
        }
    }
}

/// Anchors every `spacing` along `line`, starting half a spacing in, on lines long enough to hold
/// a label of `label_length`; a line shorter than one spacing gets one anchor at its middle.
pub(crate) fn line_anchors(line: &[[f32; 2]], spacing: f32, label_length: f32) -> Vec<LineAnchor> {
    let lengths: Vec<f32> = line
        .windows(2)
        .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt())
        .collect();
    let total: f32 = lengths.iter().sum();
    if total <= 0.0 || total < label_length {
        return Vec::new();
    }
    let mut targets = Vec::new();
    if total < spacing {
        targets.push(total / 2.0);
    } else {
        let mut d = spacing / 2.0;
        while d < total {
            if d >= label_length / 2.0 && total - d >= label_length / 2.0 {
                targets.push(d);
            }
            d += spacing;
        }
        if targets.is_empty() {
            targets.push(total / 2.0);
        }
    }
    let mut anchors = Vec::new();
    let mut walked = 0.0;
    let mut target = targets.into_iter().peekable();
    for (segment, &length) in lengths.iter().enumerate() {
        while let Some(&t) = target.peek() {
            if t > walked + length {
                break;
            }
            let k = if length > 0.0 {
                (t - walked) / length
            } else {
                0.0
            };
            let (a, b) = (line[segment], line[segment + 1]);
            anchors.push(LineAnchor {
                point: [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k],
                segment,
            });
            target.next();
        }
        walked += length;
    }
    anchors
}

fn centroid(ring: &[[i32; 2]], scale: f32) -> [f32; 2] {
    let (mut cx, mut cy, mut area) = (0.0f64, 0.0f64, 0.0f64);
    for w in ring.windows(2) {
        let (x0, y0) = (f64::from(w[0][0]), f64::from(w[0][1]));
        let (x1, y1) = (f64::from(w[1][0]), f64::from(w[1][1]));
        let cross = x0 * y1 - x1 * y0;
        area += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    if area.abs() < 1e-9 {
        let p = ring.first().copied().unwrap_or([0, 0]);
        return [p[0] as f32 * scale, p[1] as f32 * scale];
    }
    [
        (cx / (3.0 * area)) as f32 * scale,
        (cy / (3.0 * area)) as f32 * scale,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_are_spaced_along_the_line() {
        let line = [[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0]];
        let anchors = line_anchors(&line, 400.0, 0.0);
        let xs: Vec<[f32; 2]> = anchors.iter().map(|a| a.point).collect();
        assert_eq!(
            xs,
            vec![
                [200.0, 0.0],
                [600.0, 0.0],
                [1000.0, 0.0],
                [1000.0, 400.0],
                [1000.0, 800.0]
            ]
        );
        assert_eq!(anchors[3].segment, 1);
    }

    #[test]
    fn short_lines_get_one_middle_anchor() {
        let anchors = line_anchors(&[[0.0, 0.0], [100.0, 0.0]], 400.0, 0.0);
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].point, [50.0, 0.0]);
        assert!(line_anchors(&[[0.0, 0.0], [100.0, 0.0]], 400.0, 200.0).is_empty());
    }
}
