//! Tile sources: fetching, decoding, caching, and choosing what to draw.

pub(crate) mod bucket;
pub(crate) mod geojson;
pub(crate) mod mvt;
pub(crate) mod pmtiles;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use crate::geo::{PlacedTile, TileId};
use crate::style::{Layer, LayerKind, Source, SourceKind};
use bucket::Bucket;

/// What a loaded tile holds.
pub(crate) enum TileContent {
    Vector {
        buckets: Vec<Bucket>,
        symbols: Vec<crate::symbols::SymbolFeature>,
        features: Features,
    },
    Raster(Image),
    Dem(Dem),
}

/// A vector tile's decoded features, which taps search.
pub(crate) struct Features {
    decoded: std::cell::OnceCell<Arc<Vec<mvt::Layer>>>,
    /// The MVT the features decode from, when they were made elsewhere and not sent along.
    encoded: Vec<u8>,
}

impl Features {
    pub fn decoded(layers: Arc<Vec<mvt::Layer>>) -> Features {
        Features {
            decoded: std::cell::OnceCell::from(layers),
            encoded: Vec::new(),
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn encoded(bytes: Vec<u8>) -> Features {
        Features {
            decoded: std::cell::OnceCell::new(),
            encoded: bytes,
        }
    }

    pub fn layers(&self) -> &Arc<Vec<mvt::Layer>> {
        self.decoded
            .get_or_init(|| Arc::new(mvt::decode(&self.encoded).unwrap_or_default()))
    }
}

pub(crate) struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Elevations in meters, `size` samples across with a one-sample border.
pub(crate) struct Dem {
    pub size: u32,
    pub heights: Vec<f32>,
}

impl Dem {
    pub fn height(&self, x: i32, y: i32) -> f32 {
        let stride = self.size as i32 + 2;
        let x = (x + 1).clamp(0, stride - 1);
        let y = (y + 1).clamp(0, stride - 1);
        self.heights[(y * stride + x) as usize]
    }
}

pub(crate) enum TileState {
    Loading,
    Loaded(Rc<LoadedTile>),
    /// Missing on the server, or failed: drawn as nothing, retried after eviction.
    Empty,
}

pub(crate) struct LoadedTile {
    pub id: TileId,
    pub content: TileContent,
    /// GPU objects, made by the renderer on first draw.
    pub gpu: RefCell<Option<Box<dyn std::any::Any>>>,
}

struct Entry {
    state: TileState,
    used: u64,
}

type TileFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Option<TileContent>>>>;
/// Makes a tile's content, or `None` when the source has no such tile.
type Loader = Rc<dyn Fn(TileId) -> Option<TileFuture>>;

/// One style source's tiles.
pub(crate) struct TileSource {
    pub id: String,
    /// The source as the style defined it, before TileJSON filled it in.
    pub definition: Source,
    pub source: RefCell<Source>,
    layers: RefCell<Arc<Vec<Arc<Layer>>>>,
    /// Indices into the style's layers that read this source.
    pub layer_indices: RefCell<Vec<usize>>,
    tiles: RefCell<HashMap<TileId, Entry>>,
    frame: Cell<u64>,
    in_flight: Cell<usize>,
    pub ready: Cell<bool>,
    /// Set when the source cannot load, so the map stops waiting on it.
    failed: Cell<bool>,
    /// Counts each tile that finishes loading and each new data set.
    pub data_version: Cell<u64>,
    geojson: RefCell<Option<crate::work::GeoJson>>,
    on_change: Rc<dyn Fn()>,
    loader: RefCell<Option<Loader>>,
}

const MAX_IN_FLIGHT: usize = 16;
const CACHE_SIZE: usize = 256;

impl TileSource {
    pub fn new(
        id: String,
        source: Source,
        layers: Arc<Vec<Arc<Layer>>>,
        on_change: Rc<dyn Fn()>,
    ) -> Rc<TileSource> {
        let indices = layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.source.as_deref() == Some(id.as_str()))
            .map(|(i, _)| i)
            .collect();
        let ready = !source.tiles.is_empty();
        let failed =
            !ready && source.url.is_none() && !matches!(source.kind, SourceKind::GeoJson { .. });
        Rc::new(TileSource {
            id,
            definition: source.clone(),
            source: RefCell::new(source),
            layers: RefCell::new(layers),
            layer_indices: RefCell::new(indices),
            tiles: RefCell::new(HashMap::new()),
            frame: Cell::new(0),
            in_flight: Cell::new(0),
            ready: Cell::new(ready),
            failed: Cell::new(failed),
            data_version: Cell::new(0),
            geojson: RefCell::new(None),
            on_change,
            loader: RefCell::new(None),
        })
    }

    /// Replaces how tiles are made, for sources that are not fetched by URL.
    pub fn set_loader(&self, loader: Loader) {
        *self.loader.borrow_mut() = Some(loader);
        self.ready.set(true);
    }

    /// Reads this source's tiles from `archive`, within the zooms and bounds its header gives.
    pub fn set_pmtiles(self: &Rc<Self>, archive: Rc<pmtiles::Archive>) {
        {
            let mut source = self.source.borrow_mut();
            source.min_zoom = archive.header.min_zoom;
            source.max_zoom = archive.header.max_zoom;
            source.bounds = Some(archive.header.bounds);
        }
        let weak = Rc::downgrade(self);
        self.set_loader(Rc::new(move |id| {
            let source = weak.upgrade()?;
            let layers = source.layers.borrow().clone();
            let indices = source.layer_indices.borrow().clone();
            let (kind, max_zoom) = {
                let source = source.source.borrow();
                (source.kind.clone(), source.max_zoom)
            };
            let archive = archive.clone();
            Some(Box::pin(async move {
                let bytes = match archive.tile(id).await {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) => return None,
                    Err(error) => {
                        log(&format!("whisker-map: {error}"));
                        return None;
                    }
                };
                crate::work::tile(id, kind, bytes, layers, indices, max_zoom).await
            }))
        }));
        (self.on_change)();
    }

