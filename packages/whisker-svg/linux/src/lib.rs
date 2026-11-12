//! Linux Host implementation for `whisker-svg`.

#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::rc::Rc;

use whisker::platform_module::WhiskerValue;
use whisker_linux::gtk4::{self as gtk, cairo, prelude::*};
use whisker_linux::{LinuxModuleDefinition, LinuxView, LinuxViewDefinition};
use whisker_svg::Color;
use whisker_svg::raster::{decode_display_list, parse_color, rasterize};

struct SvgState {
    display_list: Vec<u8>,
    tint: Color,
    cache: Option<(i32, i32, cairo::ImageSurface)>,
}

struct SvgView {
    area: gtk::DrawingArea,
    state: Rc<RefCell<SvgState>>,
}

impl LinuxView for SvgView {
    fn widget(&self) -> gtk::Widget {
        self.area.clone().upcast()
    }
}

impl SvgView {
    fn new() -> Self {
        let area = gtk::DrawingArea::new();
        let state = Rc::new(RefCell::new(SvgState {
            display_list: Vec::new(),
            tint: Color::BLACK,
            cache: None,
        }));
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
        Self { area, state }
    }

    fn update(&self, change: impl FnOnce(&mut SvgState)) {
        let mut state = self.state.borrow_mut();
        change(&mut state);
        state.cache = None;
        drop(state);
        self.area.queue_draw();
    }
}

fn surface(state: &SvgState, width: i32, height: i32, scale: i32) -> Option<cairo::ImageSurface> {
    let (w, h) = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
    let mut pixels = rasterize(&state.display_list, state.tint, w, h)?;
    // tiny-skia writes premultiplied RGBA; cairo's ARGB32 is BGRA in memory on little-endian.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let stride = width * 4;
    let surface =
        cairo::ImageSurface::create_for_data(pixels, cairo::Format::ARgb32, width, height, stride)
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

struct SvgModule;

#[whisker::WhiskerModule]
impl whisker::WhiskerModule for SvgModule {
    type Definition = LinuxModuleDefinition;

    fn definition() -> Self::Definition {
        LinuxModuleDefinition::new().name("whisker-svg:Svg").view(
            LinuxViewDefinition::new("whisker-svg:Svg", |_| SvgView::new())
                .prop(
                    "display-list",
                    |view, value| {
                        let decoded = decode_display_list(string(value));
                        view.update(|state| state.display_list = decoded);
                    },
                    |view| view.update(|state| state.display_list.clear()),
                )
                .prop(
                    "color",
                    |view, value| {
                        let tint = parse_color(string(value));
                        view.update(|state| state.tint = tint);
                    },
                    |view| view.update(|state| state.tint = Color::BLACK),
                ),
        )
    }
}
