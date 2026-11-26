//! Live Cities: simulated people walking the real streets of a few cities.
//!
//! A spinning globe shows how many people are out in each city. Pick one and the camera flies
//! down to street level, where people walk the roads read from the map's own tiles. Zoomed out
//! they gather into clusters; up close each is a face bubble, and the one in focus steps out as a
//! 3D avatar inside the map, hidden by the buildings in front of them.

mod cities;
mod people;

use std::cell::{Cell, RefCell};
use std::time::Duration;

use whisker::css::shorthand::padding_margin::MarginValue;
use whisker::css::{
    AlignItems, Animation, AnimationDirection, AnimationFillMode, AnimationIterationCount,
    EasingFunction, FlexDirection, FontWeight, JustifyContent, Keyframes, Overflow, Position,
    PositionKind, TransformFn,
};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_image::{Image, ImageMode};
use whisker_map::{
    LatLng, MapCamera, MapLayer, MapProjection, MapRef, MapStyle, MapTap, MapView, Marker,
    SourceQuery,
};
use whisker_safe_area::safe_area_insets;
use whisker_scene::math::{Quat, Vec3};
use whisker_scene::{
    AnimationOptions, DirectionalLight, HemisphereLight, Model, ModelView, Scene, SceneContent,
    SceneHit,
};

use crate::cities::{CITIES, LOOKS, css_color};
use crate::people::{Crowd, Mood, Roads};

const PEOPLE: [usize; 5] = [260, 230, 210, 190, 200];
/// Height of the avatar models, in their own units.
const MODEL_HEIGHT: f32 = 1.2;
/// How tall the 3D avatar stands on screen, in logical pixels, whatever the zoom.
const AVATAR_PIXELS: f64 = 84.0;
/// Up to this zoom people gather into clusters.
const CLUSTER_ZOOM: f64 = 17.0;
/// From this zoom everyone on screen is drawn in 3D.
const AVATAR_ZOOM: f64 = CLUSTER_ZOOM + 1.0;
const BUBBLE: f32 = 56.0;
/// From a bubble's bottom, where it points, up to its center.
const BUBBLE_LIFT: f64 = 46.0;
/// How long a 3D avatar takes to pop up, in seconds.
const POP_SECONDS: f64 = 0.45;
const UPDATE_SECONDS: f32 = 0.35;
const CHASE_PITCH: f32 = 72.0;
const CHASE_ZOOM: f64 = 19.6;
/// How far below the screen's middle the followed avatar stands, as a share of its height.
const CHASE_LOW: f64 = 0.16;
const HUDDLE_WIDTH: f32 = 92.0;
/// How far from a cluster's center, in logical pixels, someone is counted among its faces.
const HUDDLE_REACH: f64 = 120.0;

/// A cluster of people, drawn with two of their faces.
#[derive(Clone, PartialEq)]
struct Huddle {
    cluster: u64,
    count: usize,
    faces: (usize, Option<usize>),
    members: Vec<usize>,
}

const DAY_STYLE: &str = include_str!("../assets/style-day.json");
const NIGHT_STYLE: &str = include_str!("../assets/style-night.json");

mod ink {
    use whisker::prelude::Color;

    pub fn pink() -> Color {
        Color::hex(0xff3d7f)
    }
    pub fn ink() -> Color {
        Color::hex(0x2b2350)
    }
    pub fn soft() -> Color {
        Color::hex(0x8a82a8)
    }
    pub fn white() -> Color {
        Color::hex(0xffffff)
    }
    pub fn shadow() -> Color {
        Color::rgba(43, 35, 80, 0.18)
    }
}

/// Everything the frame loop, the map and the UI share.
#[derive(Clone, Copy)]
struct App {
    camera: RwSignal<MapCamera>,
    city: RwSignal<Option<usize>>,
    intro: RwSignal<bool>,
    night: RwSignal<bool>,
    selected: RwSignal<Option<usize>>,
    following: RwSignal<bool>,
    /// Bumped every frame the crowd moves.
    tick: RwSignal<u64>,
    /// Bumped a few times a second, for text that need not change every frame.
    pulse: RwSignal<u64>,
    /// Who is on screen outside a cluster.
    shown: RwSignal<Vec<usize>>,
    huddles: RwSignal<Vec<Huddle>>,
    /// Whether the camera is close enough for everyone on screen to be drawn in 3D.
    close: ReadSignal<bool>,
    /// Seconds since the app started.
    clock: RwSignal<f64>,
    crowd: StoredValue<RefCell<Option<Crowd>>>,
    map: StoredValue<Cell<Option<MapRef>>>,
}

impl App {
    fn with_crowd<R>(self, f: impl FnOnce(&Crowd) -> R) -> Option<R> {
        self.crowd.with(|c| c.borrow().as_ref().map(f))
    }

    fn map_ref(self) -> Option<MapRef> {
        self.map.with(Cell::get)
    }

    fn go(self, index: usize) {
        self.intro.set(false);
        self.selected.set(None);
        self.following.set(false);
        if self.city.get_untracked() != Some(index) {
            self.city.set(Some(index));
            self.crowd.with(|c| c.borrow_mut().take());
            self.shown.set(Vec::new());
            self.huddles.set(Vec::new());
            if let Some(map) = self.map_ref() {
                map.set_geojson("people", empty());
            }
        }
        let city = &CITIES[index];
        if let Some(map) = self.map_ref() {
            map.fly_to(MapCamera {
                center: city.center,
                zoom: 17.0,
                bearing: city.bearing,
                pitch: 50.0,
            });
        }
    }

    fn globe(self) {
        self.selected.set(None);
        self.following.set(false);
        if let Some(map) = self.map_ref() {
            map.fly_to(MapCamera {
                center: self.camera.get_untracked().center,
                zoom: 1.4,
                bearing: 0.0,
                pitch: 0.0,
            });
        }
    }

