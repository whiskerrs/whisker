//! Linux Host implementation for `whisker-surface`.
//!
//! GTK cannot hand a child window to a renderer, so the renderer presents finished frames through a
//! `whisker_surface::DmabufSink` and this widget shows them as textures.

#![cfg(target_os = "linux")]

use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::time::Duration;

use whisker::platform_module::WhiskerValue;
use whisker_linux::gtk4::{self as gtk, gdk, glib, prelude::*};
use whisker_linux::{LinuxModuleDefinition, LinuxView, LinuxViewDefinition};
use whisker_surface::{DmabufFrame, DmabufPresenter, DmabufSink, NativeSurface, SurfaceSize, host};

struct State {
    surface: Option<u64>,
    attached: Option<u64>,
    reported: Option<SurfaceSize>,
    ticking: Option<gtk::TickCallbackId>,
}

/// Calls Rust only from the frame clock, never from a prop setter inside Whisker's commit.
struct SurfaceWidget {
    picture: gtk::Picture,
    state: Rc<RefCell<State>>,
}

impl LinuxView for SurfaceWidget {
    fn widget(&self) -> gtk::Widget {
        self.picture.clone().upcast()
    }
}

struct Presenter {
    picture: glib::WeakRef<gtk::Picture>,
}

impl DmabufPresenter for Presenter {
    fn present(&self, frame: DmabufFrame) -> bool {
        let Some(picture) = self.picture.upgrade() else {
            return false;
        };
        let builder = gdk::DmabufTextureBuilder::new()
            .set_display(&picture.display())
            .set_width(frame.width)
            .set_height(frame.height)
            .set_fourcc(frame.fourcc)
            .set_modifier(frame.modifier)
            .set_premultiplied(true)
            .set_n_planes(frame.planes.len() as u32);
        let DmabufFrame {
            planes, release, ..
        } = frame;
        // SAFETY: the planes' fds stay open until GTK calls the release closure, which owns them.
        let texture = unsafe {
            let builder = planes
                .iter()
                .enumerate()
                .fold(builder, |builder, (index, plane)| {
                    let index = index as u32;
                    builder
                        .set_fd(index, plane.fd.as_raw_fd())
                        .set_offset(index, plane.offset)
                        .set_stride(index, plane.stride)
                });
            builder.build_with_release_func(move || {
                drop(planes);
                if let Some(release) = release {
                    release();
                }
            })
        };
        match texture {
            Ok(texture) => {
                picture.set_paintable(Some(&texture));
                true
            }
            Err(_) => false,
        }
    }

    fn modifiers(&self, fourcc: u32) -> Vec<u64> {
        let Some(picture) = self.picture.upgrade() else {
            return Vec::new();
        };
        let formats = picture.display().dmabuf_formats();
        (0..formats.n_formats())
            .map(|index| formats.format(index))
            .filter(|(format, _)| *format == fourcc)
            .map(|(_, modifier)| modifier)
            .collect()
    }

    fn present_pixels(&self, width: u32, height: u32, stride: u32, pixels: &[u8]) {
        let Some(picture) = self.picture.upgrade() else {
            return;
        };
        let bytes = glib::Bytes::from(pixels);
        let texture = gdk::MemoryTexture::new(
            width as i32,
            height as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &bytes,
            stride as usize,
        );
        picture.set_paintable(Some(&texture));
    }
}

impl SurfaceWidget {
    fn new() -> Self {
        let picture = gtk::Picture::new();
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk::ContentFit::Fill);
        let state = Rc::new(RefCell::new(State {
            surface: None,
            attached: None,
            reported: None,
            ticking: None,
        }));
        let resize_state = Rc::downgrade(&state);
        picture.connect_realize(move |picture| {
            if let Some(state) = resize_state.upgrade() {
                schedule(picture, &state);
            }
        });
        Self { picture, state }
    }

    fn set_surface(&self, value: &str) {
        let id = host::parse_id(value);
        {
            let mut state = self.state.borrow_mut();
            if state.surface != id {
                if let Some(attached) = state.attached.take() {
                    host::detach(attached);
                }
                state.reported = None;
                state.surface = id;
            }
        }
        schedule(&self.picture, &self.state);
    }
}

impl Drop for SurfaceWidget {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        if let Some(ticking) = state.ticking.take() {
            ticking.remove();
        }
        if let Some(attached) = state.attached.take() {
            host::detach(attached);
        }
    }
}

fn schedule(picture: &gtk::Picture, state: &Rc<RefCell<State>>) {
    if state.borrow().ticking.is_some() {
        return;
    }
    let weak = Rc::downgrade(state);
    let id = picture.add_tick_callback(move |picture, clock| {
        let Some(state) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if tick(picture, &state, clock.frame_time()) {
            glib::ControlFlow::Continue
        } else {
            state.borrow_mut().ticking = None;
            glib::ControlFlow::Break
        }
    });
    state.borrow_mut().ticking = Some(id);
}

fn tick(picture: &gtk::Picture, state: &Rc<RefCell<State>>, frame_time_us: i64) -> bool {
    let (id, size, attach, resize) = {
        let mut state = state.borrow_mut();
        let Some(id) = state.surface else {
            return false;
        };
        let scale = picture.scale_factor().max(1);
        let width = (picture.width() * scale).max(0) as u32;
        let height = (picture.height() * scale).max(0) as u32;
        if width == 0 || height == 0 {
            return true;
        }
        let size = SurfaceSize {
            width,
            height,
            scale: scale as f32,
        };
        let attach = state.attached != Some(id);
        let resize = !attach && state.reported != Some(size);
        state.attached = Some(id);
        state.reported = Some(size);
        (id, size, attach, resize)
    };
    if attach {
        let sink = DmabufSink::new(Presenter {
            picture: picture.downgrade(),
        });
        host::attach(id, NativeSurface::Dmabuf(sink), size);
    } else if resize {
        host::resize(id, size);
    }
    // A resize settles after one more allocation, so keep ticking until the size holds.
    host::frame(id, Duration::from_micros(frame_time_us.max(0) as u64)) || resize || attach
}

fn string(value: &WhiskerValue) -> &str {
    match value {
        WhiskerValue::String(value) => value,
        _ => "",
    }
}

struct SurfaceModule;

#[whisker::WhiskerModule]
impl whisker::WhiskerModule for SurfaceModule {
    type Definition = LinuxModuleDefinition;

    fn definition() -> Self::Definition {
        LinuxModuleDefinition::new()
            .name("whisker-surface:Surface")
            .view(
                LinuxViewDefinition::new("whisker-surface:Surface", |_| SurfaceWidget::new())
                    .prop(
                        "surface",
                        |view, value| view.set_surface(string(value)),
                        |view| view.set_surface(""),
                    )
                    .prop("wheel", |_, _| {}, |_| {})
                    .event("wheel"),
            )
    }
}