    /// Cuts this source's tiles from `index`, replacing any tiles made from earlier data.
    pub fn set_geojson(self: &Rc<Self>, index: crate::work::GeoJson) {
        let weak = Rc::downgrade(self);
        self.tiles.borrow_mut().clear();
        *self.geojson.borrow_mut() = Some(index.clone());
        self.data_version.set(self.data_version.get() + 1);
        self.set_loader(Rc::new(move |id| {
            let source = weak.upgrade()?;
            let layers = source.layers.borrow().clone();
            let indices = source.layer_indices.borrow().clone();
            let max_zoom = source.source.borrow().max_zoom;
            let index = index.clone();
            Some(Box::pin(crate::work::geojson_tile(
                index, id, layers, indices, max_zoom,
            )))
        }));
        (self.on_change)();
    }

    /// The index this GeoJSON source's tiles are cut from, once its data has loaded.
    pub fn geojson(&self) -> Option<crate::work::GeoJson> {
        self.geojson.borrow().clone()
    }

    /// Whether the source is set up, or has failed to, and no tile it asked for is still loading.
    pub fn is_loaded(&self) -> bool {
        (self.ready.get() || self.failed.get()) && self.in_flight.get() == 0
    }

    pub fn fail(&self) {
        self.failed.set(true);
        (self.on_change)();
    }

