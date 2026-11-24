//! Web Host implementation for `whisker-surface`.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use whisker_surface::{NativeSurface, SurfaceSize, host};
use whisker_web::wasm_bindgen::closure::Closure;
use whisker_web::wasm_bindgen::{JsCast, JsValue};
use whisker_web::{
    ModuleDefinition, WebEventEmitter, WebNativeEvent, WebViewDefinition, WhiskerModule,
    WhiskerValue,
};

struct State {
    canvas: web_sys::HtmlCanvasElement,
    surface: Option<u64>,
    attached: Option<u64>,
    reported: Option<SurfaceSize>,
    frame_request: Option<i32>,
    callback: Option<Closure<dyn FnMut(f64)>>,
    wheel: Option<Closure<dyn FnMut(web_sys::WheelEvent)>>,
    wheel_enabled: bool,
}

/// Calls Rust only from `requestAnimationFrame`, never from a prop setter inside Whisker's commit.
struct SurfaceCanvas(Rc<RefCell<State>>);

impl SurfaceCanvas {
    fn new(document: &web_sys::Document, emitter: WebEventEmitter) -> Result<Self, JsValue> {
        let canvas: web_sys::HtmlCanvasElement =
            document.create_element("canvas")?.unchecked_into();
        canvas.set_attribute("style", "display:block;width:100%;height:100%")?;
        let state = Rc::new(RefCell::new(State {
            canvas,
            surface: None,
            attached: None,
            reported: None,
            frame_request: None,
            callback: None,
            wheel: None,
            wheel_enabled: false,
        }));
        let weak = Rc::downgrade(&state);
        let wheel =
            Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |event: web_sys::WheelEvent| {
                let Some(state) = weak.upgrade() else { return };
                let height = {
                    let state = state.borrow();
                    if !state.wheel_enabled {
                        return;
                    }
                    state.canvas.client_height()
                };
                event.prevent_default();
                let unit = match event.delta_mode() {
                    web_sys::WheelEvent::DOM_DELTA_LINE => 40.0,
                    web_sys::WheelEvent::DOM_DELTA_PAGE => f64::from(height),
                    _ => 1.0,
                };
                emitter.emit(WebNativeEvent {
                    event: "wheel".into(),
                    detail: WhiskerValue::map([
                        ("x", WhiskerValue::Float(f64::from(event.offset_x()))),
                        ("y", WhiskerValue::Float(f64::from(event.offset_y()))),
                        ("delta_x", WhiskerValue::Float(event.delta_x() * unit)),
                        ("delta_y", WhiskerValue::Float(event.delta_y() * unit)),
                        ("ctrl_key", WhiskerValue::Bool(event.ctrl_key())),
                    ]),
                });
            });
        let options = web_sys::AddEventListenerOptions::new();
        // A passive listener cannot stop the page from scrolling or zooming.
        options.set_passive(false);
        state
            .borrow()
            .canvas
            .add_event_listener_with_callback_and_add_event_listener_options(
                "wheel",
                wheel.as_ref().unchecked_ref(),
                &options,
            )?;
        state.borrow_mut().wheel = Some(wheel);
        let weak = Rc::downgrade(&state);
        let callback = Closure::<dyn FnMut(f64)>::new(move |time: f64| tick(&weak, time));
        state.borrow_mut().callback = Some(callback);
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
        // The browser throws when a pending frame calls a dropped closure.
        if let (Some(request), Some(window)) = (state.frame_request.take(), web_sys::window()) {
            let _ = window.cancel_animation_frame(request);
        }
        state.callback = None;
        if let Some(attached) = state.attached.take() {
            host::detach(attached);
        }
    }
}

fn schedule(state: &mut State) {
    if state.frame_request.is_some() {
        return;
    }
    let (Some(window), Some(callback)) = (web_sys::window(), &state.callback) else {
        return;
    };
    state.frame_request = window
        .request_animation_frame(callback.as_ref().unchecked_ref())
        .ok();
}

fn tick(weak: &Weak<RefCell<State>>, time: f64) {
    let Some(state) = weak.upgrade() else { return };
    let (id, size, attach, resize) = {
        let mut state = state.borrow_mut();
        state.frame_request = None;
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
                    |document, emitter| SurfaceCanvas::new(document, emitter.clone()),
                    |view| view.0.borrow().canvas.clone().into(),
                )
                .event("wheel")
                .prop(
                    "wheel",
                    |view, value| {
                        view.0.borrow_mut().wheel_enabled =
                            matches!(value, WhiskerValue::Bool(true));
                        Ok(())
                    },
                    |view| {
                        view.0.borrow_mut().wheel_enabled = false;
                        Ok(())
                    },
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
