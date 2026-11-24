//! The state one map shares between its component, gestures, and renderer.

use glam::DVec2;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use whisker::RwSignal;
use whisker_surface::{FrameInfo, SurfaceSize, WeakSurface};
use whisker_wgpu::wgpu;

use crate::animation::{Animation, clamp_center, normalize_bearing};
use crate::camera::{MAX_PITCH, MAX_ZOOM, MIN_ZOOM, MapCamera, View, min_zoom};
use crate::geo::LatLng;
use crate::query::MapTap;
use crate::render::{PlacedLabel, TileSet};
use crate::sprite::SpriteStore;
use crate::style::{GeoJsonOptions, Layer, Light, MapProjection, MapStyle, SourceKind, Style};
use crate::text::GlyphStore;
use crate::tiles::geojson::GeoJsonIndex;
use crate::tiles::pmtiles::Archive;
use crate::tiles::{TileSource, log};

/// A loaded style and the tile sources it reads.
pub(crate) struct StyleState {
    pub sources: HashMap<String, Rc<TileSource>>,
    pub layers: Arc<Vec<Arc<Layer>>>,
    pub light: Light,
    pub glyphs: Option<Rc<GlyphStore>>,
    pub sprite: Vec<(String, String)>,
    pub sprites: Rc<SpriteStore>,
    pub projection: MapProjection,
    /// Asks the map for another frame once something it waits on has arrived.
    pub on_change: Rc<dyn Fn()>,
}

/// A marker's place and where the last frame put it on screen.
pub(crate) struct MarkerSlot {
    pub position: Cell<LatLng>,
    pub screen: RwSignal<Option<(f32, f32)>>,
}

pub(crate) struct MapInner {
    camera: Cell<MapCamera>,
    camera_signal: Cell<Option<RwSignal<MapCamera>>>,
    size: Cell<SurfaceSize>,
    style: RefCell<Option<Rc<StyleState>>>,
    style_generation: Cell<u64>,
    surface: RefCell<Option<WeakSurface>>,
    animation: RefCell<Option<Animation>>,
    markers: RefCell<Vec<Weak<MarkerSlot>>>,
    /// The loaded style document and the URL it came from, which `add_source` and `add_layer` edit.
    document: RefCell<Option<(serde_json::Value, Option<String>)>>,
    /// Edits the app made before the style document loaded.
    pending_edits: RefCell<Vec<StyleEdit>>,
    /// GeoJSON data the app set before its source existed, by source id.
    pending_geojson: RefCell<HashMap<String, serde_json::Value>>,
    /// The tiles and labels of the last frame drawn.
    drawn: RefCell<(TileSet, Vec<PlacedLabel>)>,
    /// Whether the next frame should be requested even when nothing else asks for one.
    busy: Cell<bool>,
    /// The app's projection, which wins over the style's.
    projection: Cell<Option<MapProjection>>,
}

impl MapInner {
    pub fn new(camera: MapCamera) -> Rc<MapInner> {
        Rc::new(MapInner {
            camera: Cell::new(clamp_camera(camera, MIN_ZOOM)),
            camera_signal: Cell::new(None),
            size: Cell::new(SurfaceSize {
                width: 1,
                height: 1,
                scale: 1.0,
            }),
            style: RefCell::new(None),
            style_generation: Cell::new(0),
            surface: RefCell::new(None),
            animation: RefCell::new(None),
            markers: RefCell::new(Vec::new()),
            document: RefCell::new(None),
            pending_edits: RefCell::new(Vec::new()),
            pending_geojson: RefCell::new(HashMap::new()),
            drawn: RefCell::new((HashMap::new(), Vec::new())),
            busy: Cell::new(false),
            projection: Cell::new(None),
        })
    }

    /// Applies `edit` to the style, now or once the style has loaded.
    pub fn edit_style(self: &Rc<Self>, edit: StyleEdit) {
        let mut document = self.document.borrow_mut();
        let Some((json, base)) = document.as_mut() else {
            self.pending_edits.borrow_mut().push(edit);
            return;
        };
        edit.apply(json);
        let style = Style::from_json(json);
        let base = base.clone();
        drop(document);
        self.install_style(style, base.as_deref());
    }

