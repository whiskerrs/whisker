//! Linux Host implementation for `whisker-image`.

#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use whisker::platform_module::WhiskerValue;
use whisker_image::ImageMode;
use whisker_image::loader::{RgbaImage, fit_image, load_image, parse_mode};
use whisker_linux::gtk4::{self as gtk, cairo, gio, glib, prelude::*};
use whisker_linux::{LinuxEventEmitter, LinuxModuleDefinition, LinuxView, LinuxViewDefinition};

const NAME: &str = "whisker-image:Image";

#[derive(Default)]
struct ImageState {
    src: String,
    headers: String,
    mode: ImageMode,
    request: u64,
    decoded: Option<Arc<RgbaImage>>,
    cache: Option<(i32, i32, cairo::ImageSurface)>,
}

struct ImageView {
    area: gtk::DrawingArea,
    state: Rc<RefCell<ImageState>>,
    events: LinuxEventEmitter,
}

impl LinuxView for ImageView {
    fn widget(&self) -> gtk::Widget {
        self.area.clone().upcast()
    }
}

impl ImageView {
    fn new(events: LinuxEventEmitter) -> Self {
        let area = gtk::DrawingArea::new();
        let state = Rc::new(RefCell::new(ImageState::default()));
        let draw_state = Rc::clone(&state);
        area.set_draw_func(move |area, cr, width, height| {
            let scale = area.scale_factor();
            let (pixel_width, pixel_height) = (width * scale, height * scale);
            let mut state = draw_state.borrow_mut();
            let stale =
                !matches!(&state.cache, Some((w, h, _)) if *w == pixel_width && *h == pixel_height);
            if stale {
                state.cache = surface(&state, pixel_width, pixel_height, scale)
                    .map(|surface| (pixel_width, pixel_height, surface));
            }
            if let Some((_, _, surface)) = &state.cache {
                let _ = cr.set_source_surface(surface, 0.0, 0.0);
                let _ = cr.paint();
            }
        });
        Self {
            area,
            state,
            events,
        }
    }

    fn set_src(&self, value: &str) {
        self.state.borrow_mut().src = value.to_owned();
        self.reload();
    }

    fn set_headers(&self, value: &str) {
        let reload = {
            let mut state = self.state.borrow_mut();
            state.headers = value.to_owned();
            !state.src.is_empty()
        };
        if reload {
            self.reload();
        }
    }

    fn set_mode(&self, value: &str) {
        let Some(mode) = parse_mode(value) else {
            return;
        };
        let mut state = self.state.borrow_mut();
        if state.mode != mode {
            state.mode = mode;
            state.cache = None;
            drop(state);
            self.area.queue_draw();
        }
    }

    fn reload(&self) {
        let (request, src, headers) = {
            let mut state = self.state.borrow_mut();
            state.request = state.request.wrapping_add(1);
            state.decoded = None;
            state.cache = None;
            (state.request, state.src.clone(), state.headers.clone())
        };
        self.area.queue_draw();
        if src.is_empty() {
            return;
        }
        let state = Rc::downgrade(&self.state);
        let area = self.area.downgrade();
        let events = self.events.clone();
        glib::MainContext::default().spawn_local(async move {
            let Ok(result) = gio::spawn_blocking(move || load_image(&src, &headers)).await else {
                return;
            };
            finish(&state, request, result, &events);
            if let Some(area) = area.upgrade() {
                area.queue_draw();
            }
        });
    }
}

fn finish(
    state: &Weak<RefCell<ImageState>>,
    request: u64,
    result: Result<Arc<RgbaImage>, String>,
    events: &LinuxEventEmitter,
) {
    let Some(state) = state.upgrade() else {
        return;
    };
    let mut state = state.borrow_mut();
    if state.request != request {
        return;
    }
    state.cache = None;
    let (event, detail) = match result {
        Ok(image) => {
            let detail = detail(image.width().into(), image.height().into(), "");
            state.decoded = Some(image);
            ("load", detail)
        }
        Err(error) => ("error", detail(0.0, 0.0, &error)),
    };
    drop(state);
    events.emit(event, detail);
}

fn detail(width: f64, height: f64, error: &str) -> WhiskerValue {
    WhiskerValue::map([
        ("width", WhiskerValue::Float(width)),
        ("height", WhiskerValue::Float(height)),
        ("error", WhiskerValue::String(error.to_owned())),
    ])
}

fn surface(state: &ImageState, width: i32, height: i32, scale: i32) -> Option<cairo::ImageSurface> {
    let decoded = state.decoded.as_ref()?;
    let (w, h) = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
    if w == 0 || h == 0 {
        return None;
    }
    let mut pixels = fit_image(decoded, state.mode, w, h).into_raw();
    // cairo's ARGB32 is premultiplied BGRA in memory on little-endian.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        let premultiply = |channel: u8| ((u16::from(channel) * alpha + 127) / 255) as u8;
        let (r, g, b) = (pixel[0], pixel[1], pixel[2]);
        pixel[0] = premultiply(b);
        pixel[1] = premultiply(g);
        pixel[2] = premultiply(r);
    }
    let surface = cairo::ImageSurface::create_for_data(
        pixels,
        cairo::Format::ARgb32,
        width,
        height,
        width * 4,
    )
    .ok()?;
    surface.set_device_scale(f64::from(scale), f64::from(scale));
    Some(surface)
}

fn string(value: &WhiskerValue) -> &str {
    match value {
        WhiskerValue::String(value) => value,
        _ => "",
    }
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

struct ImageModule;

#[whisker::WhiskerModule]
impl whisker::WhiskerModule for ImageModule {
    type Definition = LinuxModuleDefinition;

    fn definition() -> Self::Definition {
        LinuxModuleDefinition::new()
            .name(NAME)
            .async_function("prefetch", |args, promise, _| {
                let Some((urls, headers)) = prefetch_arguments(args) else {
                    promise.reject("Image.prefetch requires an URL array and headers string");
                    return;
                };
                std::thread::spawn(move || {
                    for url in urls {
                        let _ = load_image(&url, &headers);
                    }
                    promise.resolve(WhiskerValue::Null);
                });
            })
            .view(
                LinuxViewDefinition::new(NAME, ImageView::new)
                    .prop(
                        "src",
                        |view, value| view.set_src(string(value)),
                        |view| view.set_src(""),
                    )
                    .prop(
                        "mode",
                        |view, value| view.set_mode(string(value)),
                        |view| view.set_mode("aspectFill"),
                    )
                    .prop(
                        "headers",
                        |view, value| view.set_headers(string(value)),
                        |view| view.set_headers(""),
                    )
                    .event("load")
                    .event("error"),
            )
    }
}
