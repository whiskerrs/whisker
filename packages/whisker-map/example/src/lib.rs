//! `whisker-map` example app.
//!
//! An OpenFreeMap style centered on Tokyo Station, with a marker and buttons that move the camera.

use std::time::Duration;
use whisker::css::{FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;

use whisker_map::{
    LatLng, MapCamera, MapProjection, MapRef, MapStyle, MapTap, MapView, Marker, RenderedQuery,
};
use whisker_safe_area::safe_area_insets;

const TOKYO_STATION: LatLng = LatLng::new(35.6812, 139.7671);
const OSAKA_STATION: LatLng = LatLng::new(34.7025, 135.4959);
const FLORENCE: LatLng = LatLng::new(43.7696, 11.2558);

/// Scatters `count` points around Tokyo Station, the same ones on every run.
fn sample_points(count: u32) -> serde_json::Value {
    let mut seed: u64 = 7;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as f64 / f64::from(1u32 << 31) - 0.5
    };
    let features: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            let lng = TOKYO_STATION.lng + next() * 0.2;
            let lat = TOKYO_STATION.lat + next() * 0.15;
            serde_json::json!({
                "type": "Feature",
                "id": i,
                "properties": {"name": format!("Point {i}")},
                "geometry": {"type": "Point", "coordinates": [lng, lat]},
            })
        })
        .collect();
    serde_json::json!({"type": "FeatureCollection", "features": features})
}

/// Adds clustered sample points above the style's own layers.
fn add_points(map_ref: MapRef) {
    map_ref.add_source(
        "points",
        serde_json::json!({
            "type": "geojson",
            "data": sample_points(400),
            "cluster": true,
            "clusterRadius": 50,
        }),
    );
    map_ref.add_layer(
        serde_json::json!({
            "id": "clusters",
            "type": "circle",
            "source": "points",
            "filter": ["has", "point_count"],
            "paint": {
                "circle-color": ["step", ["get", "point_count"], "#51bbd6", 20, "#f1f075", 60, "#f28cb1"],
                "circle-radius": ["step", ["get", "point_count"], 14, 20, 18, 60, 24],
                "circle-stroke-width": 2,
                "circle-stroke-color": "#ffffff",
            },
        }),
        None,
    );
    map_ref.add_layer(
        serde_json::json!({
            "id": "cluster-count",
            "type": "symbol",
            "source": "points",
            "filter": ["has", "point_count"],
            "layout": {
                "text-field": ["get", "point_count_abbreviated"],
                "text-font": ["Noto Sans Bold"],
                "text-size": 12,
                "text-allow-overlap": true,
            },
        }),
        None,
    );
    map_ref.add_layer(
        serde_json::json!({
            "id": "point",
            "type": "circle",
            "source": "points",
            "filter": ["!", ["has", "point_count"]],
            "paint": {
                "circle-color": "#11b4da",
                "circle-radius": 5,
                "circle-stroke-width": 1,
                "circle-stroke-color": "#ffffff",
            },
        }),
        None,
    );
}

/// Draws a route around central Tokyo that shades from its start to its end.
fn add_route(map_ref: MapRef) {
    map_ref.add_source(
        "route",
        serde_json::json!({
            "type": "geojson",
            "lineMetrics": true,
            "data": {
                "type": "Feature",
                "properties": {},
                "geometry": {
                    "type": "LineString",
                    "coordinates": [
                        [139.7671, 35.6812], [139.7454, 35.6586], [139.7016, 35.6580],
                        [139.7006, 35.6896], [139.7109, 35.7295], [139.7774, 35.7138],
                        [139.7671, 35.6812],
                    ],
                },
            },
        }),
    );
    map_ref.add_layer(
        serde_json::json!({
            "id": "route",
            "type": "line",
            "source": "route",
            "layout": {"line-cap": "round", "line-join": "round"},
            "paint": {
                "line-width": 8,
                "line-gradient": [
                    "interpolate", ["linear"], ["line-progress"],
                    0, "#2563eb", 0.5, "#a855f7", 1, "#ef4444",
                ],
            },
        }),
        Some("clusters"),
    );
}

/// Shades the terrain from AWS's open Terrarium elevation tiles, under the water.
fn add_relief(map_ref: MapRef) {
    map_ref.add_source(
        "dem",
        serde_json::json!({
            "type": "raster-dem",
            "tiles": ["https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png"],
            "encoding": "terrarium",
            "tileSize": 256,
            "maxzoom": 15,
        }),
    );
    map_ref.add_layer(
        serde_json::json!({
            "id": "relief",
            "type": "hillshade",
            "source": "dem",
            "paint": {"hillshade-exaggeration": 0.6},
        }),
        Some("water"),
    );
}

/// Outlines Florence's roads and buildings, read by byte range from a single PMTiles archive.
fn add_florence(map_ref: MapRef) {
    map_ref.add_source(
        "florence",
        serde_json::json!({
            "type": "vector",
            "url": "pmtiles://https://pmtiles.io/protomaps(vector)ODbL_firenze.pmtiles",
        }),
    );
    map_ref.add_layer(
        serde_json::json!({
            "id": "florence-buildings",
            "type": "fill",
            "source": "florence",
            "source-layer": "buildings",
            "paint": {"fill-color": "#7b61ff", "fill-opacity": 0.35},
        }),
        None,
    );
    map_ref.add_layer(
        serde_json::json!({
            "id": "florence-roads",
            "type": "line",
            "source": "florence",
            "source-layer": "roads",
            "paint": {"line-color": "#e5484d", "line-width": 1.5},
        }),
        None,
    );
    map_ref.fly_to(MapCamera {
        center: FLORENCE,
        zoom: 15.0,
        ..Default::default()
    });
}