    /// Replaces a GeoJSON source's data; other sources are left alone.
    pub fn set_geojson(&self, source: &str, data: serde_json::Value) {
        let tile_source = self
            .style()
            .and_then(|style| style.sources.get(source).cloned());
        let Some(tile_source) = tile_source else {
            self.pending_geojson
                .borrow_mut()
                .insert(source.to_string(), data);
            return;
        };
        let options = match &tile_source.source.borrow().kind {
            SourceKind::GeoJson { options, .. } => options.clone(),
            _ => return,
        };
        load_geojson(&tile_source, data, options);
    }

    pub fn publish_frame(&self, tiles: TileSet, labels: Vec<PlacedLabel>) {
        *self.drawn.borrow_mut() = (tiles, labels);
    }

    /// What a tap at `point`, in logical pixels, lands on.
    pub fn tap_at(&self, point: DVec2) -> MapTap {
        let view = self.view();
        let features = match self.style() {
            Some(style) => {
                let drawn = self.drawn.borrow();
                crate::query::features_at(&view, &style, &drawn.0, &drawn.1, point)
            }
            None => Vec::new(),
        };
        MapTap {
            position: view.unproject(point),
            x: point.x,
            y: point.y,
            features,
        }
    }

    pub fn set_surface(&self, surface: WeakSurface) {
        *self.surface.borrow_mut() = Some(surface);
    }

    pub fn bind_camera(&self, signal: RwSignal<MapCamera>) {
        self.camera_signal.set(Some(signal));
    }

    pub fn request_frame(&self) {
        if let Some(surface) = self
            .surface
            .borrow()
            .as_ref()
            .and_then(WeakSurface::upgrade)
        {
            surface.request_frame();
        }
    }

    pub fn camera(&self) -> MapCamera {
        self.camera.get()
    }

    /// The viewport in logical pixels.
    pub fn logical_size(&self) -> (f64, f64) {
        let size = self.size.get();
        let scale = f64::from(size.scale.max(0.01));
        (
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        )
    }

    /// Physical pixels per logical pixel.
    pub fn logical_scale(&self) -> f32 {
        self.size.get().scale
    }

    pub fn view(&self) -> View {
        let (width, height) = self.logical_size();
        View::projected(self.camera.get(), width, height, self.projection())
    }

    pub fn projection(&self) -> MapProjection {
        self.projection.get().unwrap_or_else(|| {
            self.style()
                .map_or(MapProjection::Mercator, |style| style.projection)
        })
    }

    pub fn set_projection(&self, projection: MapProjection) {
        self.projection.set(Some(projection));
        self.move_camera(self.camera.get());
        self.request_frame();
    }

    /// Takes a camera the app wrote, stopping any motion the map was playing.
    pub fn camera_from_app(&self, camera: MapCamera) {
        let camera = clamp_camera(camera, min_zoom(self.projection()));
        if camera == self.camera.get() {
            return;
        }
        self.animation.borrow_mut().take();
        self.camera.set(camera);
        self.request_frame();
    }

    /// Moves the camera for a gesture or motion and tells the app.
    pub fn move_camera(&self, camera: MapCamera) {
        let camera = clamp_camera(camera, min_zoom(self.projection()));
        if camera == self.camera.get() {
            return;
        }
        self.camera.set(camera);
        if let Some(signal) = self.camera_signal.get()
            && signal.get_untracked() != camera
        {
            signal.set(camera);
        }
        self.request_frame();
    }

    pub fn animate(&self, animation: Animation) {
        *self.animation.borrow_mut() = Some(animation);
        self.request_frame();
    }

    pub fn stop_animation(&self) {
        self.animation.borrow_mut().take();
    }

    #[cfg(test)]
    pub fn is_animating(&self) -> bool {
        self.animation.borrow().is_some()
    }

    pub fn set_size(&self, size: SurfaceSize) {
        self.size.set(size);
    }

    pub fn style(&self) -> Option<Rc<StyleState>> {
        self.style.borrow().clone()
    }

    pub fn clear_color(&self) -> wgpu::Color {
        wgpu::Color::TRANSPARENT
    }

