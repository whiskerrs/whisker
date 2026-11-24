//! Mapbox Vector Tile decoding.

use std::collections::HashMap;
use std::sync::Arc;

use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeometryKind {
    Point,
    Line,
    Polygon,
}

/// A point in tile units, usually 0 to the layer's extent across.
pub(crate) type TilePoint = [i32; 2];

#[derive(Clone, Debug)]
pub(crate) struct Feature {
    pub id: Option<u64>,
    pub kind: GeometryKind,
    pub properties: HashMap<Arc<str>, Value>,
    /// Points, lines, or polygon rings, with each polygon's outer ring before its holes.
    pub geometry: Vec<Vec<TilePoint>>,
    /// Where each line part starts and ends along its whole line, 0 to 1, when that is known.
    pub progress: Vec<[f32; 2]>,
}

impl Feature {
    pub fn property(&self, key: &str) -> Option<&Value> {
        self.properties.get(key)
    }

    /// The polygons of a polygon feature: each an outer ring followed by its holes.
    pub fn polygons(&self) -> Vec<Vec<&[TilePoint]>> {
        let mut polygons: Vec<Vec<&[TilePoint]>> = Vec::new();
        for ring in &self.geometry {
            if ring.len() < 3 {
                continue;
            }
            let area = signed_area(ring);
            if area == 0.0 {
                continue;
            }
            // Outer rings wind clockwise on screen, which is a positive area with y down.
            if area > 0.0 || polygons.is_empty() {
                polygons.push(vec![ring]);
            } else if let Some(last) = polygons.last_mut() {
                last.push(ring);
            }
        }
        polygons
    }
}

pub(crate) fn signed_area(ring: &[TilePoint]) -> f64 {
    let mut sum = 0.0;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        sum += f64::from(a[0]) * f64::from(b[1]) - f64::from(b[0]) * f64::from(a[1]);
    }
    sum / 2.0
}

#[derive(Clone, Debug)]
pub(crate) struct Layer {
    pub name: String,
    pub extent: u32,
    pub features: Vec<Feature>,
}

#[derive(Debug)]
pub(crate) struct DecodeError(&'static str);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid vector tile: {}", self.0)
    }
}

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub(crate) fn done(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub(crate) fn varint(&mut self) -> Result<u64, DecodeError> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self
                .data
                .get(self.pos)
                .ok_or(DecodeError("truncated varint"))?;
            self.pos += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(DecodeError("varint too long"))
    }

    pub(crate) fn key(&mut self) -> Result<(u32, u8), DecodeError> {
        let key = self.varint()?;
        Ok(((key >> 3) as u32, (key & 7) as u8))
    }

    pub(crate) fn bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let len = self.varint()? as usize;
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.data.len())
            .ok_or(DecodeError("truncated field"))?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn fixed(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos + n;
        if end > self.data.len() {
            return Err(DecodeError("truncated fixed field"));
        }
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    pub(crate) fn skip(&mut self, wire: u8) -> Result<(), DecodeError> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => {
                self.fixed(8)?;
            }
            2 => {
                self.bytes()?;
            }
            5 => {
                self.fixed(4)?;
            }
            _ => return Err(DecodeError("unknown wire type")),
        }
        Ok(())
    }

    fn packed(&mut self) -> Result<Vec<u32>, DecodeError> {
        let mut inner = Reader::new(self.bytes()?);
        let mut out = Vec::new();
        while !inner.done() {
            out.push(inner.varint()? as u32);
        }
        Ok(out)
    }
}

/// Reads the layers of a tile, inflating it first when it is gzipped.
pub(crate) fn decode(data: &[u8]) -> Result<Vec<Layer>, DecodeError> {
    let inflated;
    let data = if data.starts_with(&[0x1f, 0x8b]) {
        inflated = crate::net::gunzip(data).map_err(|_| DecodeError("bad gzip"))?;
        &inflated[..]
    } else {
        data
    };
    let mut reader = Reader::new(data);
    let mut layers = Vec::new();
    while !reader.done() {
        let (field, wire) = reader.key()?;
        if field == 3 && wire == 2 {
            layers.push(decode_layer(reader.bytes()?)?);
        } else {
            reader.skip(wire)?;
        }
    }
    Ok(layers)
}

struct RawFeature<'a> {
    id: Option<u64>,
    tags: Vec<u32>,
    kind: u32,
    geometry: &'a [u8],
}

