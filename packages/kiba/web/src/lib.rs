//! Web Host implementation for Kiba.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use kiba::{Renderer, wgpu};
use wasm_bindgen_futures::spawn_local;
use whisker_web::wasm_bindgen::JsCast;
use whisker_web::wasm_bindgen::closure::Closure;
use whisker_web::{ModuleDefinition, WebViewDefinition, WhiskerModule, wasm_bindgen, web_sys};

const MODULE_NAME: &str = "kiba:Viewport";

async fn create_renderer(canvas: web_sys::HtmlCanvasElement) -> Result<Renderer, String> {
    let instance =
        wgpu::util::new_instance_with_webgpu_detection(&wgpu::InstanceDescriptor::default()).await;
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|error| error.to_string())?;
    Renderer::new(&instance, surface).await
}

type FrameCallback = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

struct ViewportWebView {
    canvas: web_sys::HtmlCanvasElement,
    stopped: Rc<Cell<bool>>,
}

impl ViewportWebView {
    fn new(canvas: web_sys::HtmlCanvasElement) -> Self {
        let stopped = Rc::new(Cell::new(false));
        let view = Self {
            canvas: canvas.clone(),
            stopped: stopped.clone(),
        };
        spawn_local(async move {
            match create_renderer(canvas.clone()).await {
                Ok(renderer) => run_frame_loop(canvas, renderer, stopped),
                Err(error) => {
                    web_sys::console::error_1(&format!("kiba: {error}").into());
                }
            }
        });
        view
    }
}

impl Drop for ViewportWebView {
    fn drop(&mut self) {
        self.stopped.set(true);
    }
}

fn run_frame_loop(
    canvas: web_sys::HtmlCanvasElement,
    mut renderer: Renderer,
    stopped: Rc<Cell<bool>>,
) {
    let callback: FrameCallback = Rc::new(RefCell::new(None));
    let next = callback.clone();
    *callback.borrow_mut() = Some(Closure::new(move || {
        if stopped.get() {
            next.borrow_mut().take();
            return;
        }
        let window = web_sys::window().expect("kiba requires a window");
        let scale = window.device_pixel_ratio();
        let width = (f64::from(canvas.client_width()) * scale) as u32;
        let height = (f64::from(canvas.client_height()) * scale) as u32;
        if (canvas.width(), canvas.height()) != (width, height) {
            canvas.set_width(width);
            canvas.set_height(height);
        }
        renderer.render(width, height);
        if let Some(callback) = next.borrow().as_ref() {
            window
                .request_animation_frame(callback.as_ref().unchecked_ref())
                .expect("requestAnimationFrame failed");
        }
    }));
    let first = callback.borrow();
    web_sys::window()
        .expect("kiba requires a window")
        .request_animation_frame(first.as_ref().expect("set above").as_ref().unchecked_ref())
        .expect("requestAnimationFrame failed");
}

struct ViewportModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for ViewportModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .view(WebViewDefinition::new(
                MODULE_NAME,
                |document, _| {
                    let canvas = document
                        .create_element("canvas")?
                        .dyn_into::<web_sys::HtmlCanvasElement>()?;
                    canvas.style().set_property("display", "block")?;
                    canvas.style().set_property("width", "100%")?;
                    canvas.style().set_property("height", "100%")?;
                    Ok::<_, wasm_bindgen::JsValue>(ViewportWebView::new(canvas))
                },
                |view| view.canvas.clone().unchecked_into(),
            ))
    }
}