    pub fn add_marker(&self, slot: &Rc<MarkerSlot>) {
        let mut markers = self.markers.borrow_mut();
        markers.retain(|m| m.strong_count() > 0);
        markers.push(Rc::downgrade(slot));
        drop(markers);
        self.request_frame();
    }

    /// Advances any motion and resolves the camera for the frame about to draw.
    pub fn begin_frame(&self, frame: &FrameInfo, size: SurfaceSize) -> View {
        self.size.set(size);
        let sampled = {
            let view = self.view();
            self.animation
                .borrow_mut()
                .as_mut()
                .map(|animation| animation.sample(frame.timestamp, &view))
        };
        if let Some((camera, done)) = sampled {
            if done {
                self.animation.borrow_mut().take();
            }
            self.move_camera(camera);
        }
        self.busy.set(false);
        self.view()
    }

    /// Asks for another frame while work remains, and moves markers to where this frame drew.
    pub fn end_frame(&self, view: &View) {
        let markers: Vec<Rc<MarkerSlot>> = self
            .markers
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for marker in markers {
            let screen = view
                .project(marker.position.get())
                .filter(|p| p.x.is_finite() && p.y.is_finite())
                .map(|p| (p.x as f32, p.y as f32));
            let changed = match (marker.screen.get_untracked(), screen) {
                (Some(a), Some(b)) => (a.0 - b.0).abs() > 0.01 || (a.1 - b.1).abs() > 0.01,
                (a, b) => a.is_some() != b.is_some(),
            };
            if changed {
                marker.screen.set(screen);
            }
        }
        if self.animation.borrow().is_some() || self.busy.get() {
            self.request_frame();
        }
    }

    /// Asks the frame being drawn to be followed by another.
    pub fn keep_drawing(&self) {
        self.busy.set(true);
    }

    pub fn load_style(self: &Rc<Self>, style: MapStyle) {
        let generation = self.style_generation.get() + 1;
        self.style_generation.set(generation);
        let weak = Rc::downgrade(self);
        whisker::spawn_local(async move {
            let (text, base) = match style {
                MapStyle::Json(text) => (text, None),
                MapStyle::Url(url) => match crate::net::fetch(url.clone(), None).await {
                    Ok(Some(bytes)) => (String::from_utf8_lossy(&bytes).into_owned(), Some(url)),
                    Ok(None) => {
                        log(&format!("whisker-map: style {url} not found"));
                        return;
                    }
                    Err(error) => {
                        log(&format!("whisker-map: style {url}: {error}"));
                        return;
                    }
                },
            };
            let mut document: serde_json::Value = match serde_json::from_str(&text) {
                Ok(document) => document,
                Err(error) => {
                    log(&format!("whisker-map: style JSON: {error}"));
                    return;
                }
            };
            let Some(map) = weak.upgrade() else { return };
            if map.style_generation.get() != generation {
                return;
            }
            for edit in map.pending_edits.borrow_mut().drain(..) {
                edit.apply(&mut document);
            }
            map.install_style(Style::from_json(&document), base.as_deref());
            *map.document.borrow_mut() = Some((document, base));
        });
    }