fn button() -> Css {
    Css::new()
        .padding_left(px(10))
        .padding_right(px(10))
        .padding_top(px(4))
        .padding_bottom(px(4))
        .margin_left(px(8))
        .border_radius(px(6))
        .background_color(Color::hex(0x2e5a80))
        .color(Color::hex(0xffffff))
        .font_size(px(13))
}

fn pin() -> Css {
    Css::new()
        .width(px(18))
        .height(px(18))
        .border_radius(px(9))
        .background_color(Color::hex(0xe5484d))
        .border_top_width(px(3))
        .border_right_width(px(3))
        .border_bottom_width(px(3))
        .border_left_width(px(3))
        .border_top_color(Color::hex(0xffffff))
        .border_right_color(Color::hex(0xffffff))
        .border_bottom_color(Color::hex(0xffffff))
        .border_left_color(Color::hex(0xffffff))
}

#[whisker::main]
pub fn app() -> Element {
    let camera = signal(MapCamera {
        center: TOKYO_STATION,
        zoom: 13.0,
        ..Default::default()
    });
    let map_ref = MapRef::new();
    add_points(map_ref);
    add_route(map_ref);
    let tapped = signal(String::new());
    let globe = signal(false);
    let insets = safe_area_insets();
    let on_tap = move |tap: MapTap| {
        let layers: Vec<&str> = tap
            .features
            .iter()
            .take(3)
            .map(|f| f.layer.as_str())
            .collect();
        let name = tap
            .features
            .iter()
            .find_map(|f| f.properties.get("name").and_then(|n| n.as_str()))
            .unwrap_or("");
        let text = format!("{} {name}", layers.join(", "));
        tapped.set(text);
        let cluster = tap.features.iter().find_map(|f| {
            let id = f.properties.get("cluster_id")?.as_u64()?;
            let at = f.geometry.get("coordinates")?.as_array()?;
            Some((id, LatLng::new(at[1].as_f64()?, at[0].as_f64()?)))
        });
        if let Some((id, center)) = cluster {
            whisker::spawn_local(async move {
                if let Some(zoom) = map_ref.cluster_expansion_zoom("points", id).await {
                    let target = MapCamera {
                        center,
                        zoom,
                        ..camera.get_untracked()
                    };
                    map_ref.ease_to(target, Duration::from_millis(500));
                }
            });
        }
    };
    let settled = signal(String::new());
    let on_idle = move || {
        let shown = map_ref.query_rendered_features(
            None,
            RenderedQuery {
                layers: Some(vec!["clusters".into(), "point".into()]),
                ..Default::default()
            },
        );
        let clusters = shown.iter().filter(|f| f.layer == "clusters").count();
        settled.set(format!(
            "{clusters} clusters, {} points",
            shown.len() - clusters
        ));
    };
    let page = Css::new()
        .flex_grow(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .background_color(Color::hex(0xf2efe9));
    let map = Css::new().flex_grow(1.0);
    let bar = Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .padding(px(12))
        .background_color(Color::hex(0xffffff));
    let label = Css::new()
        .flex_grow(1.0)
        .font_size(px(13))
        .font_weight(FontWeight::Numeric(600))
        .color(Color::hex(0x333333));
    render! {
        View(style: page) {
            View(style: computed(move || bar.clone().padding_top(px(insets.get().top as f32 + 12.0)))) {
                Text(
                    style: label.clone(),
                    value: computed(move || {
                        let c = camera.get();
                        format!(
                            "{:.4}, {:.4}  z{:.2}  {:.0}°  {:.0}°",
                            c.center.lat, c.center.lng, c.zoom, c.bearing, c.pitch
                        )
                    }),
                )
                Text(style: label.clone(), value: computed(move || tapped.get()))
                Text(style: label, value: computed(move || settled.get()))
                Text(
                    style: button(),
                    value: "Tokyo",
                    on_tap: move |_| map_ref.fly_to(MapCamera { center: TOKYO_STATION, zoom: 15.0, ..Default::default() }),
                )
                Text(
                    style: button(),
                    value: "Osaka",
                    on_tap: move |_| map_ref.fly_to(MapCamera { center: OSAKA_STATION, zoom: 15.0, ..Default::default() }),
                )
                Text(
                    style: button(),
                    value: computed(move || if globe.get() { "Flat" } else { "Globe" }.to_string()),
                    on_tap: move |_| {
                        let round = !globe.get_untracked();
                        globe.set(round);
                        if round {
                            map_ref.set_projection(MapProjection::Globe);
                            let mut target = camera.get_untracked();
                            target.zoom = 1.8;
                            target.pitch = 0.0;
                            map_ref.fly_to(target);
                        } else {
                            map_ref.set_projection(MapProjection::Mercator);
                        }
                    },
                )
                Text(style: button(), value: "Relief", on_tap: move |_| add_relief(map_ref))
                Text(style: button(), value: "PMTiles", on_tap: move |_| add_florence(map_ref))
                Text(
                    style: button(),
                    value: "Tilt",
                    on_tap: move |_| {
                        let mut target = camera.get_untracked();
                        target.pitch = if target.pitch > 0.0 { 0.0 } else { 60.0 };
                        target.bearing = if target.pitch > 0.0 { -30.0 } else { 0.0 };
                        map_ref.ease_to(target, Duration::from_millis(800));
                    },
                )
            }
            MapView(
                map_style: MapStyle::Url("https://tiles.openfreemap.org/styles/liberty".into()),
                camera: camera,
                style: map,
                on_tap: on_tap,
                on_idle: on_idle,
                map_ref: map_ref,
            ) {
                Marker(position: TOKYO_STATION) { View(style: pin()) }
            }
        }
    }
}