    /// Reads the streets once the map has the city's tiles and puts people on them.
    fn populate(self) {
        let Some(index) = self.city.get_untracked() else {
            return;
        };
        let city = &CITIES[index];
        let ready = self
            .with_crowd(|c| c.center == city.center)
            .unwrap_or(false);
        let camera = self.camera.get_untracked();
        let arrived = camera.zoom >= 14.0
            && people::to_meters(city.center, camera.center)
                .iter()
                .all(|m| m.abs() < 600.0);
        let Some(map) = self.map_ref() else { return };
        if ready || !arrived {
            return;
        }
        let classes = serde_json::json!([
            "match",
            ["get", "class"],
            [
                "minor",
                "service",
                "path",
                "track",
                "tertiary",
                "secondary",
                "primary"
            ],
            true,
            false,
        ]);
        // Street names live in `transportation_name`, whose lines share their points with the
        // unnamed `transportation` lines that also cover every small street.
        let features: Vec<_> = ["transportation", "transportation_name"]
            .into_iter()
            .flat_map(|layer| {
                map.query_source_features(
                    "openmaptiles",
                    SourceQuery {
                        source_layer: Some(layer.into()),
                        filter: Some(serde_json::json!([
                            "all",
                            classes,
                            ["!=", ["get", "brunnel"], "tunnel"],
                            ["!=", ["get", "subclass"], "platform"],
                            ["!=", ["get", "indoor"], 1],
                        ])),
                    },
                )
            })
            .collect();
        let roads = Roads::from_streets(
            city.center,
            features.iter().map(|f| {
                let name = ["name_en", "name:latin", "name"]
                    .iter()
                    .find_map(|key| f.properties.get(*key).and_then(|n| n.as_str()))
                    .unwrap_or("");
                (name, &f.geometry)
            }),
        );
        if roads.is_empty() {
            return;
        }
        let crowd = Crowd::new(
            city.center,
            roads,
            PEOPLE[index],
            LOOKS.len(),
            index as u64 + 1,
        );
        self.crowd.with(|c| *c.borrow_mut() = Some(crowd));
        self.refresh();
    }

