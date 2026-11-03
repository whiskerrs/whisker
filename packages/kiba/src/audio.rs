use std::rc::Rc;

use glam::Vec3;

/// A sound effect or looping ambience, loaded whole and played any number of times at once.
///
/// It uses Web Audio on the Web. Native Hosts are silent for now. For music and long streams, use
/// `whisker-audio`, which plays one file at a time through the platform's media player.
///
/// Browsers keep audio off until the player first touches the page, and Kiba starts it then. Anything
/// played before that is lost.
#[derive(Clone)]
pub struct Sound(Rc<web::Shared>);

/// How a [`Sound`] plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Playback {
    /// 1 is as recorded.
    pub volume: f32,
    /// Where in the world it comes from, panning and fading around the camera. `None` is everywhere.
    pub position: Option<Vec3>,
    /// Whether it repeats until the page closes.
    pub looping: bool,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            volume: 1.0,
            position: None,
            looping: false,
        }
    }
}

impl Sound {
    /// Starts loading a compressed audio file and returns at once.
    ///
    /// `location` is a URL on the Web, as `asset!` gives. A sound played before it loads is dropped.
    /// Failures are logged.
    pub fn load(location: impl Into<String>) -> Self {
        Self(web::load(location.into()))
    }

    /// Plays it once now at full volume, from everywhere.
    pub fn play(&self) {
        self.play_with(Playback::default());
    }

    /// Plays it now as described.
    pub fn play_with(&self, playback: Playback) {
        web::play(&self.0, playback);
    }

    /// Whether it has loaded.
    pub fn is_ready(&self) -> bool {
        web::is_ready(&self.0)
    }
}

pub(crate) fn set_listener(position: Vec3, forward: Vec3, up: Vec3) {
    web::set_listener(position, forward, up);
}

/// Lets audio start, which browsers allow only during a user gesture.
#[doc(hidden)]
pub fn resume_audio() {
    web::resume();
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use glam::Vec3;
    use wasm_bindgen::JsCast;
    use web_sys::{AudioBuffer, AudioContext, PannerNode, PanningModelType};

    use super::Playback;
    use crate::renderer::log;

    #[derive(Default)]
    pub(super) struct Shared(RefCell<Option<AudioBuffer>>);

    thread_local! {
        static CONTEXT: Option<AudioContext> = AudioContext::new().ok();
    }

    fn with_context<T>(run: impl FnOnce(&AudioContext) -> T) -> Option<T> {
        CONTEXT.with(|context| context.as_ref().map(run))
    }

    pub(super) fn load(location: String) -> Rc<Shared> {
        let shared = Rc::new(Shared::default());
        let target = shared.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(reason) = fetch_and_decode(&location, &target).await {
                log(&format!("failed to load a sound from {location}: {reason}"));
            }
        });
        shared
    }

    async fn fetch_and_decode(location: &str, target: &Shared) -> Result<(), String> {
        let describe = |error: wasm_bindgen::JsValue| format!("{error:?}");
        let window = web_sys::window().ok_or("no window")?;
        let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(location))
            .await
            .map_err(describe)?
            .unchecked_into::<web_sys::Response>();
        if !response.ok() {
            return Err(format!("HTTP {}", response.status()));
        }
        let bytes = response.array_buffer().map_err(describe)?;
        let bytes = wasm_bindgen_futures::JsFuture::from(bytes)
            .await
            .map_err(describe)?;
        let decoding = with_context(|context| context.decode_audio_data(&bytes.unchecked_into()))
            .ok_or("this browser has no Web Audio")?
            .map_err(describe)?;
        let decoded = wasm_bindgen_futures::JsFuture::from(decoding)
            .await
            .map_err(describe)?;
        *target.0.borrow_mut() = Some(decoded.unchecked_into());
        Ok(())
    }

    pub(super) fn is_ready(shared: &Shared) -> bool {
        shared.0.borrow().is_some()
    }

    pub(super) fn play(shared: &Shared, playback: Playback) {
        let buffer = shared.0.borrow();
        let Some(buffer) = buffer.as_ref() else {
            return;
        };
        let played = with_context(|context| -> Result<(), wasm_bindgen::JsValue> {
            let source = context.create_buffer_source()?;
            source.set_buffer(Some(buffer));
            source.set_loop(playback.looping);
            let gain = context.create_gain()?;
            gain.gain().set_value(playback.volume);
            source.connect_with_audio_node(&gain)?;
            match playback.position {
                Some(position) => {
                    let panner = PannerNode::new(context)?;
                    panner.set_panning_model(PanningModelType::Equalpower);
                    panner.position_x().set_value(position.x);
                    panner.position_y().set_value(position.y);
                    panner.position_z().set_value(position.z);
                    gain.connect_with_audio_node(&panner)?;
                    panner.connect_with_audio_node(&context.destination())?;
                }
                None => {
                    gain.connect_with_audio_node(&context.destination())?;
                }
            }
            source.start()
        });
        if let Some(Err(error)) = played {
            log(&format!("failed to play a sound: {error:?}"));
        }
    }

    pub(super) fn set_listener(position: Vec3, forward: Vec3, up: Vec3) {
        with_context(|context| {
            let listener = context.listener();
            listener.set_position(position.x.into(), position.y.into(), position.z.into());
            listener.set_orientation(
                forward.x.into(),
                forward.y.into(),
                forward.z.into(),
                up.x.into(),
                up.y.into(),
                up.z.into(),
            );
        });
    }

    pub(super) fn resume() {
        with_context(|context| {
            let _ = context.resume();
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod web {
    use std::rc::Rc;

    use glam::Vec3;

    use super::Playback;

    pub(super) struct Shared;

    pub(super) fn load(_location: String) -> Rc<Shared> {
        Rc::new(Shared)
    }

    pub(super) fn is_ready(_shared: &Shared) -> bool {
        false
    }

    pub(super) fn play(_shared: &Shared, _playback: Playback) {}

    pub(super) fn set_listener(_position: Vec3, _forward: Vec3, _up: Vec3) {}

    pub(super) fn resume() {}
}