fn decode_layer(data: &[u8]) -> Result<Layer, DecodeError> {
    let mut reader = Reader::new(data);
    let mut name = String::new();
    let mut extent = 4096;
    let mut keys: Vec<Arc<str>> = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    let mut raw = Vec::new();
    while !reader.done() {
        let (field, wire) = reader.key()?;
        match (field, wire) {
            (1, 2) => name = String::from_utf8_lossy(reader.bytes()?).into_owned(),
            (2, 2) => raw.push(decode_raw_feature(reader.bytes()?)?),
            (3, 2) => keys.push(String::from_utf8_lossy(reader.bytes()?).as_ref().into()),
            (4, 2) => values.push(decode_value(reader.bytes()?)?),
            (5, 0) => extent = reader.varint()? as u32,
            _ => reader.skip(wire)?,
        }
    }
    let mut features = Vec::with_capacity(raw.len());
    for feature in raw {
        let kind = match feature.kind {
            1 => GeometryKind::Point,
            2 => GeometryKind::Line,
            3 => GeometryKind::Polygon,
            _ => continue,
        };
        let mut properties = HashMap::with_capacity(feature.tags.len() / 2);
        for pair in feature.tags.chunks_exact(2) {
            if let (Some(key), Some(value)) =
                (keys.get(pair[0] as usize), values.get(pair[1] as usize))
            {
                properties.insert(key.clone(), value.clone());
            }
        }
        let geometry = decode_geometry(feature.geometry, kind)?;
        features.push(Feature {
            id: feature.id,
            kind,
            properties,
            geometry,
            progress: Vec::new(),
        });
    }
    Ok(Layer {
        name,
        extent,
        features,
    })
}

fn decode_raw_feature(data: &[u8]) -> Result<RawFeature<'_>, DecodeError> {
    let mut reader = Reader::new(data);
    let mut feature = RawFeature {
        id: None,
        tags: Vec::new(),
        kind: 0,
        geometry: &[],
    };
    while !reader.done() {
        let (field, wire) = reader.key()?;
        match (field, wire) {
            (1, 0) => feature.id = Some(reader.varint()?),
            (2, 2) => feature.tags = reader.packed()?,
            (3, 0) => feature.kind = reader.varint()? as u32,
            (4, 2) => feature.geometry = reader.bytes()?,
            _ => reader.skip(wire)?,
        }
    }
    Ok(feature)
}

fn decode_value(data: &[u8]) -> Result<Value, DecodeError> {
    let mut reader = Reader::new(data);
    let mut value = Value::Null;
    while !reader.done() {
        let (field, wire) = reader.key()?;
        value = match (field, wire) {
            (1, 2) => Value::String(String::from_utf8_lossy(reader.bytes()?).as_ref().into()),
            (2, 5) => {
                let bytes = reader.fixed(4)?;
                Value::Number(f64::from(f32::from_le_bytes(bytes.try_into().unwrap())))
            }
            (3, 1) => {
                let bytes = reader.fixed(8)?;
                Value::Number(f64::from_le_bytes(bytes.try_into().unwrap()))
            }
            (4, 0) => Value::Number(reader.varint()? as i64 as f64),
            (5, 0) => Value::Number(reader.varint()? as f64),
            (6, 0) => {
                let v = reader.varint()?;
                Value::Number(((v >> 1) as i64 ^ -((v & 1) as i64)) as f64)
            }
            (7, 0) => Value::Bool(reader.varint()? != 0),
            _ => {
                reader.skip(wire)?;
                continue;
            }
        };
    }
    Ok(value)
}

