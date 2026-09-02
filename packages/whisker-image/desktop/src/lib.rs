//! Desktop Host implementation for `whisker-image`.

use std::collections::BTreeMap;
use std::io::Read;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use image::imageops::FilterType;
use image::{RgbaImage, imageops};
use whisker_desktop::{
    DesktopEventEmitter, DesktopNativeEvent, DesktopRaster, DesktopViewDefinition,
    ModuleDefinition, WhiskerModule, WhiskerValue,
};

const MODULE_NAME: &str = "whisker-image:Image";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ImageMode {
    #[default]
    AspectFill,
    AspectFit,
    ScaleToFill,
    Center,
}

impl ImageMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "aspectFill" => Some(Self::AspectFill),
            "aspectFit" => Some(Self::AspectFit),
            "scaleToFill" => Some(Self::ScaleToFill),
            "center" => Some(Self::Center),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct CachedRaster {
    generation: u64,
    width: u32,
    height: u32,
    raster: DesktopRaster,
}

#[derive(Debug, Default)]
struct ImageState {
    request: u64,
    generation: u64,
    mode: ImageMode,
    decoded: Option<Arc<RgbaImage>>,
    cache: Option<CachedRaster>,
}

#[derive(Debug)]
struct ImageDesktopView {
    state: Arc<Mutex<ImageState>>,
    events: DesktopEventEmitter,
    src: String,
    headers: String,
}

impl ImageDesktopView {
    fn new(events: DesktopEventEmitter) -> Self {
        Self {
            state: Arc::new(Mutex::new(ImageState::default())),
            events,
            src: String::new(),
            headers: String::new(),
        }
    }

    fn set_src(&mut self, value: &str) {
        self.src.clear();
        self.src.push_str(value);
        self.reload();
    }

    fn set_headers(&mut self, value: &str) {
        self.headers.clear();
        self.headers.push_str(value);
        if !self.src.is_empty() {
            self.reload();
        }
    }

    fn set_mode(&mut self, value: &str) {
        let Some(mode) = ImageMode::parse(value) else {
            return;
        };
        let mut state = self.state.lock().expect("Image state lock poisoned");
        if state.mode != mode {
            state.mode = mode;
            state.generation = state.generation.wrapping_add(1);
            state.cache = None;
        }
    }

    fn reload(&mut self) {
        let request = {
            let mut state = self.state.lock().expect("Image state lock poisoned");
            state.request = state.request.wrapping_add(1);
            state.decoded = None;
            state.cache = None;
            state.generation = state.generation.wrapping_add(1);
            state.request
        };
        if self.src.is_empty() {
            return;
        }
        let src = self.src.clone();
        let headers = self.headers.clone();
        let state = Arc::clone(&self.state);
        let events = self.events.clone();
        std::thread::Builder::new()
            .name("whisker-image-loader".into())
            .spawn(move || {
                let result = load_image(&src, &headers);
                let event = {
                    let mut state = state.lock().expect("Image state lock poisoned");
                    if state.request != request {
                        return;
                    }
                    state.generation = state.generation.wrapping_add(1);
                    state.cache = None;
                    match result {
                        Ok(image) => {
                            let width = image.width();
                            let height = image.height();
                            state.decoded = Some(image);
                            DesktopNativeEvent {
                                event: "load".into(),
                                detail: image_detail(width as f64, height as f64, ""),
                            }
                        }
                        Err(error) => {
                            state.decoded = None;
                            DesktopNativeEvent {
                                event: "error".into(),
                                detail: image_detail(0.0, 0.0, &error),
                            }
                        }
                    }
                };
                events.emit(event);
            })
            .expect("Desktop Host can spawn an image loader");
    }

    fn rasterize(&self, width: u32, height: u32) -> Option<DesktopRaster> {
        if width == 0 || height == 0 {
            return None;
        }
        let mut state = self.state.lock().expect("Image state lock poisoned");
        if let Some(cache) = state.cache.as_ref()
            && cache.generation == state.generation
            && cache.width == width
            && cache.height == height
        {
            return Some(cache.raster.clone());
        }
        let decoded = state.decoded.as_ref()?;
        let pixels = fit_image(decoded, state.mode, width, height);
        let raster = DesktopRaster::new(state.generation, width, height, pixels.into_raw()).ok()?;
        state.cache = Some(CachedRaster {
            generation: state.generation,
            width,
            height,
            raster: raster.clone(),
        });
        Some(raster)
    }
}

struct ImageModule;

#[whisker_desktop::WhiskerModule]
impl WhiskerModule for ImageModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .async_function("prefetch", |args, promise, _| {
                let Some((urls, headers)) = prefetch_arguments(args) else {
                    promise.reject("Image.prefetch requires an URL array and headers string");
                    return;
                };
                std::thread::Builder::new()
                    .name("whisker-image-prefetch".into())
                    .spawn(move || {
                        for url in urls {
                            let _ = load_image(&url, &headers);
                        }
                        promise.resolve(WhiskerValue::Null);
                    })
                    .expect("Desktop Host can spawn an image prefetch worker");
            })
            .view(
                DesktopViewDefinition::new(MODULE_NAME, ImageDesktopView::new)
                    .prop(
                        "src",
                        |view, value| {
                            let WhiskerValue::String(value) = value else {
                                unreachable!("Desktop Host validates Image src")
                            };
                            view.set_src(value);
                        },
                        |view| view.set_src(""),
                    )
                    .prop(
                        "mode",
                        |view, value| {
                            let WhiskerValue::String(value) = value else {
                                unreachable!("Desktop Host validates Image mode")
                            };
                            view.set_mode(value);
                        },
                        |view| view.set_mode("aspectFill"),
                    )
                    .prop(
                        "headers",
                        |view, value| {
                            let WhiskerValue::String(value) = value else {
                                unreachable!("Desktop Host validates Image headers")
                            };
                            view.set_headers(value);
                        },
                        |view| view.set_headers(""),
                    )
                    .event("load")
                    .event("error")
                    .raster(ImageDesktopView::rasterize),
            )
    }
}