    fn install_style(self: &Rc<Self>, style: Style, base: Option<&str>) {
        let layers = Arc::new(style.layers);
        let weak = Rc::downgrade(self);
        let on_change: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(map) = weak.upgrade() {
                map.request_frame();
            }
        });
        let previous = self.style();
        let mut sources = HashMap::new();
        for (id, mut source) in style.sources {
            if let Some(url) = &source.url {
                source.url = Some(match url.strip_prefix(PMTILES) {
                    Some(archive) => format!("{PMTILES}{}", resolve_url(base, archive)),
                    None => resolve_url(base, url),
                });
            }
            source.tiles = source.tiles.iter().map(|t| resolve_url(base, t)).collect();
            if let Some(kept) = previous
                .as_ref()
                .and_then(|p| p.sources.get(&id))
                .filter(|kept| kept.definition == source)
            {
                kept.reset(layers.clone());
                sources.insert(id, kept.clone());
                continue;
            }
            let archive = source
                .url
                .as_deref()
                .and_then(|url| url.strip_prefix(PMTILES))
                .map(str::to_string);
            let needs_tilejson =
                source.tiles.is_empty() && source.url.is_some() && archive.is_none();
            let geojson = match &source.kind {
                SourceKind::GeoJson { data, options } => Some((data.clone(), options.clone())),
                _ => None,
            };
            let tile_source =
                TileSource::new(id.clone(), source, layers.clone(), on_change.clone());
            if needs_tilejson {
                fetch_tilejson(&tile_source, on_change.clone());
            }
            if let Some(archive) = archive {
                open_pmtiles(&tile_source, archive);
            }
            if let Some((data, options)) = geojson {
                let data = self
                    .pending_geojson
                    .borrow_mut()
                    .remove(&id)
                    .unwrap_or(data);
                let data = match data.as_str() {
                    Some(url) => serde_json::Value::String(resolve_url(base, url)),
                    None => data,
                };
                load_geojson(&tile_source, data, options);
            }
            sources.insert(id, tile_source);
        }
        let sprite: Vec<(String, String)> = style
            .sprite
            .into_iter()
            .map(|(id, url)| (id, resolve_url(base, &url)))
            .collect();
        let state = StyleState {
            sources,
            layers,
            light: style.light,
            glyphs: style.glyphs.map(|g| {
                let url = resolve_url(base, &g);
                previous
                    .as_ref()
                    .and_then(|p| p.glyphs.clone())
                    .filter(|kept| kept.url() == url)
                    .unwrap_or_else(|| GlyphStore::new(url, on_change.clone()))
            }),
            sprites: previous
                .as_ref()
                .filter(|p| p.sprite == sprite)
                .map_or_else(|| Rc::new(SpriteStore::default()), |p| p.sprites.clone()),
            sprite,
            projection: style.projection,
            on_change,
        };
        *self.style.borrow_mut() = Some(Rc::new(state));
        self.request_frame();
    }
}

/// A change the app makes to the loaded style document.
pub(crate) enum StyleEdit {
    AddSource(String, serde_json::Value),
    AddLayer(serde_json::Value, Option<String>),
}

impl StyleEdit {
    fn apply(self, document: &mut serde_json::Value) {
        match self {
            StyleEdit::AddSource(id, source) => {
                if let Some(sources) = document
                    .as_object_mut()
                    .map(|d| d.entry("sources").or_insert_with(|| serde_json::json!({})))
                    .and_then(serde_json::Value::as_object_mut)
                {
                    sources.insert(id, source);
                }
            }
            StyleEdit::AddLayer(layer, before) => {
                let Some(layers) = document
                    .as_object_mut()
                    .map(|d| d.entry("layers").or_insert_with(|| serde_json::json!([])))
                    .and_then(serde_json::Value::as_array_mut)
                else {
                    return;
                };
                let id = layer.get("id").cloned();
                layers.retain(|l| id.is_none() || l.get("id") != id.as_ref());
                let index = before
                    .and_then(|before| {
                        layers.iter().position(|l| {
                            l.get("id").and_then(serde_json::Value::as_str) == Some(before.as_str())
                        })
                    })
                    .unwrap_or(layers.len());
                layers.insert(index, layer);
            }
        }
    }
}

/// Builds a GeoJSON source's tiles from `data`, a GeoJSON object or the URL of one.
fn load_geojson(source: &Rc<TileSource>, data: serde_json::Value, options: GeoJsonOptions) {
    let weak = Rc::downgrade(source);
    whisker::spawn_local(async move {
        let data = match data.as_str() {
            Some(url) => match crate::net::fetch(url.to_string(), None).await {
                Ok(Some(bytes)) => match serde_json::from_slice(&bytes) {
                    Ok(data) => data,
                    Err(error) => {
                        log(&format!("whisker-map: GeoJSON {url}: {error}"));
                        return;
                    }
                },
                Ok(None) => {
                    log(&format!("whisker-map: GeoJSON {url} not found"));
                    return;
                }
                Err(error) => {
                    log(&format!("whisker-map: GeoJSON {url}: {error}"));
                    return;
                }
            },
            None => data,
        };
        let index = crate::work::run(move || Arc::new(GeoJsonIndex::new(&data, options))).await;
        if let Some(source) = weak.upgrade() {
            source.set_geojson(index);
        }
    });
}

const PMTILES: &str = "pmtiles://";

