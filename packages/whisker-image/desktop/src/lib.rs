//! Desktop Host implementation for `whisker-image`.

use std::sync::{Arc, Mutex};

use whisker_desktop::{
    DesktopEventEmitter, DesktopNativeEvent, DesktopRaster, DesktopViewDefinition,
    ModuleDefinition, WhiskerModule, WhiskerValue,
};
use whisker_image::ImageMode;
use whisker_image::loader::{RgbaImage, fit_image, load_image, parse_mode};

const MODULE_NAME: &str = "whisker-image:Image";

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
        let Some(mode) = parse_mode(value) else {
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
    fn definition_exposes_one_raster_view() {
        assert_eq!(__whisker_module_definition().factories().len(), 1);
    }
}
