//! Web Host implementation for Kiba.

use std::cell::Cell;
use std::rc::Rc;

use kiba::{InputEvent, Key, Renderer, Vec2, wgpu};
use wasm_bindgen_futures::spawn_local;
use whisker_web::wasm_bindgen::JsCast;
use whisker_web::wasm_bindgen::closure::Closure;
use whisker_web::{
    ModuleDefinition, WebViewDefinition, WhiskerModule, WhiskerValue, wasm_bindgen, web_sys,
};

const MODULE_NAME: &str = "kiba:Viewport";

async fn create_renderer(canvas: web_sys::HtmlCanvasElement) -> Result<Renderer, String> {
    let instance =
        wgpu::util::new_instance_with_webgpu_detection(&wgpu::InstanceDescriptor::default()).await;
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|error| error.to_string())?;
    Renderer::new(&instance, surface).await
}

fn resize_to_display(canvas: &web_sys::HtmlCanvasElement) -> (u32, u32) {
    let scale = web_sys::window()
        .expect("kiba requires a window")
        .device_pixel_ratio();
    let width = (f64::from(canvas.client_width()) * scale) as u32;
    let height = (f64::from(canvas.client_height()) * scale) as u32;
    if (canvas.width(), canvas.height()) != (width, height) {
        canvas.set_width(width);
        canvas.set_height(height);
    }
    (width, height)
}

struct Listener {
    target: web_sys::EventTarget,
    kind: &'static str,
    callback: Closure<dyn FnMut(web_sys::Event)>,
}

impl Listener {
    fn new(
        target: web_sys::EventTarget,
        kind: &'static str,
        callback: impl FnMut(web_sys::Event) + 'static,
    ) -> Self {
        let callback = Closure::<dyn FnMut(web_sys::Event)>::new(callback);
        let options = web_sys::AddEventListenerOptions::new();
        // A passive listener could not stop the page from scrolling under the wheel.
        options.set_passive(false);
        target
            .add_event_listener_with_callback_and_add_event_listener_options(
                kind,
                callback.as_ref().unchecked_ref(),
                &options,
            )
            .expect("addEventListener does not fail for these arguments");
        Self {
            target,
            kind,
            callback,
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.kind, self.callback.as_ref().unchecked_ref());
    }
}

fn is_text_entry(event: &web_sys::Event) -> bool {
    let Some(element) = event
        .target()
        .and_then(|target| target.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return false;
    };
    matches!(element.tag_name().as_str(), "INPUT" | "TEXTAREA") || element.is_content_editable()
}

