//! The byte layout tile jobs and their results take between the page and its Workers.

use std::cell::{OnceCell, RefCell};
use std::sync::Arc;

use bytemuck::Pod;

use crate::geo::TileId;
use crate::style::color::Rgba;
use crate::style::{GeoJsonOptions, SourceKind};
use crate::symbols::{LineAnchor, SymbolFeature, TextLayout};
use crate::text::Anchor;
use crate::tiles::bucket::{Bucket, Geometry, Mesh};
use crate::tiles::mvt::{Feature, GeometryKind, Layer};
use crate::tiles::{Dem, Features, Image, TileContent};
use crate::value::Value;

/// One tile to make from its source's bytes.
pub(crate) struct TileJob {
    pub style: u32,
    pub id: TileId,
    pub kind: SourceKind,
    pub indices: Vec<usize>,
    pub max_zoom: u8,
    pub bytes: Vec<u8>,
}

pub(crate) fn encode_job(job: &TileJob) -> Vec<u8> {
    let mut w = Writer(Vec::with_capacity(job.bytes.len() + 64));
    w.u32(job.style);
    w.tile_id(job.id);
    match job.kind {
        SourceKind::Raster { .. } => w.u8(1),
        SourceKind::RasterDem { terrarium, .. } => {
            w.u8(2);
            w.bool(terrarium);
        }
        _ => w.u8(0),
    }
    w.u32(job.indices.len() as u32);
    for &index in &job.indices {
        w.u32(index as u32);
    }
    w.u8(job.max_zoom);
    w.bytes(&job.bytes);
    w.0
}

pub(crate) fn decode_job(data: &[u8]) -> Option<TileJob> {
    let mut r = Reader { data, at: 0 };
    let style = r.u32()?;
    let id = r.tile_id()?;
    let kind = match r.u8()? {
        0 => SourceKind::Vector,
        1 => SourceKind::Raster { tile_size: 0 },
        2 => SourceKind::RasterDem {
            tile_size: 0,
            terrarium: r.bool()?,
        },
        _ => return None,
    };
    let count = r.u32()? as usize;
    let indices = (0..count)
        .map(|_| r.u32().map(|i| i as usize))
        .collect::<Option<_>>()?;
    let max_zoom = r.u8()?;
    let bytes = r.bytes()?.to_vec();
    Some(TileJob {
        style,
        id,
        kind,
        indices,
        max_zoom,
        bytes,
    })
}

/// A GeoJSON source to index: its options, then the GeoJSON text.
pub(crate) fn encode_geojson(options: &GeoJsonOptions, text: &[u8]) -> Vec<u8> {
    let mut w = Writer(Vec::with_capacity(text.len() + 64));
    w.bool(options.cluster);
    w.f64(options.cluster_radius);
    w.u8(options.cluster_max_zoom);
    w.u32(options.cluster_min_points as u32);
    w.u8(options.max_zoom);
    w.f64(options.tolerance);
    w.f64(options.buffer);
    w.bytes(text);
    w.0
}

pub(crate) fn decode_geojson(data: &[u8]) -> Option<(GeoJsonOptions, &[u8])> {
    let mut r = Reader { data, at: 0 };
    let options = GeoJsonOptions {
        cluster: r.bool()?,
        cluster_radius: r.f64()?,
        cluster_max_zoom: r.u8()?,
        cluster_min_points: r.u32()? as usize,
        max_zoom: r.u8()?,
        tolerance: r.f64()?,
        buffer: r.f64()?,
    };
    Some((options, r.bytes()?))
}