    /// Throws away every tile, for a style change that alters their geometry.
    pub fn reset(&self, layers: Arc<Vec<Arc<Layer>>>) {
        let indices = layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.source.as_deref() == Some(self.id.as_str()))
            .map(|(i, _)| i)
            .collect();
        *self.layer_indices.borrow_mut() = indices;
        *self.layers.borrow_mut() = layers;
        self.tiles.borrow_mut().clear();
    }

    /// The zoom of the tiles that best fit a camera zoom.
    pub fn ideal_zoom(&self, zoom: f64) -> u8 {
        let source = self.source.borrow();
        let (tile_size, round) = match source.kind {
            SourceKind::Raster { tile_size } | SourceKind::RasterDem { tile_size, .. } => {
                (tile_size, true)
            }
            _ => (512, false),
        };
        let z = zoom + (512.0 / f64::from(tile_size)).log2();
        let z = if round { z.round() } else { z.floor() };
        (z.max(0.0) as u8).clamp(source.min_zoom, source.max_zoom)
    }

    pub fn begin_frame(&self) {
        self.frame.set(self.frame.get() + 1);
    }

    fn in_bounds(&self, id: TileId) -> bool {
        let source = self.source.borrow();
        if id.z < source.min_zoom {
            return false;
        }
        let Some([west, south, east, north]) = source.bounds else {
            return true;
        };
        let n = f64::from(1u32 << id.z);
        let lng = |x: f64| x / n * 360.0 - 180.0;
        let lat = |y: f64| {
            let k = std::f64::consts::PI * (1.0 - 2.0 * y / n);
            k.sinh().atan().to_degrees()
        };
        let (x0, x1) = (lng(f64::from(id.x)), lng(f64::from(id.x + 1)));
        let (y0, y1) = (lat(f64::from(id.y + 1)), lat(f64::from(id.y)));
        x1 >= west && x0 <= east && y1 >= south && y0 <= north
    }

    /// The loaded tiles to draw for `wanted`, borrowing ancestors and children while some load or
    /// are not yet `ready` to draw.
    pub fn tiles_for(
        self: &Rc<Self>,
        wanted: &[PlacedTile],
        ready: &mut dyn FnMut(&LoadedTile) -> bool,
    ) -> Vec<(PlacedTile, Rc<LoadedTile>)> {
        if !self.ready.get() {
            return Vec::new();
        }
        let frame = self.frame.get();
        let mut chosen: Vec<(PlacedTile, Rc<LoadedTile>)> = Vec::new();
        let mut seen = HashSet::new();
        for placed in wanted {
            if !self.in_bounds(placed.id) {
                continue;
            }
            self.request(placed.id);
            if let Some(tile) = self.loaded(placed.id, ready) {
                if seen.insert(*placed) {
                    chosen.push((*placed, tile));
                }
                continue;
            }
            // Children first: they were drawn a moment ago when zooming out.
            let mut covered = false;
            for child in placed.id.children() {
                if let Some(tile) = self.loaded(child, ready) {
                    let p = PlacedTile {
                        id: child,
                        wrap: placed.wrap,
                    };
                    if seen.insert(p) {
                        chosen.push((p, tile));
                    }
                    covered = true;
                }
            }
            let mut ancestor = placed.id;
            for _ in 0..6 {
                let Some(parent) = ancestor.parent() else {
                    break;
                };
                ancestor = parent;
                if let Some(tile) = self.loaded(ancestor, ready) {
                    let p = PlacedTile {
                        id: ancestor,
                        wrap: placed.wrap,
                    };
                    if seen.insert(p) {
                        chosen.push((p, tile));
                    }
                    covered = true;
                    break;
                }
            }
            let _ = covered;
        }
        for (_, tile) in &chosen {
            if let Some(entry) = self.tiles.borrow_mut().get_mut(&tile.id) {
                entry.used = frame;
            }
        }
        // Ancestors first, so children drawn later take their place.
        chosen.sort_by_key(|(p, _)| p.id.z);
        chosen
    }

    fn loaded(
        &self,
        id: TileId,
        ready: &mut dyn FnMut(&LoadedTile) -> bool,
    ) -> Option<Rc<LoadedTile>> {
        let tile = {
            let mut tiles = self.tiles.borrow_mut();
            let entry = tiles.get_mut(&id)?;
            entry.used = self.frame.get();
            match &entry.state {
                TileState::Loaded(tile) => tile.clone(),
                _ => return None,
            }
        };
        ready(&tile).then_some(tile)
    }

    fn request(self: &Rc<Self>, id: TileId) {
        {
            let mut tiles = self.tiles.borrow_mut();
            if let Some(entry) = tiles.get_mut(&id) {
                entry.used = self.frame.get();
                return;
            }
            if self.in_flight.get() >= MAX_IN_FLIGHT {
                return;
            }
            tiles.insert(
                id,
                Entry {
                    state: TileState::Loading,
                    used: self.frame.get(),
                },
            );
        }
        self.in_flight.set(self.in_flight.get() + 1);
        let weak = Rc::downgrade(self);
        let future = self.load(id);
        whisker::spawn_local(async move {
            let content = future.await;
            finish(&weak, id, content);
        });
    }

    fn load(&self, id: TileId) -> TileFuture {
        if let Some(loader) = self.loader.borrow().as_ref() {
            if let Some(future) = loader(id) {
                return future;
            }
            return Box::pin(async { None });
        }
        let source = self.source.borrow().clone();
        let layers = self.layers.borrow().clone();
        let indices = self.layer_indices.borrow().clone();
        let template = source.tiles[(id.x as usize + id.y as usize) % source.tiles.len()].clone();
        let url = id.url(&template, source.tms);
        Box::pin(async move {
            let bytes = match crate::net::fetch(url, None).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return None,
                Err(error) => {
                    log(&format!("whisker-map: {error}"));
                    return None;
                }
            };
            let kind = source.kind.clone();
            let max_zoom = source.max_zoom;
            crate::work::tile(id, kind, bytes, layers, indices, max_zoom).await
        })
    }

    /// Drops tiles unused for a while, keeping the cache under its size.
    pub fn evict(&self) {
        let mut tiles = self.tiles.borrow_mut();
        if tiles.len() <= CACHE_SIZE {
            return;
        }
        let mut entries: Vec<(TileId, u64)> = tiles
            .iter()
            .filter(|(_, e)| !matches!(e.state, TileState::Loading))
            .map(|(id, e)| (*id, e.used))
            .collect();
        entries.sort_by_key(|(_, used)| *used);
        let excess = tiles.len() - CACHE_SIZE;
        for (id, _) in entries.into_iter().take(excess) {
            tiles.remove(&id);
        }
    }
}

fn finish(weak: &Weak<TileSource>, id: TileId, content: Option<TileContent>) {
    let Some(source) = weak.upgrade() else { return };
    source
        .in_flight
        .set(source.in_flight.get().saturating_sub(1));
    let state = match content {
        Some(content) => TileState::Loaded(Rc::new(LoadedTile {
            id,
            content,
            gpu: RefCell::new(None),
        })),
        None => TileState::Empty,
    };
    if let Some(entry) = source.tiles.borrow_mut().get_mut(&id) {
        entry.state = state;
    }
    source.data_version.set(source.data_version.get() + 1);
    (source.on_change)();
}