fn decode_geometry(data: &[u8], kind: GeometryKind) -> Result<Vec<Vec<TilePoint>>, DecodeError> {
    let mut reader = Reader::new(data);
    let mut parts: Vec<Vec<TilePoint>> = Vec::new();
    let mut current: Vec<TilePoint> = Vec::new();
    let (mut x, mut y) = (0i32, 0i32);
    let zigzag = |v: u64| ((v >> 1) as i32) ^ -((v & 1) as i32);
    while !reader.done() {
        let command = reader.varint()? as u32;
        let (id, count) = (command & 7, command >> 3);
        match id {
            1 | 2 => {
                for _ in 0..count {
                    x = x.wrapping_add(zigzag(reader.varint()?));
                    y = y.wrapping_add(zigzag(reader.varint()?));
                    if id == 1 && kind != GeometryKind::Point && !current.is_empty() {
                        parts.push(std::mem::take(&mut current));
                    }
                    current.push([x, y]);
                }
            }
            7 => {
                if let Some(first) = current.first().copied() {
                    current.push(first);
                }
            }
            _ => return Err(DecodeError("unknown geometry command")),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    if kind == GeometryKind::Point {
        // Each point of a multipoint is its own part.
        return Ok(parts.into_iter().flatten().map(|p| vec![p]).collect());
    }
    Ok(parts)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Default)]
    pub(crate) struct Writer(pub Vec<u8>);

    impl Writer {
        pub fn varint(&mut self, mut v: u64) {
            loop {
                let byte = (v & 0x7f) as u8;
                v >>= 7;
                if v == 0 {
                    self.0.push(byte);
                    return;
                }
                self.0.push(byte | 0x80);
            }
        }
        pub fn key(&mut self, field: u32, wire: u8) {
            self.varint(u64::from(field) << 3 | u64::from(wire));
        }
        pub fn bytes(&mut self, field: u32, bytes: &[u8]) {
            self.key(field, 2);
            self.varint(bytes.len() as u64);
            self.0.extend_from_slice(bytes);
        }
        pub fn packed(&mut self, field: u32, values: &[u32]) {
            let mut inner = Writer(Vec::new());
            for v in values {
                inner.varint(u64::from(*v));
            }
            self.bytes(field, &inner.0);
        }
    }

    pub(crate) fn zz(v: i32) -> u32 {
        ((v << 1) ^ (v >> 31)) as u32
    }

    pub(crate) type TestFeature<'a> = (u32, Vec<u32>, Vec<(&'a str, &'a str)>);

    /// Encodes one layer of features given as (kind, geometry commands, string properties).
    pub(crate) fn encode_layer(name: &str, features: &[TestFeature]) -> Vec<u8> {
        let mut keys: Vec<&str> = Vec::new();
        let mut values: Vec<&str> = Vec::new();
        let mut layer = Writer(Vec::new());
        layer.bytes(1, name.as_bytes());
        for (kind, geometry, props) in features {
            let mut tags = Vec::new();
            for (k, v) in props {
                let ki = keys.iter().position(|x| x == k).unwrap_or_else(|| {
                    keys.push(k);
                    keys.len() - 1
                });
                let vi = values.iter().position(|x| x == v).unwrap_or_else(|| {
                    values.push(v);
                    values.len() - 1
                });
                tags.extend([ki as u32, vi as u32]);
            }
            let mut feature = Writer(Vec::new());
            feature.packed(2, &tags);
            feature.key(3, 0);
            feature.varint(u64::from(*kind));
            feature.packed(4, geometry);
            layer.bytes(2, &feature.0);
        }
        for k in keys {
            layer.bytes(3, k.as_bytes());
        }
        for v in values {
            let mut value = Writer(Vec::new());
            value.bytes(1, v.as_bytes());
            layer.bytes(4, &value.0);
        }
        layer.key(5, 0);
        layer.varint(4096);
        let mut tile = Writer(Vec::new());
        tile.bytes(3, &layer.0);
        tile.0
    }

    /// A closed square ring from (x, y) with side `s`, as geometry commands.
    pub(crate) fn square(x: i32, y: i32, s: i32) -> Vec<u32> {
        vec![
            9,
            zz(x),
            zz(y),
            (3 << 3) | 2,
            zz(s),
            zz(0),
            zz(0),
            zz(s),
            zz(-s),
            zz(0),
            15,
        ]
    }

    #[test]
    fn decodes_polygons_and_properties() {
        let tile = encode_layer("water", &[(3, square(0, 0, 100), vec![("class", "lake")])]);
        let layers = decode(&tile).unwrap();
        assert_eq!(layers[0].name, "water");
        let feature = &layers[0].features[0];
        assert_eq!(feature.kind, GeometryKind::Polygon);
        assert_eq!(
            feature.property("class").and_then(Value::as_str),
            Some("lake")
        );
        assert_eq!(feature.geometry[0].len(), 5);
        assert_eq!(feature.polygons().len(), 1);
    }

    #[test]
    fn decodes_multi_lines() {
        let geometry = vec![
            9,
            zz(0),
            zz(0),
            10,
            zz(10),
            zz(0),
            9,
            zz(5),
            zz(5),
            10,
            zz(0),
            zz(10),
        ];
        let tile = encode_layer("roads", &[(2, geometry, vec![])]);
        let layers = decode(&tile).unwrap();
        let lines = &layers[0].features[0].geometry;
        assert_eq!(lines, &vec![vec![[0, 0], [10, 0]], vec![[15, 5], [15, 15]]]);
    }
}
