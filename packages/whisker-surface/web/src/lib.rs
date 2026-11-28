//! Web Host implementation for `whisker-surface`.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use whisker_surface::{NativeSurface, SurfaceSize, host};
use whisker_web::wasm_bindgen::{JsCast, JsValue};
use whisker_web::{ModuleDefinition, WebViewDefinition, WhiskerModule, WhiskerValue};

struct State {
    canvas: web_sys::HtmlCanvasElement,
    surface: Option<u64>,
    attached: Option<u64>,
    reported: Option<SurfaceSize>,
    frame_requested: bool,
    this: Weak<RefCell<State>>,
}

/// Calls Rust only from a View frame, never from a prop setter inside Whisker's commit.
struct SurfaceCanvas(Rc<RefCell<State>>);

impl SurfaceCanvas {
    fn new(document: &web_sys::Document) -> Result<Self, JsValue> {
        let canvas: web_sys::HtmlCanvasElement =
            document.create_element("canvas")?.unchecked_into();
        canvas.set_attribute("style", "display:block;width:100%;height:100%")?;
        let state = Rc::new_cyclic(|this| {
            RefCell::new(State {
                canvas,
                surface: None,
                attached: None,
                reported: None,
                frame_requested: false,
                this: this.clone(),
            })
        });
        Ok(Self(state))
    }

    fn set_surface(&self, value: &str) {
        let id = host::parse_id(value);
        let mut state = self.0.borrow_mut();
        if state.surface != id {
            if let Some(attached) = state.attached.take() {
                host::detach(attached);
            }
            state.reported = None;
            state.surface = id;
        }
        schedule(&mut state);
    }
}

impl Drop for SurfaceCanvas {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        if let Some(attached) = state.attached.take() {
            host::detach(attached);
        }
    }
}

fn schedule(state: &mut State) {
    if state.frame_requested {
        return;
    }
    state.frame_requested = true;
    let weak = state.this.clone();
    whisker_web::request_view_frame(move |time| tick(&weak, time));
}

fn tick(weak: &Weak<RefCell<State>>, time: f64) {
    let Some(state) = weak.upgrade() else { return };
    let (id, size, attach, resize) = {
        let mut state = state.borrow_mut();
        state.frame_requested = false;
        let Some(id) = state.surface else { return };
        if !state.canvas.is_connected() {
            return;
        }
        let ratio = web_sys::window().map_or(1.0, |window| window.device_pixel_ratio());
        let width = (f64::from(state.canvas.client_width()) * ratio).round() as u32;
        let height = (f64::from(state.canvas.client_height()) * ratio).round() as u32;
        if width == 0 || height == 0 {
            return;
        }
        if (state.canvas.width(), state.canvas.height()) != (width, height) {
            state.canvas.set_width(width);
            state.canvas.set_height(height);
        }
        let size = SurfaceSize {
            width,
            height,
            scale: ratio as f32,
        };
        let attach = state.attached != Some(id);
        let resize = !attach && state.reported != Some(size);
        state.attached = Some(id);
        state.reported = Some(size);
        (id, size, attach, resize)
    };
    if attach {
        let canvas = state.borrow().canvas.clone();
        host::attach(id, NativeSurface::WebCanvas(canvas), size);
    } else if resize {
        host::resize(id, size);
    }
    if host::frame(id, Duration::from_secs_f64(time.max(0.0) / 1000.0)) {
        schedule(&mut state.borrow_mut());
    }
}

struct SurfaceModule;

#[WhiskerModule]
impl WhiskerModule for SurfaceModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name("whisker-surface:Surface")
            .view(
                WebViewDefinition::new(
                    "whisker-surface:Surface",
                    |document, _| SurfaceCanvas::new(document),
                    |view| view.0.borrow().canvas.clone().into(),
                )
                .prop(
                    "surface",
                    |view, value| {
                        let WhiskerValue::String(value) = value else {
                            return Err(JsValue::from_str("Surface property must be a string"));
                        };
                        view.set_surface(value);
                        Ok(())
                    },
                    |view| {
                        view.set_surface("");
                        Ok(())
                    },
                ),
            )
    }
}
