//! MapLibre style documents.

pub(crate) mod color;
pub(crate) mod expr;

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value as Json;

use crate::value::Value;
use expr::{Context, Kind, Property};

/// Where the map's style comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum MapStyle {
    /// The address of a MapLibre `style.json`.
    Url(String),
    /// A MapLibre style document.
    Json(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LayerKind {
    Background,
    Fill,
    Line,
    Circle,
    Symbol,
    Raster,
    Hillshade,
    FillExtrusion,
    Heatmap,
}

impl LayerKind {
    fn parse(name: &str) -> Option<LayerKind> {
        Some(match name {
            "background" => LayerKind::Background,
            "fill" => LayerKind::Fill,
            "line" => LayerKind::Line,
            "circle" => LayerKind::Circle,
            "symbol" => LayerKind::Symbol,
            "raster" => LayerKind::Raster,
            "hillshade" => LayerKind::Hillshade,
            "fill-extrusion" => LayerKind::FillExtrusion,
            "heatmap" => LayerKind::Heatmap,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub(crate) struct Layer {
    pub id: String,
    pub kind: LayerKind,
    pub source: Option<String>,
    pub source_layer: Option<String>,
    pub min_zoom: f64,
    pub max_zoom: f64,
    pub filter: Option<Property>,
    pub visible: bool,
    pub paint: HashMap<String, Property>,
    pub layout: HashMap<String, Property>,
}

impl Layer {
    pub fn get(&self, name: &str) -> Property {
        self.paint
            .get(name)
            .or_else(|| self.layout.get(name))
            .cloned()
            .unwrap_or_else(|| default_property(name))
    }

    pub fn has(&self, name: &str) -> bool {
        self.paint.contains_key(name) || self.layout.contains_key(name)
    }

    pub fn shown_at(&self, zoom: f64) -> bool {
        // As in MapLibre, a minimum zoom of 0 sets no limit, so the layer shows below zoom 0.
        self.visible && (self.min_zoom <= 0.0 || zoom >= self.min_zoom) && zoom < self.max_zoom
    }

    pub fn number(&self, name: &str, zoom: f64) -> f64 {
        self.get(name).number(&Context::zoom(zoom), 0.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SourceKind {
    Vector,
    Raster {
        tile_size: u32,
    },
    RasterDem {
        tile_size: u32,
        terrarium: bool,
    },
    GeoJson {
        data: Json,
        options: GeoJsonOptions,
    },
    Image {
        url: String,
        coordinates: [[f64; 2]; 4],
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GeoJsonOptions {
    pub cluster: bool,
    pub cluster_radius: f64,
    pub cluster_max_zoom: u8,
    pub cluster_min_points: usize,
    pub max_zoom: u8,
    pub tolerance: f64,
    pub buffer: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Source {
    pub kind: SourceKind,
    pub url: Option<String>,
    pub tiles: Vec<String>,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub tms: bool,
    pub bounds: Option<[f64; 4]>,
    pub attribution: Option<String>,
}

impl Source {
    fn parse(json: &Json) -> Option<Source> {
        let kind_name = json.get("type")?.as_str()?;
        let tile_size = json.get("tileSize").and_then(Json::as_u64).unwrap_or(512) as u32;
        let kind = match kind_name {
            "vector" => SourceKind::Vector,
            "raster" => SourceKind::Raster { tile_size },
            "raster-dem" => SourceKind::RasterDem {
                tile_size,
                terrarium: json.get("encoding").and_then(Json::as_str) == Some("terrarium"),
            },
            "geojson" => SourceKind::GeoJson {
                data: json.get("data").cloned().unwrap_or(Json::Null),
                options: GeoJsonOptions {
                    cluster: json.get("cluster").and_then(Json::as_bool).unwrap_or(false),
                    cluster_radius: json
                        .get("clusterRadius")
                        .and_then(Json::as_f64)
                        .unwrap_or(50.0),
                    cluster_max_zoom: json
                        .get("clusterMaxZoom")
                        .and_then(Json::as_u64)
                        .map_or(14, |z| z as u8),
                    cluster_min_points: json
                        .get("clusterMinPoints")
                        .and_then(Json::as_u64)
                        .unwrap_or(2) as usize,
                    max_zoom: json.get("maxzoom").and_then(Json::as_u64).unwrap_or(18) as u8,
                    tolerance: json
                        .get("tolerance")
                        .and_then(Json::as_f64)
                        .unwrap_or(0.375),
                    buffer: json.get("buffer").and_then(Json::as_f64).unwrap_or(128.0),
                },
            },
            "image" => {
                let coordinates = json.get("coordinates")?.as_array()?;
                let mut corners = [[0.0; 2]; 4];
                for (slot, corner) in corners.iter_mut().zip(coordinates) {
                    let corner = corner.as_array()?;
                    *slot = [corner.first()?.as_f64()?, corner.get(1)?.as_f64()?];
                }
                SourceKind::Image {
                    url: json.get("url")?.as_str()?.to_string(),
                    coordinates: corners,
                }
            }
            _ => return None,
        };
        let mut source = Source {
            kind,
            url: json.get("url").and_then(Json::as_str).map(str::to_string),
            tiles: Vec::new(),
            min_zoom: 0,
            max_zoom: 22,
            tms: false,
            bounds: None,
            attribution: None,
        };
        if matches!(source.kind, SourceKind::Image { .. }) {
            source.url = None;
        }
        if let SourceKind::GeoJson { options, .. } = &source.kind {
            source.max_zoom = options.max_zoom;
            source.url = None;
        }
        source.apply_tilejson(json);
        Some(source)
    }

    /// Takes the tile addresses and limits a TileJSON document gives.
    pub fn apply_tilejson(&mut self, json: &Json) {
        if let Some(tiles) = json.get("tiles").and_then(Json::as_array) {
            self.tiles = tiles
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect();
        }
        if let Some(z) = json.get("minzoom").and_then(Json::as_u64) {
            self.min_zoom = z as u8;
        }
        if let Some(z) = json.get("maxzoom").and_then(Json::as_u64) {
            self.max_zoom = z as u8;
        }
        if let Some(scheme) = json.get("scheme").and_then(Json::as_str) {
            self.tms = scheme == "tms";
        }
        if let Some(bounds) = json.get("bounds").and_then(Json::as_array) {
            let b: Vec<f64> = bounds.iter().filter_map(Json::as_f64).collect();
            if b.len() == 4 {
                self.bounds = Some([b[0], b[1], b[2], b[3]]);
            }
        }
        if let Some(attribution) = json.get("attribution").and_then(Json::as_str) {
            self.attribution = Some(attribution.to_string());
        }
    }
}

/// How the map is laid out: flat, or as a globe that flattens into Mercator as it zooms in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MapProjection {
    Mercator,
    Globe,
}

#[derive(Debug)]
pub(crate) struct Style {
    pub sources: HashMap<String, Source>,
    pub layers: Vec<Arc<Layer>>,
    /// The `layers` array the layers were parsed from.
    pub layers_json: Json,
    pub glyphs: Option<String>,
    pub sprite: Vec<(String, String)>,
    pub projection: MapProjection,
    pub light: Light,
}

/// The style's root `light`, which shades extruded buildings.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Light {
    pub viewport_anchor: bool,
    /// Radial distance, azimuth, and polar angle in degrees.
    pub position: [f64; 3],
    pub color: color::Rgba,
    pub intensity: f64,
}

impl Default for Light {
    fn default() -> Self {
        Light {
            viewport_anchor: true,
            position: [1.15, 210.0, 30.0],
            color: color::Rgba([1.0, 1.0, 1.0, 1.0]),
            intensity: 0.5,
        }
    }
}

impl Style {
    pub fn from_json(json: &Json) -> Style {
        let sources = json
            .get("sources")
            .and_then(Json::as_object)
            .map(|sources| {
                sources
                    .iter()
                    .filter_map(|(id, source)| Some((id.clone(), Source::parse(source)?)))
                    .collect()
            })
            .unwrap_or_default();
        let layers_json = json.get("layers").cloned().unwrap_or(Json::Null);
        let layers = parse_layers(&layers_json);
        let sprite = match json.get("sprite") {
            Some(Json::String(url)) => vec![("default".to_string(), url.clone())],
            Some(Json::Array(items)) => items
                .iter()
                .filter_map(|item| {
                    Some((
                        item.get("id")?.as_str()?.to_string(),
                        item.get("url")?.as_str()?.to_string(),
                    ))
                })
                .collect(),
            _ => Vec::new(),
        };
        let projection = match json.get("projection").and_then(|p| p.get("type")) {
            Some(Json::String(kind)) if kind == "globe" => MapProjection::Globe,
            // A projection that changes by zoom is drawn as the globe it starts from.
            Some(Json::Array(items))
                if items
                    .iter()
                    .any(|i| i == "globe" || i == "vertical-perspective") =>
            {
                MapProjection::Globe
            }
            _ => MapProjection::Mercator,
        };
        let mut light = Light::default();
        if let Some(l) = json.get("light") {
            if let Some(anchor) = l.get("anchor").and_then(Json::as_str) {
                light.viewport_anchor = anchor != "map";
            }
            if let Some(position) = l.get("position").and_then(Json::as_array) {
                let p: Vec<f64> = position.iter().filter_map(Json::as_f64).collect();
                if p.len() == 3 {
                    light.position = [p[0], p[1], p[2]];
                }
            }
            if let Some(c) = l.get("color").and_then(Json::as_str).and_then(color::parse) {
                light.color = c;
            }
            if let Some(i) = l.get("intensity").and_then(Json::as_f64) {
                light.intensity = i;
            }
        }
        Style {
            sources,
            layers,
            layers_json,
            glyphs: json
                .get("glyphs")
                .and_then(Json::as_str)
                .map(str::to_string),
            sprite,
            projection,
            light,
        }
    }
}

/// The layers of a style's `layers` array that this crate can draw, in order.
pub(crate) fn parse_layers(json: &Json) -> Vec<Arc<Layer>> {
    json.as_array()
        .map(|layers| {
            layers
                .iter()
                .filter_map(parse_layer)
                .map(Arc::new)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_layer(json: &Json) -> Option<Layer> {
    let kind = LayerKind::parse(json.get("type")?.as_str()?)?;
    let properties = |section: &str| -> HashMap<String, Property> {
        json.get(section)
            .and_then(Json::as_object)
            .map(|props| {
                props
                    .iter()
                    .map(|(name, value)| {
                        (name.clone(), Property::parse(value, property_kind(name)))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let layout = properties("layout");
    let visible = layout
        .get("visibility")
        .and_then(|p| p.string(&Context::zoom(0.0)))
        .is_none_or(|v| v != "none");
    Some(Layer {
        id: json.get("id")?.as_str()?.to_string(),
        kind,
        source: json
            .get("source")
            .and_then(Json::as_str)
            .map(str::to_string),
        source_layer: json
            .get("source-layer")
            .and_then(Json::as_str)
            .map(str::to_string),
        min_zoom: json.get("minzoom").and_then(Json::as_f64).unwrap_or(0.0),
        max_zoom: json.get("maxzoom").and_then(Json::as_f64).unwrap_or(24.0),
        filter: json.get("filter").map(Property::filter),
        visible,
        paint: properties("paint"),
        layout,
    })
}

pub(crate) fn property_kind(name: &str) -> Kind {
    if name.ends_with("-color") || name == "line-gradient" {
        return Kind::Color;
    }
    match name {
        "text-field"
        | "icon-image"
        | "text-transform"
        | "symbol-placement"
        | "text-anchor"
        | "icon-anchor"
        | "line-cap"
        | "line-join"
        | "visibility"
        | "text-justify"
        | "text-rotation-alignment"
        | "text-pitch-alignment"
        | "icon-rotation-alignment"
        | "icon-pitch-alignment"
        | "fill-translate-anchor"
        | "line-translate-anchor"
        | "circle-pitch-alignment"
        | "circle-pitch-scale"
        | "icon-text-fit"
        | "raster-resampling"
        | "hillshade-illumination-anchor"
        | "symbol-z-order"
        | "fill-pattern"
        | "line-pattern"
        | "background-pattern"
        | "fill-extrusion-pattern" => Kind::String,
        "text-font"
        | "line-dasharray"
        | "text-offset"
        | "icon-offset"
        | "fill-translate"
        | "line-translate"
        | "circle-translate"
        | "text-translate"
        | "icon-translate"
        | "fill-extrusion-translate"
        | "text-variable-anchor"
        | "icon-padding"
        | "text-writing-mode"
        | "icon-text-fit-padding" => Kind::Array,
        "text-allow-overlap"
        | "icon-allow-overlap"
        | "text-ignore-placement"
        | "icon-ignore-placement"
        | "text-optional"
        | "icon-optional"
        | "fill-antialias"
        | "symbol-avoid-edges"
        | "text-keep-upright"
        | "icon-keep-upright"
        | "fill-extrusion-vertical-gradient" => Kind::Bool,
        _ => Kind::Number,
    }
}

fn default_value(name: &str) -> Value {
    let n = Value::Number;
    let s = |v: &str| Value::String(v.into());
    let black = Value::Color(color::Rgba::BLACK);
    let transparent = Value::Color(color::Rgba::TRANSPARENT);
    match name {
        "background-color"
        | "fill-color"
        | "line-color"
        | "circle-color"
        | "text-color"
        | "icon-color"
        | "fill-extrusion-color"
        | "hillshade-shadow-color" => black,
        "circle-stroke-color" | "text-halo-color" | "icon-halo-color" => transparent,
        "hillshade-highlight-color" => Value::Color(color::Rgba([1.0, 1.0, 1.0, 1.0])),
        "hillshade-accent-color" => black,
        "background-opacity"
        | "fill-opacity"
        | "line-opacity"
        | "circle-opacity"
        | "circle-stroke-opacity"
        | "text-opacity"
        | "icon-opacity"
        | "raster-opacity"
        | "fill-extrusion-opacity"
        | "heatmap-opacity"
        | "icon-size"
        | "raster-contrast-unused"
        | "raster-brightness-max"
        | "heatmap-weight"
        | "heatmap-intensity" => n(1.0),
        "line-width" => n(1.0),
        "circle-radius" => n(5.0),
        "text-size" => n(16.0),
        "text-max-width" => n(10.0),
        "text-line-height" => n(1.2),
        "text-padding" => n(2.0),
        "icon-padding" => n(2.0),
        "text-max-angle" => n(45.0),
        "symbol-spacing" => n(250.0),
        "heatmap-radius" => n(30.0),
        "hillshade-exaggeration" => n(0.5),
        "hillshade-illumination-direction" => n(335.0),
        "raster-fade-duration" => n(300.0),
        "line-miter-limit" => n(2.0),
        "line-round-limit" => n(1.05),
        "fill-antialias" => Value::Bool(true),
        "fill-extrusion-vertical-gradient" => Value::Bool(true),
        "text-keep-upright" => Value::Bool(true),
        "text-font" => {
            Value::Array(vec![s("Open Sans Regular"), s("Arial Unicode MS Regular")].into())
        }
        "text-anchor" | "icon-anchor" => s("center"),
        "text-justify" => s("center"),
        "symbol-placement" => s("point"),
        "line-cap" => s("butt"),
        "line-join" => s("miter"),
        "text-transform" => s("none"),
        "text-rotation-alignment"
        | "icon-rotation-alignment"
        | "text-pitch-alignment"
        | "icon-pitch-alignment" => s("auto"),
        "fill-translate-anchor" | "line-translate-anchor" | "circle-translate-anchor" => s("map"),
        "circle-pitch-alignment" | "circle-pitch-scale" => s("viewport"),
        "text-offset" | "icon-offset" | "fill-translate" | "line-translate"
        | "circle-translate" | "text-translate" | "icon-translate" => {
            Value::Array(vec![n(0.0), n(0.0)].into())
        }
        "heatmap-color" => Value::Null,
        _ => n(0.0),
    }
}

pub(crate) fn default_property(name: &str) -> Property {
    Property::constant(default_value(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_style() {
        let json: Json = serde_json::from_str(
            r##"{
              "version": 8,
              "glyphs": "https://example.com/{fontstack}/{range}.pbf",
              "sources": {
                "omt": {"type": "vector", "url": "https://example.com/planet"},
                "dem": {"type": "raster-dem", "tiles": ["https://e.com/{z}/{x}/{y}.png"], "encoding": "terrarium"}
              },
              "layers": [
                {"id": "bg", "type": "background", "paint": {"background-color": "#f8f4f0"}},
                {"id": "water", "type": "fill", "source": "omt", "source-layer": "water",
                 "filter": ["==", "$type", "Polygon"], "paint": {"fill-color": "rgb(158,189,255)"}},
                {"id": "hidden", "type": "line", "source": "omt", "source-layer": "x", "layout": {"visibility": "none"}},
                {"id": "unknown", "type": "sky"}
              ]
            }"##,
        )
        .unwrap();
        let style = Style::from_json(&json);
        assert_eq!(style.layers.len(), 3);
        assert!(!style.layers[2].visible);
        assert_eq!(
            style.sources["omt"].url.as_deref(),
            Some("https://example.com/planet")
        );
        assert!(matches!(
            style.sources["dem"].kind,
            SourceKind::RasterDem {
                terrarium: true,
                ..
            }
        ));
        let water = &style.layers[1];
        let c = water
            .get("fill-color")
            .color(&Context::zoom(0.0), color::Rgba::BLACK);
        assert!((c.0[2] - 1.0).abs() < 1e-6);
        assert_eq!(water.number("fill-opacity", 3.0), 1.0);
    }
}