fn open_pmtiles(source: &Rc<TileSource>, url: String) {
    let weak = Rc::downgrade(source);
    whisker::spawn_local(async move {
        let archive = match Archive::open(url).await {
            Ok(archive) => Rc::new(archive),
            Err(error) => {
                log(&format!("whisker-map: PMTiles {error}"));
                return;
            }
        };
        if let Some(source) = weak.upgrade() {
            source.set_pmtiles(archive);
        }
    });
}

fn fetch_tilejson(source: &Rc<TileSource>, on_change: Rc<dyn Fn()>) {
    let Some(url) = source.source.borrow().url.clone() else {
        return;
    };
    let weak = Rc::downgrade(source);
    whisker::spawn_local(async move {
        let json = match crate::net::fetch(url.clone(), None).await {
            Ok(Some(bytes)) => serde_json::from_slice::<serde_json::Value>(&bytes),
            Ok(None) => {
                log(&format!("whisker-map: TileJSON {url} not found"));
                return;
            }
            Err(error) => {
                log(&format!("whisker-map: TileJSON {url}: {error}"));
                return;
            }
        };
        let json = match json {
            Ok(json) => json,
            Err(error) => {
                log(&format!("whisker-map: TileJSON {url}: {error}"));
                return;
            }
        };
        let Some(source) = weak.upgrade() else { return };
        {
            let mut s = source.source.borrow_mut();
            s.apply_tilejson(&json);
            let base = Some(url.as_str());
            s.tiles = s.tiles.iter().map(|t| resolve_url(base, t)).collect();
        }
        source.ready.set(!source.source.borrow().tiles.is_empty());
        on_change();
    });
}

/// Resolves `url` against the document it was found in.
pub(crate) fn resolve_url(base: Option<&str>, url: &str) -> String {
    if url.contains("://") || url.starts_with("data:") {
        return url.to_string();
    }
    let Some(base) = base else {
        return url.to_string();
    };
    if let Some(rest) = url.strip_prefix("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return format!("{scheme}://{rest}");
    }
    let Some(scheme_end) = base.find("://") else {
        return url.to_string();
    };
    let after_scheme = scheme_end + 3;
    if url.starts_with('/') {
        let host_end = base[after_scheme..]
            .find('/')
            .map_or(base.len(), |i| after_scheme + i);
        return format!("{}{url}", &base[..host_end]);
    }
    let path = base.split(['?', '#']).next().unwrap_or(base);
    let dir_end = path
        .rfind('/')
        .filter(|&i| i >= after_scheme)
        .map_or(path.len(), |i| i + 1);
    let mut dir = path[..dir_end].to_string();
    if !dir.ends_with('/') {
        dir.push('/');
    }
    format!("{dir}{url}")
}

pub(crate) fn clamp_camera(camera: MapCamera, min_zoom: f64) -> MapCamera {
    MapCamera {
        center: clamp_center(camera.center),
        zoom: camera.zoom.clamp(min_zoom, MAX_ZOOM),
        bearing: normalize_bearing(camera.bearing),
        pitch: camera.pitch.clamp(0.0, MAX_PITCH),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_urls() {
        let base = Some("https://example.com/styles/liberty.json?key=1");
        assert_eq!(
            resolve_url(base, "tiles.json"),
            "https://example.com/styles/tiles.json"
        );
        assert_eq!(resolve_url(base, "/planet"), "https://example.com/planet");
        assert_eq!(
            resolve_url(base, "//cdn.example.com/a"),
            "https://cdn.example.com/a"
        );
        assert_eq!(resolve_url(base, "https://other/x"), "https://other/x");
        assert_eq!(resolve_url(None, "x"), "x");
    }

    #[test]
    fn clamps_the_camera() {
        let c = clamp_camera(
            MapCamera {
                center: LatLng::new(89.0, 190.0),
                zoom: 30.0,
                bearing: 270.0,
                pitch: 90.0,
            },
            0.0,
        );
        assert!(c.center.lat < 85.06);
        assert!((c.center.lng + 170.0).abs() < 1e-9);
        assert_eq!(c.zoom, MAX_ZOOM);
        assert_eq!(c.bearing, -90.0);
        assert_eq!(c.pitch, MAX_PITCH);
    }
}