fn listen_for_input(
    canvas: &web_sys::HtmlCanvasElement,
    current_game: &Rc<Cell<Option<i64>>>,
) -> Vec<Listener> {
    let window = web_sys::window().expect("kiba requires a window");
    let send = {
        let current_game = current_game.clone();
        move |event: InputEvent| {
            if let Some(game) = current_game.get() {
                kiba::send_input(game, event);
            }
        }
    };
    let position = {
        let window = window.clone();
        move |event: &web_sys::PointerEvent| {
            let scale = window.device_pixel_ratio() as f32;
            Vec2::new(event.offset_x() as f32, event.offset_y() as f32) * scale
        }
    };
    let canvas_target: web_sys::EventTarget = canvas.clone().into();
    let window_target: web_sys::EventTarget = window.clone().into();

    let pointer = |kind: &'static str, to_event: fn(i32, Vec2) -> InputEvent| {
        let send = send.clone();
        let position = position.clone();
        Listener::new(canvas_target.clone(), kind, move |event| {
            let event: web_sys::PointerEvent = event.unchecked_into();
            send(to_event(event.pointer_id(), position(&event)));
        })
    };
    let key = |kind: &'static str, to_event: fn(Key) -> InputEvent| {
        let send = send.clone();
        Listener::new(window_target.clone(), kind, move |event| {
            if is_text_entry(&event) {
                return;
            }
            let event: web_sys::KeyboardEvent = event.unchecked_into();
            if let Some(key) = Key::from_code(&event.code()) {
                send(to_event(key));
            }
        })
    };

    let capture_target = canvas.clone();
    let down_send = send.clone();
    let down_position = position.clone();
    let wheel_send = send.clone();
    let blur_send = send.clone();
    let gone = |kind: &'static str| {
        let send = send.clone();
        Listener::new(canvas_target.clone(), kind, move |event| {
            let event: web_sys::PointerEvent = event.unchecked_into();
            send(InputEvent::PointerGone {
                id: event.pointer_id(),
            });
        })
    };
    vec![
        Listener::new(canvas_target.clone(), "pointerdown", move |event| {
            let event: web_sys::PointerEvent = event.unchecked_into();
            // Capture keeps a drag alive when it leaves the canvas or crosses overlaid UI.
            let _ = capture_target.set_pointer_capture(event.pointer_id());
            down_send(InputEvent::PointerDown {
                id: event.pointer_id(),
                position: down_position(&event),
            });
        }),
        pointer("pointermove", |id, position| InputEvent::PointerMove {
            id,
            position,
        }),
        pointer("pointerup", |id, position| InputEvent::PointerUp {
            id,
            position,
        }),
        gone("pointercancel"),
        gone("pointerleave"),
        Listener::new(canvas_target.clone(), "wheel", move |event| {
            event.prevent_default();
            let event: web_sys::WheelEvent = event.unchecked_into();
            wheel_send(InputEvent::Wheel(event.delta_y() as f32));
        }),
        key("keydown", InputEvent::KeyDown),
        key("keyup", InputEvent::KeyUp),
        Listener::new(window_target.clone(), "blur", move |_| {
            blur_send(InputEvent::FocusLost);
        }),
    ]
}

struct ViewportWebView {
    canvas: web_sys::HtmlCanvasElement,
    // Cleared on unmount or game change, so a renderer still being created is not attached.
    current_game: Rc<Cell<Option<i64>>>,
    _listeners: Vec<Listener>,
}

impl ViewportWebView {
    fn set_game(&mut self, game: Option<i64>) {
        if let Some(previous) = self.current_game.replace(game) {
            kiba::detach_renderer(previous);
        }
        let Some(game) = game else {
            return;
        };
        let canvas = self.canvas.clone();
        let current_game = self.current_game.clone();
        spawn_local(async move {
            match create_renderer(canvas.clone()).await {
                Ok(renderer) if current_game.get() == Some(game) => {
                    kiba::attach_renderer(game, renderer, move || resize_to_display(&canvas));
                }
                Ok(_) => {}
                Err(error) => web_sys::console::error_1(&format!("kiba: {error}").into()),
            }
        });
    }
}

impl Drop for ViewportWebView {
    fn drop(&mut self) {
        self.set_game(None);
    }
}

struct ViewportModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for ViewportModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new().name(MODULE_NAME).view(
            WebViewDefinition::new(
                MODULE_NAME,
                |document, _| {
                    let canvas = document
                        .create_element("canvas")?
                        .dyn_into::<web_sys::HtmlCanvasElement>()?;
                    canvas.style().set_property("display", "block")?;
                    canvas.style().set_property("width", "100%")?;
                    canvas.style().set_property("height", "100%")?;
                    // Without this the browser claims touch drags for scrolling and cancels them.
                    canvas.style().set_property("touch-action", "none")?;
                    let current_game = Rc::new(Cell::new(None));
                    let listeners = listen_for_input(&canvas, &current_game);
                    Ok::<_, wasm_bindgen::JsValue>(ViewportWebView {
                        canvas,
                        current_game,
                        _listeners: listeners,
                    })
                },
                |view| view.canvas.clone().unchecked_into(),
            )
            .prop(
                "game",
                |view, value| {
                    let WhiskerValue::String(game) = value else {
                        unreachable!("Web Host validates Kiba property shapes")
                    };
                    view.set_game(game.parse().ok());
                    Ok(())
                },
                |view| {
                    view.set_game(None);
                    Ok(())
                },
            ),
        )
    }
}
