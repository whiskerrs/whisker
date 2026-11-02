//! Web Host implementation for Kiba.

use std::cell::Cell;
use std::rc::Rc;

use kiba::{Renderer, wgpu};
use wasm_bindgen_futures::spawn_local;
use whisker_web::wasm_bindgen::JsCast;
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

struct ViewportWebView {
    canvas: web_sys::HtmlCanvasElement,
    // Cleared on unmount or game change, so a renderer still being created is not attached.
    current_game: Rc<Cell<Option<i64>>>,
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
                    Ok::<_, wasm_bindgen::JsValue>(ViewportWebView {
                        canvas,
                        current_game: Rc::new(Cell::new(None)),
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