/// Everything in `content`, with a vector tile's features only `with_features`; without them the
/// page decodes the tile's bytes again when a tap needs them.
pub(crate) fn encode_content(content: &TileContent, with_features: bool) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    match content {
        TileContent::Vector {
            buckets,
            symbols,
            features,
        } => {
            w.u8(0);
            w.u32(buckets.len() as u32);
            for bucket in buckets {
                w.u32(bucket.layer as u32);
                match &bucket.geometry {
                    Geometry::Fill(m) => w.mesh(0, m),
                    Geometry::Line(m) => w.mesh(1, m),
                    Geometry::Circle(m) => w.mesh(2, m),
                    Geometry::Extrusion(m) => w.mesh(3, m),
                }
            }
            w.u32(symbols.len() as u32);
            for symbol in symbols {
                w.symbol(symbol);
            }
            w.bool(with_features);
            if with_features {
                w.layers(features.layers());
            }
        }
        TileContent::Raster(image) => {
            w.u8(1);
            w.u32(image.width);
            w.u32(image.height);
            w.bytes(&image.pixels);
        }
        TileContent::Dem(dem) => {
            w.u8(2);
            w.u32(dem.size);
            w.pod(&dem.heights);
        }
    }
    w.0
}

/// The content [`encode_content`] wrote, with `features` standing in for features it left out.
pub(crate) fn decode_content(data: &[u8], features: Features) -> Option<TileContent> {
    let mut r = Reader { data, at: 0 };
    Some(match r.u8()? {
        0 => {
            let count = r.u32()? as usize;
            let mut buckets = Vec::with_capacity(count);
            for _ in 0..count {
                let layer = r.u32()? as usize;
                let geometry = match r.u8()? {
                    0 => Geometry::Fill(r.mesh()?),
                    1 => Geometry::Line(r.mesh()?),
                    2 => Geometry::Circle(r.mesh()?),
                    3 => Geometry::Extrusion(r.mesh()?),
                    _ => return None,
                };
                buckets.push(Bucket { layer, geometry });
            }
            let count = r.u32()? as usize;
            let symbols = (0..count).map(|_| r.symbol()).collect::<Option<_>>()?;
            let features = match r.bool()? {
                true => Features::decoded(Arc::new(r.layers()?)),
                false => features,
            };
            TileContent::Vector {
                buckets,
                symbols,
                features,
            }
        }
        1 => TileContent::Raster(Image {
            width: r.u32()?,
            height: r.u32()?,
            pixels: r.bytes()?.to_vec(),
        }),
        2 => TileContent::Dem(Dem {
            size: r.u32()?,
            heights: r.pod()?,
        }),
        _ => return None,
    })
}

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }

    fn pod<T: Pod>(&mut self, v: &[T]) {
        self.bytes(bytemuck::cast_slice(v));
    }

    fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }

    fn opt_f32(&mut self, v: Option<f32>) {
        self.bool(v.is_some());
        self.f32(v.unwrap_or(0.0));
    }

    fn opt_str(&mut self, v: Option<&str>) {
        self.bool(v.is_some());
        self.str(v.unwrap_or(""));
    }

    fn tile_id(&mut self, id: TileId) {
        self.u8(id.z);
        self.u32(id.x);
        self.u32(id.y);
    }

    fn mesh<V: Pod>(&mut self, tag: u8, mesh: &Mesh<V>) {
        self.u8(tag);
        self.pod(&mesh.vertices);
        self.pod(&mesh.indices);
    }

    fn points(&mut self, points: &[[f32; 2]]) {
        self.pod(points);
    }

    fn layers(&mut self, layers: &[Layer]) {
        self.u32(layers.len() as u32);
        for layer in layers {
            self.str(&layer.name);
            self.u32(layer.extent);
            self.u32(layer.features.len() as u32);
            for feature in &layer.features {
                self.bool(feature.id.is_some());
                self.0
                    .extend_from_slice(&feature.id.unwrap_or(0).to_le_bytes());
                self.u8(match feature.kind {
                    GeometryKind::Point => 0,
                    GeometryKind::Line => 1,
                    GeometryKind::Polygon => 2,
                });
                let mut properties: Vec<_> = feature.properties.iter().collect();
                properties.sort_by(|a, b| a.0.cmp(b.0));
                self.u32(properties.len() as u32);
                for (key, value) in properties {
                    self.str(key);
                    self.value(value);
                }
                self.u32(feature.geometry.len() as u32);
                for part in &feature.geometry {
                    self.pod(part);
                }
                self.pod(&feature.progress);
            }
        }
    }

    fn value(&mut self, value: &Value) {
        match value {
            Value::Null => self.u8(0),
            Value::Bool(b) => {
                self.u8(1);
                self.bool(*b);
            }
            Value::Number(n) => {
                self.u8(2);
                self.f64(*n);
            }
            Value::String(s) => {
                self.u8(3);
                self.str(s);
            }
            Value::Color(c) => {
                self.u8(4);
                self.pod(&c.0);
            }
            Value::Array(items) => {
                self.u8(5);
                self.u32(items.len() as u32);
                for item in items.iter() {
                    self.value(item);
                }
            }
            Value::Object(map) => {
                self.u8(6);
                self.str(&serde_json::Value::Object((**map).clone()).to_string());
            }
        }
    }

    fn symbol(&mut self, s: &SymbolFeature) {
        self.u32(s.layer as u32);
        self.u32(s.source_layer as u32);
        self.u32(s.feature as u32);
        self.bool(s.along_line);
        self.u32(s.anchors.len() as u32);
        for anchor in &s.anchors {
            self.f32(anchor.point[0]);
            self.f32(anchor.point[1]);
            self.u32(anchor.segment as u32);
        }
        self.bool(s.line.is_some());
        self.points(s.line.as_deref().map_or(&[], Vec::as_slice));
        self.f32(s.spacing);
        self.opt_str(s.text.as_deref());
        self.str(&s.font);
        self.opt_str(s.icon.as_deref());
        self.f64(s.sort_key);
        self.opt_f32(s.text_size);
        self.opt_f32(s.icon_size);
        self.bool(s.text_color.is_some());
        self.pod(&s.text_color.map_or([0.0; 4], |c| c.0));
        let l = &s.layout;
        for v in [
            l.max_width,
            l.line_height,
            l.letter_spacing,
            l.justify,
            l.anchor.horizontal,
            l.anchor.vertical,
            l.offset[0],
            l.offset[1],
        ] {
            self.f32(v);
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let slice = self.data.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn bool(&mut self) -> Option<bool> {
        Some(self.u8()? != 0)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let len = self.u32()? as usize;
        self.take(len)
    }

    fn pod<T: Pod>(&mut self) -> Option<Vec<T>> {
        let bytes = self.bytes()?;
        if bytes.len() % std::mem::size_of::<T>() != 0 {
            return None;
        }
        Some(bytemuck::pod_collect_to_vec(bytes))
    }

    fn str(&mut self) -> Option<&'a str> {
        std::str::from_utf8(self.bytes()?).ok()
    }

    fn opt_f32(&mut self) -> Option<Option<f32>> {
        let some = self.bool()?;
        let v = self.f32()?;
        Some(some.then_some(v))
    }

    fn opt_string(&mut self) -> Option<Option<String>> {
        let some = self.bool()?;
        let v = self.str()?;
        Some(some.then(|| v.to_string()))
    }

    fn tile_id(&mut self) -> Option<TileId> {
        Some(TileId {
            z: self.u8()?,
            x: self.u32()?,
            y: self.u32()?,
        })
    }

    fn mesh<V: Pod>(&mut self) -> Option<Mesh<V>> {
        Some(Mesh {
            vertices: self.pod()?,
            indices: self.pod()?,
        })
    }

    fn layers(&mut self) -> Option<Vec<Layer>> {
        let count = self.u32()? as usize;
        let mut layers = Vec::with_capacity(count);
        for _ in 0..count {
            let name = self.str()?.to_string();
            let extent = self.u32()?;
            let count = self.u32()? as usize;
            let mut features = Vec::with_capacity(count);
            for _ in 0..count {
                let has_id = self.bool()?;
                let id = u64::from_le_bytes(self.take(8)?.try_into().ok()?);
                let kind = match self.u8()? {
                    0 => GeometryKind::Point,
                    1 => GeometryKind::Line,
                    2 => GeometryKind::Polygon,
                    _ => return None,
                };
                let count = self.u32()? as usize;
                let mut properties = std::collections::HashMap::with_capacity(count);
                for _ in 0..count {
                    let key: Arc<str> = self.str()?.into();
                    properties.insert(key, self.value()?);
                }
                let count = self.u32()? as usize;
                let geometry = (0..count).map(|_| self.pod()).collect::<Option<_>>()?;
                features.push(Feature {
                    id: has_id.then_some(id),
                    kind,
                    properties,
                    geometry,
                    progress: self.pod()?,
                });
            }
            layers.push(Layer {
                name,
                extent,
                features,
            });
        }
        Some(layers)
    }

    fn value(&mut self) -> Option<Value> {
        Some(match self.u8()? {
            0 => Value::Null,
            1 => Value::Bool(self.bool()?),
            2 => Value::Number(self.f64()?),
            3 => Value::String(self.str()?.into()),
            4 => Value::Color(Rgba(self.pod::<f32>()?.try_into().ok()?)),
            5 => {
                let count = self.u32()? as usize;
                Value::Array((0..count).map(|_| self.value()).collect::<Option<_>>()?)
            }
            6 => match serde_json::from_str(self.str()?).ok()? {
                serde_json::Value::Object(map) => Value::Object(Arc::new(map)),
                _ => return None,
            },
            _ => return None,
        })
    }

    fn symbol(&mut self) -> Option<SymbolFeature> {
        let layer = self.u32()? as usize;
        let source_layer = self.u32()? as usize;
        let feature = self.u32()? as usize;
        let along_line = self.bool()?;
        let count = self.u32()? as usize;
        let anchors = (0..count)
            .map(|_| {
                Some(LineAnchor {
                    point: [self.f32()?, self.f32()?],
                    segment: self.u32()? as usize,
                })
            })
            .collect::<Option<_>>()?;
        let has_line = self.bool()?;
        let line: Vec<[f32; 2]> = self.pod()?;
        let spacing = self.f32()?;
        let text = self.opt_string()?;
        let font: Arc<str> = self.str()?.into();
        let icon = self.opt_string()?;
        let sort_key = self.f64()?;
        let text_size = self.opt_f32()?;
        let icon_size = self.opt_f32()?;
        let has_color = self.bool()?;
        let color: Vec<f32> = self.pod()?;
        let mut v = [0.0f32; 8];
        for slot in &mut v {
            *slot = self.f32()?;
        }
        Some(SymbolFeature {
            layer,
            source_layer,
            feature,
            along_line,
            anchors,
            line: has_line.then(|| Arc::new(line)),
            spacing,
            line_cache: RefCell::new(None),
            text,
            font,
            icon,
            sort_key,
            text_size,
            icon_size,
            text_color: has_color.then(|| Rgba(color.try_into().unwrap_or([0.0; 4]))),
            layout: TextLayout {
                max_width: v[0],
                line_height: v[1],
                letter_spacing: v[2],
                justify: v[3],
                anchor: Anchor {
                    horizontal: v[4],
                    vertical: v[5],
                },
                offset: [v[6], v[7]],
            },
            shaping: OnceCell::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Style, parse_layers};
    use crate::tiles::process;

    fn round_trip(content: &TileContent, with_features: bool) -> Vec<u8> {
        let first = encode_content(content, with_features);
        let decoded = decode_content(&first, Features::encoded(Vec::new())).unwrap();
        let second = encode_content(&decoded, with_features);
        assert_eq!(first, second);
        first
    }

    #[test]
    fn jobs_round_trip() {
        let job = TileJob {
            style: 7,
            id: TileId { z: 3, x: 5, y: 2 },
            kind: SourceKind::RasterDem {
                tile_size: 512,
                terrarium: true,
            },
            indices: vec![0, 4, 9],
            max_zoom: 14,
            bytes: vec![1, 2, 3],
        };
        let back = decode_job(&encode_job(&job)).unwrap();
        assert_eq!((back.style, back.id, back.max_zoom), (7, job.id, 14));
        assert_eq!(back.indices, job.indices);
        assert_eq!(back.bytes, job.bytes);
        assert!(matches!(
            back.kind,
            SourceKind::RasterDem {
                terrarium: true,
                ..
            }
        ));
    }

    #[test]
    fn processed_vector_tiles_round_trip() {
        let style = serde_json::json!({
            "layers": [
                { "id": "water", "type": "fill", "source": "s", "source-layer": "water",
                  "paint": { "fill-color": "#0af" } },
                { "id": "edge", "type": "line", "source": "s", "source-layer": "water",
                  "paint": { "line-width": 2 } },
                { "id": "name", "type": "symbol", "source": "s", "source-layer": "water",
                  "layout": { "text-field": ["get", "class"], "symbol-placement": "line" } },
            ]
        });
        let layers = parse_layers(&style["layers"]);
        assert_eq!(Style::from_json(&style).layers.len(), layers.len());
        let tile = crate::tiles::mvt::tests::encode_layer(
            "water",
            &[(
                3,
                crate::tiles::mvt::tests::square(0, 0, 100),
                vec![("class", "lake")],
            )],
        );
        let content = process(
            TileId { z: 10, x: 1, y: 1 },
            &SourceKind::Vector,
            &tile,
            &layers,
            &[0, 1, 2],
            14,
        )
        .unwrap();
        let TileContent::Vector {
            buckets, symbols, ..
        } = &content
        else {
            panic!("vector content");
        };
        assert_eq!(buckets.len(), 2);
        assert!(!symbols.is_empty());
        round_trip(&content, false);
    }

    #[test]
    fn rasters_and_elevations_round_trip() {
        round_trip(
            &TileContent::Raster(Image {
                width: 2,
                height: 1,
                pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
            }),
            false,
        );
        round_trip(
            &TileContent::Dem(Dem {
                size: 1,
                heights: vec![0.5; 9],
            }),
            false,
        );
    }

    #[test]
    fn geojson_tiles_round_trip_with_their_features() {
        let options = GeoJsonOptions {
            cluster: true,
            cluster_radius: 50.0,
            cluster_max_zoom: 14,
            cluster_min_points: 2,
            max_zoom: 18,
            tolerance: 0.375,
            buffer: 128.0,
        };
        let text = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","id":4,"properties":{"name":"a","n":2,"tags":["x",1],"o":{"k":true}},
             "geometry":{"type":"Point","coordinates":[139.76,35.68]}},
            {"type":"Feature","properties":{"name":"b"},
             "geometry":{"type":"LineString","coordinates":[[139.7,35.6],[139.8,35.7]]}}]}"#;
        let encoded = encode_geojson(&options, text);
        let (back, payload) = decode_geojson(&encoded).unwrap();
        assert_eq!(back, options);
        assert_eq!(payload, text);
        let index = crate::tiles::geojson::GeoJsonIndex::new(
            &serde_json::from_slice(payload).unwrap(),
            back,
        );
        let id = TileId { z: 0, x: 0, y: 0 };
        let style = serde_json::json!([
            { "id": "dots", "type": "circle", "source": "g" },
            { "id": "names", "type": "symbol", "source": "g", "layout": { "text-field": ["get", "name"] } },
        ]);
        let content = crate::tiles::vector_content(
            id,
            Arc::new(vec![index.tile(id)]),
            &parse_layers(&style),
            &[0, 1],
            18,
        );
        round_trip(&content, true);
        let decoded = decode_content(
            &encode_content(&content, true),
            Features::encoded(Vec::new()),
        )
        .unwrap();
        let TileContent::Vector { features, .. } = decoded else {
            panic!("vector content");
        };
        let layer = &features.layers()[0];
        assert_eq!(layer.features.len(), 2);
        let point = layer
            .features
            .iter()
            .find(|f| f.kind == GeometryKind::Point)
            .unwrap();
        assert_eq!(point.id, Some(4));
        assert_eq!(point.property("name").and_then(Value::as_str), Some("a"));
        assert!(matches!(point.property("tags"), Some(Value::Array(items)) if items.len() == 2));
        assert!(matches!(point.property("o"), Some(Value::Object(map)) if map["k"] == true));
    }
}
