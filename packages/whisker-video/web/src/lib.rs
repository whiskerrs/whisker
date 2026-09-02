//! Web Host implementation for `whisker-video` using `HTMLVideoElement`.

use wasm_bindgen_futures::{JsFuture, spawn_local};
use whisker_web::wasm_bindgen::JsCast;
use whisker_web::{
    ModuleDefinition, WebViewDefinition, WhiskerModule, WhiskerValue, wasm_bindgen, web_sys,
};

const MODULE_NAME: &str = "whisker-video:Video";

struct VideoWebView {
    video: web_sys::HtmlVideoElement,
}

impl VideoWebView {
    fn set_source(&self, source: &str) {
        self.video.set_src(source);
        self.video.load();
        if !source.is_empty() {
            play(&self.video);
        }
    }
}

impl Drop for VideoWebView {
    fn drop(&mut self) {
        self.video.pause().ok();
        self.video.set_src("");
        self.video.load();
    }
}

struct VideoModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for VideoModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new().name(MODULE_NAME).view(
            WebViewDefinition::new(
                MODULE_NAME,
                |document, _| {
                    let video = document
                        .create_element("video")?
                        .dyn_into::<web_sys::HtmlVideoElement>()?;
                    video.set_autoplay(true);
                    video.set_controls(false);
                    video.set_attribute("playsinline", "")?;
                    video.style().set_property("width", "100%")?;
                    video.style().set_property("height", "100%")?;
                    video.style().set_property("object-fit", "cover")?;
                    Ok(VideoWebView { video })
                },
                |view| view.video.clone().unchecked_into(),
            )
            .prop(
                "src",
                |view, value| {
                    let WhiskerValue::String(source) = value else {
                        return Err(js_error("Video src property must be a string"));
                    };
                    view.set_source(source);
                    Ok(())
                },
                |view| {
                    view.set_source("");
                    Ok(())
                },
            )
            .command("play", |view, parameters| {
                require_null("play", parameters).map_err(|error| js_error(&error))?;
                play(&view.video);
                Ok(())
            })
            .command("pause", |view, parameters| {
                require_null("pause", parameters).map_err(|error| js_error(&error))?;
                view.video.pause()?;
                Ok(())
            })
            .command("seek", |view, parameters| {
                let seconds = match parameters {
                    WhiskerValue::Float(value) => *value,
                    WhiskerValue::Int(value) => *value as f64,
                    _ => return Err(js_error("Video seek command requires a number")),
                };
                if !seconds.is_finite() {
                    return Err(js_error("Video seek command requires a finite number"));
                }
                let duration = view.video.duration();
                let upper = if duration.is_finite() {
                    duration.max(0.0)
                } else {
                    f64::INFINITY
                };
                view.video.set_current_time(seconds.clamp(0.0, upper));
                Ok(())
            }),
        )
    }
}

fn play(video: &web_sys::HtmlVideoElement) {
    if let Ok(promise) = video.play() {
        // Browser autoplay policy can reject this promise. Playback remains
        // controllable from a user gesture, and the rejected autoplay attempt
        // must not become an unhandled JavaScript rejection.
        spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

fn require_null(command: &str, parameters: &WhiskerValue) -> Result<(), String> {
    if matches!(parameters, WhiskerValue::Null) {
        Ok(())
    } else {
        Err(format!(
            "Video {command} command does not accept parameters"
        ))
    }
}

fn js_error(message: &str) -> wasm_bindgen::JsValue {
    wasm_bindgen::JsValue::from_str(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_one_video_view() {
        let definition = VideoModule::definition();
        assert_eq!(
            definition.service_definition().module_name(),
            Some(MODULE_NAME)
        );
        assert_eq!(definition.factories().len(), 1);
    }

    #[test]
    fn parameterless_commands_reject_payloads() {
        assert!(require_null("play", &WhiskerValue::Null).is_ok());
        assert!(require_null("play", &WhiskerValue::Int(1)).is_err());
    }
}
