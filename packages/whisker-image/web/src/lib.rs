//! Web Host implementation for `whisker-image`.

use whisker_web::wasm_bindgen::JsCast;
use whisker_web::wasm_bindgen::closure::Closure;
use whisker_web::{
    ModuleDefinition, WebNativeEvent, WebViewDefinition, WhiskerModule, WhiskerValue, wasm_bindgen,
    web_sys,
};

const MODULE_NAME: &str = "whisker-image:Image";

struct ImageWebView {
    image: web_sys::HtmlImageElement,
    emitter: whisker_web::WebEventEmitter,
    src: String,
    headers: String,
    load: Closure<dyn FnMut(web_sys::Event)>,
    error: Closure<dyn FnMut(web_sys::Event)>,
}

impl Drop for ImageWebView {
    fn drop(&mut self) {
        let _ = self
            .image
            .remove_event_listener_with_callback("load", self.load.as_ref().unchecked_ref());
        let _ = self
            .image
            .remove_event_listener_with_callback("error", self.error.as_ref().unchecked_ref());
    }
}

struct ImageModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for ImageModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .async_function("prefetch", |args, promise, _| {
                let result = prefetch(args);
                match result {
                    Ok(()) => promise.resolve(WhiskerValue::Null),
                    Err(error) => promise.reject(error),
                }
            })
            .view(
                WebViewDefinition::new(
                    MODULE_NAME,
                    |document, emitter| {
                        let image = document
                            .create_element("img")?
                            .dyn_into::<web_sys::HtmlImageElement>()?;
                        image.set_decoding("async");
                        image.style().set_property("width", "100%")?;
                        image.style().set_property("height", "100%")?;

                        let loaded_image = image.clone();
                        let loaded_emitter = emitter.clone();
                        let load = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                            loaded_emitter.emit(WebNativeEvent {
                                event: "load".into(),
                                detail: WhiskerValue::map([
                                    (
                                        "width",
                                        WhiskerValue::Float(loaded_image.natural_width() as f64),
                                    ),
                                    (
                                        "height",
                                        WhiskerValue::Float(loaded_image.natural_height() as f64),
                                    ),
                                    ("error", WhiskerValue::String(String::new())),
                                ]),
                            });
                        });
                        image.add_event_listener_with_callback(
                            "load",
                            load.as_ref().unchecked_ref(),
                        )?;

                        let error_emitter = emitter.clone();
                        let error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                            emit_error(&error_emitter, "browser failed to load the image");
                        });
                        image.add_event_listener_with_callback(
                            "error",
                            error.as_ref().unchecked_ref(),
                        )?;
                        Ok(ImageWebView {
                            image,
                            emitter,
                            src: String::new(),
                            headers: String::new(),
                            load,
                            error,
                        })
                    },
                    |view| view.image.clone().unchecked_into(),
                )
                .prop(
                    "src",
                    |view, value| {
                        let WhiskerValue::String(src) = value else {
                            return Err(js_error("Image src property must be a string"));
                        };
                        view.src.clone_from(src);
                        if view.headers.trim().is_empty() {
                            view.image.set_src(src);
                        } else {
                            view.image.set_src("");
                            emit_error(
                                &view.emitter,
                                "Web Image does not yet support custom request headers",
                            );
                        }
                        Ok(())
                    },
                    |view| {
                        view.src.clear();
                        view.image.set_src("");
                        Ok(())
                    },
                )
                .prop(
                    "mode",
                    |view, value| {
                        let WhiskerValue::String(mode) = value else {
                            return Err(js_error("Image mode property must be a string"));
                        };
                        set_mode(&view.image, mode)
                    },
                    |view| set_mode(&view.image, "aspectFill"),
                )
                .prop(
                    "headers",
                    |view, value| {
                        let WhiskerValue::String(headers) = value else {
                            return Err(js_error("Image headers property must be a string"));
                        };
                        view.headers.clone_from(headers);
                        if view.headers.trim().is_empty() {
                            view.image.set_src(&view.src);
                        } else {
                            view.image.set_src("");
                            emit_error(
                                &view.emitter,
                                "Web Image does not yet support custom request headers",
                            );
                        }
                        Ok(())
                    },
                    |view| {
                        view.headers.clear();
                        view.image.set_src(&view.src);
                        Ok(())
                    },
                )
                .event("load")
                .event("error"),
            )
    }
}

fn set_mode(image: &web_sys::HtmlImageElement, mode: &str) -> Result<(), wasm_bindgen::JsValue> {
    let fit = match mode {
        "aspectFill" => "cover",
        "aspectFit" => "contain",
        "scaleToFill" => "fill",
        "center" => "none",
        _ => return Err(js_error("unsupported Image mode")),
    };
    image.style().set_property("object-fit", fit)
}

fn prefetch(args: &[WhiskerValue]) -> Result<(), String> {
    let [WhiskerValue::Array(urls), WhiskerValue::String(headers)] = args else {
        return Err("Image.prefetch requires an URL array and headers string".into());
    };
    if !headers.trim().is_empty() {
        return Err("Web Image prefetch does not yet support custom request headers".into());
    }
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| "browser Document is unavailable".to_owned())?;
    let head = document
        .head()
        .ok_or_else(|| "browser document has no head".to_owned())?;
    for url in urls {
        let WhiskerValue::String(url) = url else {
            return Err("Image.prefetch URL entries must be strings".into());
        };
        let link = document.create_element("link").map_err(debug_js)?;
        link.set_attribute("rel", "prefetch").map_err(debug_js)?;
        link.set_attribute("as", "image").map_err(debug_js)?;
        link.set_attribute("href", url).map_err(debug_js)?;
        head.append_child(&link).map_err(debug_js)?;
    }
    Ok(())
}

fn emit_error(emitter: &whisker_web::WebEventEmitter, message: &str) {
    emitter.emit(WebNativeEvent {
        event: "error".into(),
        detail: WhiskerValue::map([
            ("width", WhiskerValue::Float(0.0)),
            ("height", WhiskerValue::Float(0.0)),
            ("error", WhiskerValue::String(message.to_owned())),
        ]),
    });
}

fn js_error(message: &str) -> wasm_bindgen::JsValue {
    wasm_bindgen::JsValue::from_str(message)
}

fn debug_js(error: wasm_bindgen::JsValue) -> String {
    format!("browser image prefetch failed: {error:?}")
}