    /// Sends everyone's position to the map and picks who gets a bubble.
    fn refresh(self) {
        let colors: Vec<String> = LOOKS.iter().map(|l| css_color(l.color)).collect();
        let colors: Vec<&str> = colors.iter().map(String::as_str).collect();
        let Some(json) = self.with_crowd(|crowd| crowd.geojson(&colors)) else {
            return;
        };
        let Some(map) = self.map_ref() else { return };
        let features = map.query_source_features(
            "people",
            SourceQuery {
                source_layer: None,
                filter: None,
            },
        );
        let mut unclustered: Vec<usize> = features
            .iter()
            .filter_map(|f| f.properties.get("walker")?.as_u64())
            .map(|id| id as usize)
            .collect();
        unclustered.sort_unstable();
        unclustered.dedup();
        let viewport = whisker::viewport().get_untracked();
        let (width, height) = (f64::from(viewport.width), f64::from(viewport.height));
        let on_screen = |(x, y): (f64, f64)| {
            (-60.0..width + 60.0).contains(&x) && (-20.0..height + 80.0).contains(&y)
        };
        let shown: Vec<usize> = unclustered
            .iter()
            .copied()
            .filter(|&id| {
                self.place_of(id)
                    .and_then(|place| map.to_screen(place))
                    .is_some_and(on_screen)
            })
            .collect();
        if self.shown.with_untracked(|s| *s != shown) {
            self.shown.set(shown);
        }
        let mut clusters: Vec<(u64, usize, (f64, f64))> = Vec::new();
        for f in &features {
            let (Some(id), Some(count)) = (
                f.properties.get("cluster_id").and_then(|v| v.as_u64()),
                f.properties.get("point_count").and_then(|v| v.as_u64()),
            ) else {
                continue;
            };
            if clusters.iter().any(|c| c.0 == id) {
                continue;
            }
            if let Some(at) = point(&f.geometry).and_then(|p| map.to_screen(p))
                && on_screen(at)
            {
                clusters.push((id, count as usize, at));
            }
        }
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); clusters.len()];
        let looks = self
            .with_crowd(|c| c.walkers.iter().map(|w| w.look).collect::<Vec<_>>())
            .unwrap_or_default();
        for id in 0..looks.len() {
            if unclustered.binary_search(&id).is_ok() {
                continue;
            }
            let Some((x, y)) = self.place_of(id).and_then(|p| map.to_screen(p)) else {
                continue;
            };
            let nearest = clusters
                .iter()
                .enumerate()
                .map(|(i, c)| (i, (x - c.2.0).hypot(y - c.2.1)))
                .filter(|(_, d)| *d < HUDDLE_REACH)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = nearest {
                members[i].push(id);
            }
        }
        let huddles: Vec<Huddle> = clusters
            .iter()
            .zip(members)
            .filter(|(_, members)| !members.is_empty())
            .map(|(&(cluster, count, _), members)| {
                let first = members[0];
                let second = members
                    .iter()
                    .find(|&&id| looks[id] != looks[first])
                    .or(members.get(1))
                    .copied();
                Huddle {
                    cluster,
                    count,
                    faces: (first, second),
                    members,
                }
            })
            .collect();
        if self.huddles.with_untracked(|h| *h != huddles) {
            self.huddles.set(huddles);
        }
        map.set_geojson("people", json);
        self.pulse.update(|p| *p += 1);
    }

    /// Who is drawn as a face bubble.
    fn bubbles(self) -> Vec<usize> {
        if self.close.get() {
            return Vec::new();
        }
        let selected = self.selected.get();
        let mut shown = self.shown.get();
        shown.retain(|&id| Some(id) != selected);
        shown
    }

    fn huddles(self) -> Vec<Huddle> {
        if self.close.get() {
            return Vec::new();
        }
        self.huddles.get()
    }

    /// Where a huddle's members stand on average.
    fn middle_of(self, members: &[usize]) -> Option<LatLng> {
        let places: Vec<LatLng> = members.iter().filter_map(|&id| self.place_of(id)).collect();
        let n = places.len() as f64;
        (n > 0.0).then(|| {
            LatLng::new(
                places.iter().map(|p| p.lat).sum::<f64>() / n,
                places.iter().map(|p| p.lng).sum::<f64>() / n,
            )
        })
    }

    /// Who is drawn in 3D: everyone on screen up close, otherwise only the person in focus.
    fn avatars(self) -> Vec<usize> {
        let mut avatars = if self.close.get() {
            self.shown.get()
        } else {
            Vec::new()
        };
        if let Some(id) = self.selected.get()
            && !avatars.contains(&id)
        {
            avatars.push(id);
        }
        avatars
    }

    fn place_of(self, id: usize) -> Option<LatLng> {
        self.with_crowd(|c| {
            c.walkers
                .get(id)
                .map(|w| people::to_latlng(c.center, c.place(w).0))
        })
        .flatten()
    }

    fn step(self, dt: f32, since_refresh: &Cell<f32>) {
        self.clock.update(|t| *t += f64::from(dt));
        if self.intro.get_untracked() {
            let mut camera = self.camera.get_untracked();
            if camera.zoom < 1.6 {
                camera.center.lng =
                    (camera.center.lng + f64::from(dt) * 6.0 + 180.0).rem_euclid(360.0) - 180.0;
                self.camera.set(camera);
            }
        }
        let moved = self
            .crowd
            .with(|c| {
                c.borrow_mut()
                    .as_mut()
                    .map(|crowd| crowd.step(f64::from(dt)))
            })
            .is_some();
        if !moved {
            return;
        }
        self.tick.update(|t| *t += 1);
        if self.following.get_untracked()
            && let Some(id) = self.selected.get_untracked()
        {
            self.chase(id, dt);
        }
        since_refresh.set(since_refresh.get() + dt);
        if since_refresh.get() >= UPDATE_SECONDS {
            since_refresh.set(0.0);
            self.refresh();
        }
    }

    /// Eases the camera in behind `id`, looking the way they walk with them low on the screen.
    fn chase(self, id: usize, dt: f32) {
        let Some((place, heading)) = self.with_crowd(|c| c.place(&c.walkers[id])) else {
            return;
        };
        let mut camera = self.camera.get_untracked();
        let ease = |rate: f32| 1.0 - (-dt * rate).exp();
        let turn = (heading.to_degrees() as f32 - camera.bearing + 540.0).rem_euclid(360.0) - 180.0;
        camera.bearing += turn * ease(2.5);
        camera.pitch += (CHASE_PITCH - camera.pitch) * ease(3.0);
        camera.zoom += (CHASE_ZOOM - camera.zoom) * f64::from(ease(3.0));
        let viewport = whisker::viewport().get_untracked();
        let meters_per_pixel = 40_075_016.686 * camera.center.lat.to_radians().cos()
            / (512.0 * 2f64.powf(camera.zoom));
        let ahead = CHASE_LOW * f64::from(viewport.height) * meters_per_pixel
            / f64::from(camera.pitch).to_radians().cos();
        let facing = f64::from(camera.bearing).to_radians();
        let Some(target) = self.with_crowd(|c| {
            people::to_latlng(
                c.center,
                [
                    place[0] + facing.sin() * ahead,
                    place[1] + facing.cos() * ahead,
                ],
            )
        }) else {
            return;
        };
        let k = f64::from(ease(6.0));
        camera.center = LatLng::new(
            camera.center.lat + (target.lat - camera.center.lat) * k,
            camera.center.lng + (target.lng - camera.center.lng) * k,
        );
        self.camera.set(camera);
    }

    fn tap(self, tap: MapTap) {
        self.intro.set(false);
        let cities = tap
            .features
            .iter()
            .find(|f| matches!(f.layer.as_str(), "city-bubbles" | "city-names"));
        if let Some(index) = cities.and_then(|f| f.properties.get("city")?.as_u64()) {
            self.go(index as usize);
            return;
        }
        if let Some((id, center)) = self.huddle_at(tap.x, tap.y) {
            let (app, map) = (self, self.map_ref());
            whisker::spawn_local(async move {
                let Some(map) = map else { return };
                let camera = app.camera.get_untracked();
                let zoom = map
                    .cluster_expansion_zoom("people", id)
                    .await
                    .unwrap_or(camera.zoom + 2.0);
                let camera = MapCamera {
                    center,
                    zoom: zoom.max(CLUSTER_ZOOM + 0.5),
                    ..camera
                };
                map.ease_to(camera, Duration::from_millis(700));
            });
            return;
        }
        if let Some(id) = self
            .bubble_at(tap.x, tap.y)
            .or_else(|| self.avatar_at(tap.x, tap.y))
        {
            self.select(id);
            return;
        }
        self.selected.set(None);
        self.following.set(false);
        self.refresh();
    }

    /// The face bubble under `(x, y)`, the nearest when they overlap.
    fn bubble_at(self, x: f64, y: f64) -> Option<usize> {
        let map = self.map_ref()?;
        let radius = f64::from(BUBBLE) / 2.0 + 4.0;
        self.bubbles()
            .into_iter()
            .filter_map(|id| {
                let (bx, by) = map.to_screen(self.place_of(id)?)?;
                let distance = (x - bx).hypot(y - (by - BUBBLE_LIFT));
                (distance < radius).then_some((distance, id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    /// The cluster under `(x, y)` and where its members stand.
    fn huddle_at(self, x: f64, y: f64) -> Option<(u64, LatLng)> {
        let map = self.map_ref()?;
        self.huddles()
            .into_iter()
            .filter_map(|h| {
                let middle = self.middle_of(&h.members)?;
                let (hx, hy) = map.to_screen(middle)?;
                let distance = (x - hx).hypot(y - (hy - BUBBLE_LIFT));
                (distance < f64::from(HUDDLE_WIDTH) / 2.0 + 4.0)
                    .then_some((distance, (h.cluster, middle)))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, hit)| hit)
    }

    /// The 3D avatar under `(x, y)`, within a finger's reach of their body.
    fn avatar_at(self, x: f64, y: f64) -> Option<usize> {
        let map = self.map_ref()?;
        self.avatars()
            .into_iter()
            .filter_map(|id| {
                let (fx, fy) = map.to_screen(self.place_of(id)?)?;
                let dy = if y < fy - AVATAR_PIXELS {
                    fy - AVATAR_PIXELS - y
                } else {
                    (y - fy).max(0.0)
                };
                let distance = (x - fx).hypot(dy);
                (distance < 28.0).then_some((distance, id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    /// Brings `id` into focus: their bubble steps out as the 3D avatar and the camera moves to them.
    fn select(self, id: usize) {
        self.selected.set(Some(id));
        self.following.set(false);
        if let (Some(map), Some(place)) = (self.map_ref(), self.place_of(id)) {
            let camera = self.camera.get_untracked();
            map.ease_to(
                MapCamera {
                    center: place,
                    zoom: camera.zoom.max(AVATAR_ZOOM),
                    pitch: camera.pitch.max(50.0),
                    ..camera
                },
                Duration::from_millis(600),
            );
        }
    }
}

fn point(geometry: &serde_json::Value) -> Option<LatLng> {
    let at = geometry.get("coordinates")?.as_array()?;
    Some(LatLng::new(at.get(1)?.as_f64()?, at.first()?.as_f64()?))
}

fn empty() -> serde_json::Value {
    serde_json::json!({"type": "FeatureCollection", "features": []})
}

fn cities_geojson() -> serde_json::Value {
    let features: Vec<serde_json::Value> = CITIES
        .iter()
        .enumerate()
        .map(|(i, city)| {
            serde_json::json!({
                "type": "Feature",
                "properties": {
                    "city": i,
                    "name": city.name,
                    "count": PEOPLE[i],
                    "color": css_color(city.accent),
                },
                "geometry": {"type": "Point", "coordinates": [city.center.lng, city.center.lat]},
            })
        })
        .collect();
    serde_json::json!({"type": "FeatureCollection", "features": features})
}

/// Adds the people and city layers on top of the style's own.
fn add_layers(map: MapRef, night: bool) {
    let text = if night { "#ece7ff" } else { "#2b2350" };
    let halo = if night { "#1d1a3f" } else { "#ffffff" };
    map.set_projection(MapProjection::Globe);
    map.add_source(
        "cities",
        serde_json::json!({"type": "geojson", "data": cities_geojson()}),
    );
    map.add_source(
        "people",
        serde_json::json!({
            "type": "geojson",
            "data": empty(),
            "cluster": true,
            "clusterRadius": 44,
            "clusterMaxZoom": CLUSTER_ZOOM as u8,
        }),
    );
    map.add_layer(
        serde_json::json!({
            "id": "people-clusters",
            "type": "circle",
            "source": "people",
            "filter": ["has", "point_count"],
            "paint": {"circle-opacity": 0},
        }),
        None,
    );
    map.add_layer(
        serde_json::json!({
            "id": "city-bubbles",
            "type": "circle",
            "source": "cities",
            "maxzoom": 9,
            "paint": {
                "circle-color": ["get", "color"],
                "circle-radius": ["interpolate", ["linear"], ["zoom"], 1, 15, 6, 26],
                "circle-stroke-width": 4,
                "circle-stroke-color": "#ffffff",
            },
        }),
        None,
    );
    map.add_layer(
        serde_json::json!({
            "id": "city-names",
            "type": "symbol",
            "source": "cities",
            "maxzoom": 9,
            "layout": {
                "text-field": ["get", "count"],
                "text-font": ["Noto Sans Bold"],
                "text-size": 13,
                "text-allow-overlap": true,
            },
            "paint": {"text-color": "#ffffff"},
        }),
        None,
    );
    map.add_layer(
        serde_json::json!({
            "id": "city-labels",
            "type": "symbol",
            "source": "cities",
            "maxzoom": 9,
            "layout": {
                "text-field": ["get", "name"],
                "text-font": ["Noto Sans Bold"],
                "text-size": 15,
                "text-offset": [0, 2.0],
                "text-allow-overlap": true,
            },
            "paint": {"text-color": text, "text-halo-color": halo, "text-halo-width": 2},
        }),
        None,
    );
}

fn ring(css: Css, width: f32, color: Color) -> Css {
    css.border_top_width(px(width))
        .border_right_width(px(width))
        .border_bottom_width(px(width))
        .border_left_width(px(width))
        .border_top_color(color)
        .border_right_color(color)
        .border_bottom_color(color)
        .border_left_color(color)
}

fn pill(background: Color) -> Css {
    Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .align_items(AlignItems::Center)
        .padding_left(px(16))
        .padding_right(px(16))
        .height(px(42))
        .border_radius(px(21))
        .background_color(background)
        .box_shadow(px(0), px(4), px(14), px(0), ink::shadow())
}

fn label(size: f32, weight: u16, color: Color) -> Css {
    Css::new()
        .font_size(px(size))
        .font_weight(FontWeight::Numeric(weight))
        .color(color)
}

#[component]
fn avatar(app: App, models: StoredValue<Vec<Model>>, id: usize) -> Element {
    let Some(look) = app
        .with_crowd(|c| c.walkers.get(id).map(|w| w.look))
        .flatten()
    else {
        return render! { Fragment {} };
    };
    let model = models.with(|m| m[look].clone());
    let placed = computed(move || {
        app.tick.get();
        app.with_crowd(|c| c.walkers.get(id).map(|w| (c.place(w), w.mood)))
            .flatten()
    });
    let position = computed(move || {
        placed.get().map_or(Vec3::ZERO, |((p, _), _)| {
            Vec3::new(p[0] as f32, 0.0, -p[1] as f32)
        })
    });
    let rotation = computed(move || {
        placed.get().map_or(Quat::IDENTITY, |((_, heading), _)| {
            Quat::from_rotation_y(std::f32::consts::PI - heading as f32)
        })
    });
    let born = app.clock.get_untracked();
    let scale = computed(move || {
        let camera = app.camera.get();
        let meters_per_pixel = 40_075_016.686 * camera.center.lat.to_radians().cos()
            / (512.0 * 2f64.powf(camera.zoom));
        let pop = back_out(((app.clock.get() - born) / POP_SECONDS).min(1.0));
        Vec3::splat((AVATAR_PIXELS * meters_per_pixel * pop) as f32 / MODEL_HEIGHT)
    });
    let animation = computed(move || {
        let clip = match placed.get().map(|(_, mood)| mood) {
            Some(Mood::Waving(_)) => "wave",
            Some(Mood::Resting(_)) => "idle",
            _ => "walk",
        };
        Some(clip.to_string())
    });
    let options = AnimationOptions {
        fade: 0.3,
        looping: true,
        speed: 1.0,
    };
    render! {
        ModelView(
            model: model,
            position: position,
            rotation: rotation,
            scale: scale,
            animation: animation,
            animation_options: options,
            cast_shadow: false,
            receive_shadow: false,
            on_tap: move |_: SceneHit| app.select(id),
        )
    }
}

/// Eases from 0 to 1, overshooting a little before it settles.
fn back_out(t: f64) -> f64 {
    let (c1, t) = (1.70158, t - 1.0);
    1.0 + (c1 + 1.0) * t * t * t + c1 * t * t
}

#[derive(Clone, Copy, PartialEq)]
enum Pose {
    Walking,
    Waving,
    Resting,
}

/// A person's face in a round bubble pointing at where they stand.
#[component]
fn person_marker(app: App, id: usize) -> Element {
    let Some(look) = app
        .with_crowd(|c| c.walkers.get(id).map(|w| w.look))
        .flatten()
    else {
        return render! { Fragment {} };
    };
    let position = computed(move || {
        app.tick.get();
        app.place_of(id).unwrap_or(LatLng::new(0.0, 0.0))
    });
    let pose = computed(move || {
        app.pulse.get();
        app.with_crowd(|c| match c.walkers.get(id).map(|w| w.mood) {
            Some(Mood::Waving(_)) => Pose::Waving,
            Some(Mood::Resting(_)) => Pose::Resting,
            _ => Pose::Walking,
        })
        .unwrap_or(Pose::Walking)
    });
    render! {
        Marker(position: position) { FaceBubble(look: look, id: id, pose: pose) }
    }
}

fn keyframes(name: &str, from: Css, to: Css) -> Keyframes {
    Keyframes::builder()
        .named(name)
        .from(from)
        .to(to)
        .build()
        .expect("two frames")
}

fn scale(s: f32) -> Css {
    Css::new().transform([TransformFn::Scale(s.into(), s.into())])
}

fn loop_forever(keyframes: Keyframes, millis: f32, delay: f32) -> Animation {
    Animation::new(keyframes)
        .duration(ms(millis))
        .delay(ms(-delay))
        .timing(EasingFunction::EaseInOut)
        .iteration_count(AnimationIterationCount::Infinite)
        .direction(AnimationDirection::Alternate)
}

#[component]
fn face_bubble(look: usize, id: usize, pose: ReadSignal<Pose>) -> Element {
    let rgb = LOOKS[look].color;
    let color = Color::hex(rgb);
    let glow = Color::rgba((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0.55);
    // Spread the loops out so a crowd does not bob in step.
    let phase = (id * 7 % 13) as f32 * 130.0;
    let bottom = Position::Coords(percent(50).into(), percent(100).into());
    let pop = Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .align_items(AlignItems::Center)
        .transform_origin(bottom.clone())
        .animation(
            Animation::new(keyframes(
                "pop",
                scale(0.0).opacity(0.0),
                scale(1.0).opacity(1.0),
            ))
            .duration(ms(520.0))
            .delay(ms(phase / 20.0))
            .timing(EasingFunction::CubicBezier(0.34, 1.8, 0.64, 1.0))
            .fill_mode(AnimationFillMode::Both),
        );
    let moving = computed(move || {
        let base = Css::new()
            .display_flex()
            .flex_direction(FlexDirection::Column)
            .align_items(AlignItems::Center)
            .transform_origin(bottom.clone());
        match pose.get() {
            Pose::Walking => base.animation(loop_forever(
                keyframes(
                    "bob",
                    Css::new().transform([TransformFn::TranslateY(px(0).into())]),
                    Css::new().transform([TransformFn::TranslateY(px(-6).into())]),
                ),
                420.0,
                phase,
            )),
            Pose::Waving => base.animation(loop_forever(
                keyframes(
                    "wiggle",
                    Css::new().transform([TransformFn::Rotate((-14).deg().into())]),
                    Css::new().transform([TransformFn::Rotate(14.deg().into())]),
                ),
                140.0,
                0.0,
            )),
            Pose::Resting => base.animation(loop_forever(
                keyframes("breathe", scale(1.0), scale(0.92)),
                900.0,
                phase,
            )),
        }
    });
    let head = Css::new()
        .width(px(BUBBLE))
        .height(px(BUBBLE))
        .position(PositionKind::Relative)
        .z_index(1);
    let radar = Css::new()
        .position(PositionKind::Absolute)
        .left(px(0))
        .top(px(0))
        .width(px(BUBBLE))
        .height(px(BUBBLE))
        .border_radius(px(BUBBLE / 2.0))
        .background_color(glow)
        .animation(
            Animation::new(keyframes(
                "radar",
                scale(1.0).opacity(0.9),
                scale(1.9).opacity(0.0),
            ))
            .duration(ms(1600.0))
            .delay(ms(-phase))
            .timing(EasingFunction::EaseOut)
            .iteration_count(AnimationIterationCount::Infinite),
        );
    let face = Css::new()
        .position(PositionKind::Absolute)
        .left(px(0))
        .top(px(0))
        .width(px(BUBBLE))
        .height(px(BUBBLE))
        .border_radius(px(BUBBLE / 2.0))
        .padding(px(4))
        .background_color(color)
        .box_shadow(px(0), px(4), px(16), px(2), glow);
    let photo = ring(
        Css::new()
            .width(percent(100))
            .height(percent(100))
            .border_radius(px(BUBBLE / 2.0 - 4.0))
            .overflow(Overflow::Hidden)
            .background_color(Color::hex(0xfff6e8)),
        3.0,
        ink::white(),
    );
    let image = Css::new().width(percent(100)).height(percent(100));
    let badge = move |text_color: Color| {
        ring(
            Css::new()
                .position(PositionKind::Absolute)
                .right(px(-14))
                .top(px(-6))
                .padding_left(px(6))
                .padding_right(px(6))
                .height(px(20))
                .border_radius(px(10))
                .display_flex()
                .align_items(AlignItems::Center)
                .background_color(text_color),
            2.0,
            ink::white(),
        )
        .transform_origin(Position::Coords(percent(0).into(), percent(100).into()))
        .animation(
            Animation::new(keyframes("badge", scale(0.0), scale(1.0)))
                .duration(ms(360.0))
                .timing(EasingFunction::CubicBezier(0.34, 1.8, 0.64, 1.0))
                .fill_mode(AnimationFillMode::Both),
        )
    };
    let tail = Css::new()
        .width(px(12))
        .height(px(12))
        .margin_top(px(-8))
        .background_color(color)
        .transform([TransformFn::Rotate(45.deg().into())]);
    let spot = ring(
        Css::new()
            .width(px(10))
            .height(px(10))
            .border_radius(px(5))
            .margin_top(px(4))
            .background_color(color),
        2.0,
        ink::white(),
    );
    render! {
        View(style: pop) {
            View(style: moving) {
                View(style: head) {
                    View(style: radar)
                    View(style: face) {
                        View(style: photo) {
                            Image(src: whisker_asset::resolve(LOOKS[look].face), mode: ImageMode::AspectFill, style: image)
                        }
                    }
                    Show(when: move || pose.get() == Pose::Waving) {
                        View(style: badge(ink::pink())) {
                            Text(style: label(11.0, 900, ink::white()), value: "hi!")
                        }
                    }
                    Show(when: move || pose.get() == Pose::Resting) {
                        View(style: badge(ink::ink())) {
                            Text(style: label(11.0, 900, ink::white()), value: "zzz")
                        }
                    }
                }
                View(style: tail)
            }
            View(style: spot)
        }
    }
}

/// The selected person's name, floating over their head.
/// A cluster as two of its faces in a candy capsule, with how many are in it.
#[component]
fn huddle_marker(app: App, h: ReadSignal<Huddle>) -> Element {
    let position = computed(move || {
        app.tick.get();
        h.with(|h| app.middle_of(&h.members))
            .unwrap_or(LatLng::new(0.0, 0.0))
    });
    let looks = computed(move || {
        let (first, second) = h.with(|h| h.faces);
        app.with_crowd(|c| (c.walkers[first].look, second.map(|id| c.walkers[id].look)))
            .unwrap_or((0, None))
    });
    let count = computed(move || h.with(|h| h.count.to_string()));
    let phase = (h.with_untracked(|h| h.faces.0) * 7 % 13) as f32 * 130.0;
    render! {
        Marker(position: position) { HuddleCapsule(looks: looks, count: count, phase: phase) }
    }
}

#[component]
fn huddle_capsule(
    looks: ReadSignal<(usize, Option<usize>)>,
    count: ReadSignal<String>,
    phase: f32,
) -> Element {
    const HEIGHT: f32 = 60.0;
    const FACE: f32 = 46.0;
    let pink = ink::pink();
    let glow = Color::rgba(255, 61, 127, 0.5);
    let bottom = Position::Coords(percent(50).into(), percent(100).into());
    let pop = Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .align_items(AlignItems::Center)
        .transform_origin(bottom.clone())
        .animation(
            Animation::new(keyframes(
                "pop",
                scale(0.0).opacity(0.0),
                scale(1.0).opacity(1.0),
            ))
            .duration(ms(560.0))
            .timing(EasingFunction::CubicBezier(0.34, 1.8, 0.64, 1.0))
            .fill_mode(AnimationFillMode::Both),
        );
    let moving = Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .align_items(AlignItems::Center)
        .transform_origin(bottom)
        .animation(loop_forever(
            keyframes("squish", scale(1.0), scale(1.06)),
            700.0,
            phase,
        ));
    let head = Css::new()
        .width(px(HUDDLE_WIDTH))
        .height(px(HEIGHT))
        .position(PositionKind::Relative)
        .z_index(1);
    let capsule = |extra: Css| {
        extra
            .position(PositionKind::Absolute)
            .left(px(0))
            .top(px(0))
            .width(px(HUDDLE_WIDTH))
            .height(px(HEIGHT))
            .border_radius(px(HEIGHT / 2.0))
    };
    let body = ring(
        capsule(Css::new())
            .background_color(pink)
            .box_shadow(px(0), px(6), px(18), px(2), glow),
        3.0,
        ink::white(),
    );
    let face = move |which: usize| {
        let left = if which == 0 {
            4.0
        } else {
            HUDDLE_WIDTH - FACE - 10.0
        };
        move || {
            let look = match which {
                0 => Some(looks.get().0),
                _ => looks.get().1,
            };
            let color = Color::hex(look.map_or(0xffffff, |l| LOOKS[l].color));
            ring(
                Css::new()
                    .position(PositionKind::Absolute)
                    .left(px(left))
                    .top(px((HEIGHT - 6.0 - FACE) / 2.0))
                    .width(px(FACE))
                    .height(px(FACE))
                    .border_radius(px(FACE / 2.0))
                    .overflow(Overflow::Hidden)
                    .background_color(Color::hex(0xfff6e8))
                    .opacity(if look.is_some() { 1.0 } else { 0.0 }),
                3.0,
                color,
            )
        }
    };
    let src = move |which: usize| {
        move || {
            let look = match which {
                0 => looks.get().0,
                _ => looks.get().1.unwrap_or(looks.get().0),
            };
            whisker_asset::resolve(LOOKS[look].face)
        }
    };
    let image = || Css::new().width(percent(100)).height(percent(100));
    let badge = ring(
        Css::new()
            .position(PositionKind::Absolute)
            .right(px(-10))
            .top(px(-12))
            .min_width(px(28))
            .height(px(28))
            .padding_left(px(7))
            .padding_right(px(7))
            .border_radius(px(14))
            .display_flex()
            .align_items(AlignItems::Center)
            .justify_content(JustifyContent::Center)
            .background_color(Color::hex(0xffd23f)),
        3.0,
        ink::white(),
    )
    .animation(loop_forever(
        keyframes("beat", scale(1.0), scale(1.18)),
        480.0,
        phase,
    ));
    let confetti = |size: f32, left: f32, top: f32, color: u32| {
        Css::new()
            .position(PositionKind::Absolute)
            .left(px(left))
            .top(px(top))
            .width(px(size))
            .height(px(size))
            .border_radius(px(size / 2.0))
            .background_color(Color::hex(color))
    };
    let tail = Css::new()
        .width(px(14))
        .height(px(14))
        .margin_top(px(-9))
        .background_color(pink)
        .transform([TransformFn::Rotate(45.deg().into())]);
    let spot = ring(
        Css::new()
            .width(px(10))
            .height(px(10))
            .border_radius(px(5))
            .margin_top(px(4))
            .background_color(pink),
        2.0,
        ink::white(),
    );
    render! {
        View(style: pop) {
            View(style: moving) {
                View(style: head) {
                    View(style: confetti(9.0, -10.0, 4.0, 0x2ed3b7))
                    View(style: confetti(7.0, 18.0, -12.0, 0xffd23f))
                    View(style: confetti(6.0, -4.0, 44.0, 0xa35bff))
                    View(style: body) {
                        View(style: computed(face(1))) {
                            Image(src: computed(src(1)), mode: ImageMode::AspectFill, style: image())
                        }
                        View(style: computed(face(0))) {
                            Image(src: computed(src(0)), mode: ImageMode::AspectFill, style: image())
                        }
                    }
                    View(style: badge) {
                        Text(style: label(13.0, 900, ink::ink()), value: count)
                    }
                }
                View(style: tail)
            }
            View(style: spot)
        }
    }
}

#[component]
fn name_tag(app: App) -> Element {
    let place = computed(move || {
        app.tick.get();
        let id = app.selected.get()?;
        app.with_crowd(|c| {
            let walker = c.walkers.get(id)?;
            Some((
                people::to_latlng(c.center, c.place(walker).0),
                walker.name,
                walker.look,
            ))
        })
        .flatten()
    });
    let position = computed(move || place.get().map_or(LatLng::new(0.0, 0.0), |p| p.0));
    let name = computed(move || place.get().map(|p| p.1.to_string()).unwrap_or_default());
    let lifted = computed(move || {
        let color = place.get().map_or(0xff3d7f, |p| LOOKS[p.2].color);
        ring(pill(Color::hex(color)), 3.0, ink::white())
            .height(px(34))
            .padding_left(px(12))
            .padding_right(px(12))
            .margin_bottom(px(AVATAR_PIXELS as f32 + 6.0))
    });
    render! {
        Marker(position: position) {
            View(style: lifted) {
                Text(style: label(14.0, 900, ink::white()), value: name)
            }
        }
    }
}

/// The selected person's card.
#[component]
fn card(app: App) -> Element {
    let info = computed(move || {
        app.pulse.get();
        let id = app.selected.get()?;
        app.with_crowd(|c| {
            let w = c.walkers.get(id)?;
            let street = c.street(w);
            let doing = match w.mood {
                Mood::Waving(_) => "waving at you".to_string(),
                Mood::Resting(_) => "taking a break".to_string(),
                Mood::Walking if street.is_empty() => "out for a walk".to_string(),
                Mood::Walking => format!("walking on {street}"),
            };
            let distance = if w.walked >= 1000.0 {
                format!("{:.1} km today", w.walked / 1000.0)
            } else {
                format!("{:.0} m today", w.walked)
            };
            Some((w.name.to_string(), w.look, doing, distance))
        })
        .flatten()
    });
    let name = computed(move || info.get().map(|i| i.0).unwrap_or_default());
    let face_src =
        computed(move || whisker_asset::resolve(LOOKS[info.get().map_or(0, |i| i.1)].face));
    let doing = computed(move || info.get().map(|i| i.2).unwrap_or_default());
    let distance = computed(move || info.get().map(|i| i.3).unwrap_or_default());
    let color = move || Color::hex(info.get().map_or(0xff3d7f, |i| LOOKS[i.1].color));
    let card = Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .align_items(AlignItems::Center)
        .padding(px(16))
        .margin_bottom(px(12))
        .border_radius(px(28))
        .background_color(ink::white())
        .box_shadow(px(0), px(8), px(24), px(0), ink::shadow());
    let face = move || {
        ring(
            Css::new()
                .width(px(56))
                .height(px(56))
                .border_radius(px(28))
                .overflow(Overflow::Hidden)
                .background_color(color()),
            4.0,
            ink::white(),
        )
        .box_shadow(px(0), px(3), px(8), px(0), ink::shadow())
    };
    let body = Css::new()
        .flex_grow(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .margin_left(px(14));
    let button = move |filled: bool| {
        let css = pill(if filled {
            ink::pink()
        } else {
            Color::hex(0xf1edff)
        })
        .height(px(38))
        .flex_shrink(0.0)
        .margin_left(px(8))
        .padding_left(px(14))
        .padding_right(px(14));
        css.box_shadow(px(0), px(0), px(0), px(0), Color::rgba(0, 0, 0, 0.0))
    };
    render! {
        View(style: card) {
            View(style: computed(face)) {
                Image(src: face_src, mode: ImageMode::AspectFill, style: Css::new().width(percent(100)).height(percent(100)))
            }
            View(style: body) {
                Text(style: label(20.0, 800, ink::ink()), value: name)
                Text(style: label(14.0, 600, ink::soft()), value: doing)
                Text(style: label(13.0, 700, ink::pink()), value: distance)
            }
            View(
                style: computed(move || button(app.following.get())),
                on_tap: move |_| app.following.set(!app.following.get_untracked()),
            ) {
                Text(
                    style: computed(move || label(14.0, 800, if app.following.get() { ink::white() } else { ink::ink() })),
                    value: computed(move || if app.following.get() { "Following" } else { "Follow" }.to_string()),
                )
            }
            View(
                style: button(false),
                on_tap: move |_| {
                    if let Some(id) = app.selected.get_untracked() {
                        app.crowd.with(|c| {
                            if let Some(crowd) = c.borrow_mut().as_mut() {
                                crowd.wave(id);
                            }
                        });
                        app.pulse.update(|p| *p += 1);
                    }
                },
            ) {
                Text(style: label(14.0, 800, ink::ink()), value: "Wave")
            }
        }
    }
}

#[component]
fn chips(app: App) -> Element {
    let row = Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .min_width(percent(100))
        .padding_top(px(4))
        .padding_bottom(px(14));
    // Unlike `justify-content: center`, auto margins never push the first chip past the scroll start.
    let group = Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .flex_shrink(0.0)
        .margin_left(MarginValue::Auto)
        .margin_right(MarginValue::Auto);
    let chip = move |index: Option<usize>| {
        let active = match index {
            Some(i) => app.city.get() == Some(i) && !app.intro.get(),
            None => app.intro.get(),
        };
        let accent = index.map_or(ink::ink(), |i| Color::hex(CITIES[i].accent));
        pill(if active { accent } else { ink::white() })
            .flex_shrink(0.0)
            .margin_left(px(4))
            .margin_right(px(4))
            .padding_left(px(14))
            .padding_right(px(14))
    };
    let text = move |index: Option<usize>| {
        let active = match index {
            Some(i) => app.city.get() == Some(i) && !app.intro.get(),
            None => app.intro.get(),
        };
        label(14.0, 800, if active { ink::white() } else { ink::ink() })
    };
    let city_chip = move |i: usize| {
        render! {
            View(style: computed(move || chip(Some(i))), on_tap: move |_| app.go(i)) {
                Text(style: computed(move || text(Some(i))), value: CITIES[i].name)
            }
        }
    };
    render! {
        ScrollView(axis: ScrollAxis::Horizontal, style: Css::new().height(px(60)).flex_shrink(0.0)) {
            View(style: row) {
                View(style: group) {
                    View(
                        style: computed(move || chip(None)),
                        on_tap: move |_| {
                            app.intro.set(true);
                            app.globe();
                        },
                    ) {
                        Text(style: computed(move || text(None)), value: "Globe")
                    }
                    { city_chip(0) }
                    { city_chip(1) }
                    { city_chip(2) }
                    { city_chip(3) }
                    { city_chip(4) }
                }
            }
        }
    }
}

/// The map in one style, with the 3D people inside it.
#[component]
fn city_map(app: App, night: bool, scene: Scene) -> Element {
    let map_ref = MapRef::new();
    add_layers(map_ref, night);
    app.map.with(|m| m.set(Some(map_ref)));
    app.refresh();
    let layer = scene.layer();
    // A layer past the globe's horizon is not drawn, and the scene's frames, which move the
    // crowd and turn the globe, stop with it.
    let anchor = computed(move || {
        let camera = app.camera.get();
        match app.city.get() {
            Some(i) if camera.zoom >= 10.0 => CITIES[i].center,
            _ => camera.center,
        }
    });
    let style = if night { NIGHT_STYLE } else { DAY_STYLE };
    let fill = Css::new().flex_grow(1.0);
    render! {
        MapView(
            map_style: MapStyle::Json(style.to_string()),
            camera: app.camera,
            style: fill,
            on_tap: move |tap: MapTap| app.tap(tap),
            on_idle: move || app.populate(),
            map_ref: map_ref,
        ) {
            MapLayer(layer: layer.clone(), anchor: anchor)
            ForEach(
                each: move || app.bubbles(),
                key: |id: &usize| *id,
                children: move |id: ReadSignal<usize>| render! { PersonMarker(app: app, id: id.get_untracked()) },
            )
            ForEach(
                each: move || app.huddles(),
                key: |h: &Huddle| h.faces.0,
                children: move |h: ReadSignal<Huddle>| render! { HuddleMarker(app: app, h: h) },
            )
            Show(when: move || app.selected.get().is_some()) { NameTag(app: app) }
        }
    }
}

#[whisker::main]
pub fn app() -> Element {
    let camera = signal(MapCamera {
        center: LatLng::new(30.0, 100.0),
        zoom: 1.4,
        ..Default::default()
    });
    let app = App {
        camera,
        city: signal(None),
        intro: signal(true),
        night: signal(false),
        selected: signal(None),
        following: signal(false),
        tick: signal(0),
        pulse: signal(0),
        shown: signal(Vec::new()),
        huddles: signal(Vec::new()),
        close: computed(move || camera.get().zoom >= AVATAR_ZOOM),
        clock: signal(0.0),
        crowd: StoredValue::new(RefCell::new(None)),
        map: StoredValue::new(Cell::new(None)),
    };
    let models = StoredValue::new(
        LOOKS
            .iter()
            .map(|look| Model::from_slice(look.model).expect("bundled avatar"))
            .collect::<Vec<_>>(),
    );
    let scene = Scene::new();
    let last = Cell::new(None::<Duration>);
    let since_refresh = Cell::new(0.0f32);
    let frames = StoredValue::new(scene.on_frame(move |frame| {
        let dt = last
            .replace(Some(frame.timestamp))
            .map_or(0.0, |previous| {
                frame.timestamp.saturating_sub(previous).as_secs_f32()
            })
            .min(0.1);
        app.step(dt, &since_refresh);
    }));
    let insets = safe_area_insets();

    let page = Css::new()
        .flex_grow(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .position(PositionKind::Relative)
        .background_color(Color::hex(0xf7f1e3));
    let top = move || {
        Css::new()
            .position(PositionKind::Absolute)
            .left(px(16))
            .right(px(16))
            .top(px(insets.get().top as f32 + 12.0))
            .display_flex()
            .flex_direction(FlexDirection::Row)
            .justify_content(JustifyContent::SpaceBetween)
    };
    let bottom = move || {
        Css::new()
            .position(PositionKind::Absolute)
            .left(px(12))
            .right(px(12))
            .bottom(px(insets.get().bottom as f32 + 2.0))
            .display_flex()
            .flex_direction(FlexDirection::Column)
    };
    let dot = Css::new()
        .width(px(10))
        .height(px(10))
        .border_radius(px(5))
        .margin_right(px(8))
        .background_color(ink::pink());
    let status = computed(move || {
        app.pulse.get();
        match app.city.get() {
            Some(i) if !app.intro.get() => {
                let count = app.with_crowd(|c| c.walkers.len()).unwrap_or(0);
                if count == 0 {
                    format!("{} · finding people…", CITIES[i].name)
                } else {
                    format!("{} · {count} out now", CITIES[i].name)
                }
            }
            _ => format!("{} people out now", PEOPLE.iter().sum::<usize>()),
        }
    });
    let round = pill(ink::white())
        .width(px(42))
        .padding_left(px(0))
        .padding_right(px(0))
        .justify_content(JustifyContent::Center);
    // Drawn rather than typed, since no iOS font has a glyph for ☾.
    let moon = || {
        Css::new()
            .position(PositionKind::Relative)
            .width(px(18))
            .height(px(18))
            .border_radius(px(9))
            .background_color(ink::ink())
    };
    let moon_cut = || {
        Css::new()
            .position(PositionKind::Absolute)
            .left(px(6))
            .top(px(-3))
            .width(px(16))
            .height(px(16))
            .border_radius(px(8))
            .background_color(ink::white())
    };
    let sun = || {
        ring(
            Css::new()
                .width(px(18))
                .height(px(18))
                .border_radius(px(9))
                .background_color(Color::hex(0xffb300)),
            4.0,
            Color::hex(0xffe08a),
        )
    };
    let _keep = frames;
    let (day_scene, night_scene) = (scene.clone(), scene.clone());
    render! {
        View(style: page) {
            Show(when: move || !app.night.get()) { CityMap(app: app, night: false, scene: day_scene.clone()) }
            Show(when: move || app.night.get()) { CityMap(app: app, night: true, scene: night_scene.clone()) }
            SceneContent(scene: scene.clone()) {
                HemisphereLight(sky_color: Color::hex(0xfff6e8), ground_color: Color::hex(0x8f86c9), intensity: 1.4)
                DirectionalLight(color: Color::hex(0xffffff), intensity: 2.2, direction: Vec3::new(-0.4, -1.0, -0.5))
                ForEach(
                    each: move || app.avatars(),
                    key: |id: &usize| *id,
                    children: move |id: ReadSignal<usize>| render! { Avatar(app: app, models: models, id: id.get_untracked()) },
                )
            }
            View(style: computed(top)) {
                View(style: pill(ink::white())) {
                    View(style: dot)
                    Text(style: label(13.0, 900, ink::pink()), value: "LIVE  ")
                    Text(style: label(14.0, 800, ink::ink()), value: status)
                }
                View(style: round, on_tap: move |_| app.night.set(!app.night.get_untracked())) {
                    Show(when: move || app.night.get()) { View(style: sun()) }
                    Show(when: move || !app.night.get()) {
                        View(style: moon()) { View(style: moon_cut()) }
                    }
                }
            }
            View(style: computed(bottom)) {
                Show(when: move || app.selected.get().is_some()) { Card(app: app) }
                Chips(app: app)
            }
        }
    }
}