fn load_image(src: &str, headers: &str) -> Result<Arc<RgbaImage>, String> {
    let cache_key = format!("{src}\n{headers}");
    if let Some(image) = image_cache()
        .lock()
        .expect("Image cache lock poisoned")
        .get(&cache_key)
        .cloned()
    {
        return Ok(image);
    }
    let bytes = if src.starts_with("http://") || src.starts_with("https://") {
        let parsed: BTreeMap<String, String> = if headers.trim().is_empty() {
            BTreeMap::new()
        } else {
            serde_json::from_str(headers)
                .map_err(|error| format!("invalid image request headers: {error}"))?
        };
        let mut request = ureq::get(src);
        for (name, value) in parsed {
            request = request.set(&name, &value);
        }
        let response = request.call().map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(64 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        bytes
    } else if let Some(path) = src.strip_prefix("file://") {
        std::fs::read(path).map_err(|error| error.to_string())?
    } else if src.starts_with("data:image/") {
        decode_data_url(src)?
    } else {
        std::fs::read(src).map_err(|error| error.to_string())?
    };
    let image = image::load_from_memory(&bytes)
        .map(image::DynamicImage::into_rgba8)
        .map(Arc::new)
        .map_err(|error| error.to_string())?;
    image_cache()
        .lock()
        .expect("Image cache lock poisoned")
        .put(cache_key, Arc::clone(&image));
    Ok(image)
}

fn image_cache() -> &'static Mutex<lru::LruCache<String, Arc<RgbaImage>>> {
    static CACHE: OnceLock<Mutex<lru::LruCache<String, Arc<RgbaImage>>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(lru::LruCache::new(
            NonZeroUsize::new(64).expect("cache capacity is non-zero"),
        ))
    })
}

fn decode_data_url(src: &str) -> Result<Vec<u8>, String> {
    let (metadata, payload) = src
        .split_once(',')
        .ok_or_else(|| "image data URL has no payload".to_owned())?;
    if !metadata.ends_with(";base64") {
        return Err("only base64 image data URLs are supported".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|error| error.to_string())
}

fn fit_image(source: &RgbaImage, mode: ImageMode, width: u32, height: u32) -> RgbaImage {
    match mode {
        ImageMode::ScaleToFill => imageops::resize(source, width, height, FilterType::Triangle),
        ImageMode::AspectFill => {
            let scale =
                (width as f64 / source.width() as f64).max(height as f64 / source.height() as f64);
            let resized = imageops::resize(
                source,
                (source.width() as f64 * scale).round().max(1.0) as u32,
                (source.height() as f64 * scale).round().max(1.0) as u32,
                FilterType::Triangle,
            );
            let x = resized.width().saturating_sub(width) / 2;
            let y = resized.height().saturating_sub(height) / 2;
            imageops::crop_imm(&resized, x, y, width, height).to_image()
        }
        ImageMode::AspectFit => {
            let scale =
                (width as f64 / source.width() as f64).min(height as f64 / source.height() as f64);
            let resized = imageops::resize(
                source,
                (source.width() as f64 * scale).round().max(1.0) as u32,
                (source.height() as f64 * scale).round().max(1.0) as u32,
                FilterType::Triangle,
            );
            centered(&resized, width, height)
        }
        ImageMode::Center => centered(source, width, height),
    }
}

fn centered(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let mut target = RgbaImage::new(width, height);
    let x = (width as i64 - source.width() as i64) / 2;
    let y = (height as i64 - source.height() as i64) / 2;
    imageops::overlay(&mut target, source, x, y);
    target
}

fn image_detail(width: f64, height: f64, error: &str) -> WhiskerValue {
    WhiskerValue::map([
        ("width", WhiskerValue::Float(width)),
        ("height", WhiskerValue::Float(height)),
        ("error", WhiskerValue::String(error.to_owned())),
    ])
}

fn prefetch_arguments(args: &[WhiskerValue]) -> Option<(Vec<String>, String)> {
    let [WhiskerValue::Array(urls), WhiskerValue::String(headers)] = args else {
        return None;
    };
    let urls = urls
        .iter()
        .map(|value| match value {
            WhiskerValue::String(value) => Some(value.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some((urls, headers.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_fit_letterboxes_without_distorting() {
        let source = RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255]));
        let fitted = fit_image(&source, ImageMode::AspectFit, 4, 4);
        assert_eq!(fitted.dimensions(), (4, 4));
        assert_eq!(fitted.get_pixel(0, 0).0[3], 0);
        assert_eq!(fitted.get_pixel(0, 1).0, [255, 0, 0, 255]);
    }

    #[test]
    fn definition_exposes_one_raster_view() {
        assert_eq!(__whisker_module_definition().factories().len(), 1);
    }

    #[test]
    fn file_loader_populates_the_shared_decode_cache() {
        let path =
            std::env::temp_dir().join(format!("whisker-image-desktop-{}.png", std::process::id()));
        image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 255])))
            .save(&path)
            .unwrap();
        let source = path.to_string_lossy();
        let first = load_image(&source, "").unwrap();
        let second = load_image(&source, "").unwrap();
        assert_eq!(first.dimensions(), (2, 3));
        assert!(Arc::ptr_eq(&first, &second));
        std::fs::remove_file(path).ok();
    }
}
