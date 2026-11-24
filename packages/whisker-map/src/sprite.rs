//! A style's sprite sheets: icon images and where they sit in each sheet.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::tiles::log;

#[derive(Clone, Copy, Debug)]
pub(crate) struct SpriteImage {
    pub sheet: usize,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_ratio: f32,
}

pub(crate) struct SpriteSheet {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct SpriteStore {
    pub sheets: RefCell<Vec<Rc<SpriteSheet>>>,
    images: RefCell<HashMap<String, SpriteImage>>,
    started: Cell<bool>,
    loading: Cell<usize>,
}

impl SpriteStore {
    pub fn is_loading(&self) -> bool {
        self.loading.get() > 0
    }

    pub fn image(&self, name: &str) -> Option<SpriteImage> {
        self.images.borrow().get(name).copied()
    }

    /// Fetches every sheet once, at double resolution on dense screens.
    pub fn load(
        self: &Rc<Self>,
        sprites: &[(String, String)],
        scale: f32,
        on_change: Rc<dyn Fn()>,
    ) {
        if self.started.replace(true) {
            return;
        }
        let suffix = if scale > 1.0 { "@2x" } else { "" };
        for (id, url) in sprites {
            let prefix = if id == "default" {
                String::new()
            } else {
                format!("{id}:")
            };
            let (base, query) = match url.split_once('?') {
                Some((base, query)) => (base.to_string(), format!("?{query}")),
                None => (url.clone(), String::new()),
            };
            let json_url = format!("{base}{suffix}.json{query}");
            let png_url = format!("{base}{suffix}.png{query}");
            let (weak, store, settled) =
                (Rc::downgrade(self), Rc::downgrade(self), on_change.clone());
            self.loading.set(self.loading.get() + 1);
            whisker::spawn_local(async move {
                load_sheet(weak, prefix, json_url, png_url).await;
                if let Some(store) = store.upgrade() {
                    store.loading.set(store.loading.get() - 1);
                    settled();
                }
            });
        }
    }
}

async fn load_sheet(
    weak: std::rc::Weak<SpriteStore>,
    prefix: String,
    json_url: String,
    png_url: String,
) {
    let (json, png) = (
        crate::net::fetch(json_url.clone(), None).await,
        crate::net::fetch(png_url.clone(), None).await,
    );
    let (Ok(Some(json)), Ok(Some(png))) = (json, png) else {
        log(&format!(
            "whisker-map: sprite {json_url} could not be loaded"
        ));
        return;
    };
    let Ok(index) = serde_json::from_slice::<serde_json::Value>(&json) else {
        log(&format!("whisker-map: sprite {json_url} is not JSON"));
        return;
    };
    let image =
        crate::work::run(move || image::load_from_memory(&png).map(|i| i.into_rgba8())).await;
    let Ok(image) = image else {
        log(&format!("whisker-map: sprite {png_url} is not an image"));
        return;
    };
    let Some(store) = weak.upgrade() else { return };
    let (width, height) = image.dimensions();
    let sheet = {
        let mut sheets = store.sheets.borrow_mut();
        sheets.push(Rc::new(SpriteSheet {
            width,
            height,
            pixels: image.into_raw(),
        }));
        sheets.len() - 1
    };
    if let Some(entries) = index.as_object() {
        let mut images = store.images.borrow_mut();
        for (name, entry) in entries {
            let n = |key: &str| entry.get(key).and_then(serde_json::Value::as_f64);
            let (Some(x), Some(y), Some(w), Some(h)) = (n("x"), n("y"), n("width"), n("height"))
            else {
                continue;
            };
            images.insert(
                format!("{prefix}{name}"),
                SpriteImage {
                    sheet,
                    x: x as u32,
                    y: y as u32,
                    width: w as u32,
                    height: h as u32,
                    pixel_ratio: n("pixelRatio").unwrap_or(1.0) as f32,
                },
            );
        }
    }
}