pub(crate) fn log(message: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys_log(message);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{message}");
}

#[cfg(target_arch = "wasm32")]
fn web_sys_log(message: &str) {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console, js_name = warn)]
        fn warn(message: &str);
    }
    warn(message);
}

/// Decodes a fetched tile and builds its buckets.
pub(crate) fn process(
    id: TileId,
    kind: &SourceKind,
    bytes: &[u8],
    layers: &[Arc<Layer>],
    indices: &[usize],
    max_zoom: u8,
) -> Option<TileContent> {
    match kind {
        SourceKind::Vector => {
            let decoded = match mvt::decode(bytes) {
                Ok(decoded) => decoded,
                Err(error) => {
                    log(&format!("whisker-map: tile {id:?}: {error}"));
                    return None;
                }
            };
            Some(vector_content(
                id,
                Arc::new(decoded),
                layers,
                indices,
                max_zoom,
            ))
        }
        SourceKind::Raster { .. } => {
            let image = image::load_from_memory(bytes).ok()?.into_rgba8();
            let (width, height) = image.dimensions();
            Some(TileContent::Raster(Image {
                width,
                height,
                pixels: image.into_raw(),
            }))
        }
        SourceKind::RasterDem { terrarium, .. } => {
            let image = image::load_from_memory(bytes).ok()?.into_rgba8();
            Some(TileContent::Dem(decode_dem(&image, *terrarium)))
        }
        _ => None,
    }
}

pub(crate) fn vector_content(
    id: TileId,
    decoded: Arc<Vec<mvt::Layer>>,
    layers: &[Arc<Layer>],
    indices: &[usize],
    max_zoom: u8,
) -> TileContent {
    let zoom = f64::from(id.z);
    let mut buckets = Vec::new();
    let mut symbols = Vec::new();
    for &index in indices {
        let layer = &layers[index];
        if !layer.visible
            || matches!(
                layer.kind,
                LayerKind::Background
                    | LayerKind::Raster
                    | LayerKind::Hillshade
                    | LayerKind::Heatmap
            )
        {
            continue;
        }
        // Patterns are resolved once per draw, so one that varies by feature cannot be drawn.
        if ["fill-pattern", "line-pattern", "fill-extrusion-pattern"]
            .iter()
            .any(|name| layer.has(name) && layer.get(name).feature_dependent)
        {
            continue;
        }
        // A tile is drawn up to a zoom past its own, without limit at the source's last zoom.
        let overzoomed = id.z >= max_zoom;
        if (!overzoomed && zoom + 1.0 < layer.min_zoom) || zoom >= layer.max_zoom {
            continue;
        }
        let Some((source_index, source_layer)) = decoded.iter().enumerate().find(|(_, l)| {
            l.name
                == layer
                    .source_layer
                    .as_deref()
                    .unwrap_or(geojson::GEOJSON_LAYER)
        }) else {
            continue;
        };
        if layer.kind == LayerKind::Symbol {
            symbols.extend(crate::symbols::build(
                index,
                layer,
                source_index,
                &source_layer.features,
                source_layer.extent,
                zoom,
            ));
            continue;
        }
        if let Some(bucket) = bucket::build(
            index,
            layer,
            &source_layer.features,
            source_layer.extent,
            zoom,
        ) {
            buckets.push(bucket);
        }
    }
    TileContent::Vector {
        buckets,
        symbols,
        features: Features::decoded(decoded),
    }
}

fn decode_dem(image: &image::RgbaImage, terrarium: bool) -> Dem {
    let (w, h) = image.dimensions();
    let size = w.min(h);
    let stride = size as usize + 2;
    let mut heights = vec![0.0f32; stride * stride];
    for y in 0..size {
        for x in 0..size {
            let [r, g, b, _] = image.get_pixel(x, y).0;
            let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
            let height = if terrarium {
                r * 256.0 + g + b / 256.0 - 32768.0
            } else {
                -10000.0 + (r * 256.0 * 256.0 + g * 256.0 + b) * 0.1
            };
            heights[(y as usize + 1) * stride + x as usize + 1] = height;
        }
    }
    // Borders repeat the edge so slopes at the tile's edge stay flat.
    for i in 0..stride {
        heights[i] = heights[stride + i];
        heights[(stride - 1) * stride + i] = heights[(stride - 2) * stride + i];
    }
    for row in 0..stride {
        heights[row * stride] = heights[row * stride + 1];
        heights[row * stride + stride - 1] = heights[row * stride + stride - 2];
    }
    Dem { size, heights }
}
