//! Web Host implementation for `whisker-audio` using `HTMLAudioElement`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen_futures::{JsFuture, spawn_local};
use whisker::runtime::module::ModuleEventEmitter;
use whisker_web::wasm_bindgen::{JsCast, closure::Closure};
use whisker_web::{ModuleDefinition, WhiskerModule, WhiskerValue, web_sys};

const MODULE_NAME: &str = "whisker-audio:WhiskerAudio";

type Players = Rc<RefCell<HashMap<i64, BrowserPlayer>>>;
type MediaEventListener = (&'static str, Closure<dyn FnMut(web_sys::Event)>);

struct BrowserPlayer {
    audio: web_sys::HtmlAudioElement,
    listeners: Vec<MediaEventListener>,
}

struct AudioModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for AudioModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        let players: Players = Rc::new(RefCell::new(HashMap::new()));

        let create_players = Rc::clone(&players);
        let source_players = Rc::clone(&players);
        let play_players = Rc::clone(&players);
        let pause_players = Rc::clone(&players);
        let stop_players = Rc::clone(&players);
        let seek_players = Rc::clone(&players);
        let volume_players = Rc::clone(&players);
        let loop_players = Rc::clone(&players);
        let release_players = Rc::clone(&players);

        ModuleDefinition::new()
            .name(MODULE_NAME)
            .function("create", move |args, emitter| {
                let (id, source) = match id_and_string(args, "create", "source") {
                    Ok(values) => values,
                    Err(error) => return error,
                };
                match BrowserPlayer::new(id, source, emitter.clone()) {
                    Ok(player) => {
                        if let Some(previous) = create_players.borrow_mut().insert(id, player) {
                            drop(previous);
                        }
                        WhiskerValue::Null
                    }
                    Err(error) => WhiskerValue::Error(error),
                }
            })
            .function("setSource", move |args, emitter| {
                let (id, source) = match id_and_string(args, "setSource", "source") {
                    Ok(values) => values,
                    Err(error) => return error,
                };
                with_player(&source_players, id, "setSource", |player| {
                    player.audio.pause().ok();
                    player.audio.set_src(source);
                    player.audio.load();
                    emit_status(id, &player.audio, emitter);
                })
            })
            .function("play", move |args, emitter| {
                let id = match id_argument(args, "play") {
                    Ok(id) => id,
                    Err(error) => return error,
                };
                with_player(&play_players, id, "play", |player| {
                    if let Ok(promise) = player.audio.play() {
                        // Autoplay policy may reject this promise. The public API is
                        // fire-and-forget, so avoid an unhandled rejection and report
                        // the actual paused state through the normal status event.
                        spawn_local(async move {
                            let _ = JsFuture::from(promise).await;
                        });
                    }
                    emit_status(id, &player.audio, emitter);
                })
            })
            .function("pause", move |args, emitter| {
                let id = match id_argument(args, "pause") {
                    Ok(id) => id,
                    Err(error) => return error,
                };
                with_player(&pause_players, id, "pause", |player| {
                    player.audio.pause().ok();
                    emit_status(id, &player.audio, emitter);
                })
            })
            .function("stop", move |args, emitter| {
                let id = match id_argument(args, "stop") {
                    Ok(id) => id,
                    Err(error) => return error,
                };
                with_player(&stop_players, id, "stop", |player| {
                    player.audio.pause().ok();
                    player.audio.set_current_time(0.0);
                    emit_status(id, &player.audio, emitter);
                })
            })
            .function("seekTo", move |args, emitter| {
                let (id, seconds) = match id_and_float(args, "seekTo", "position") {
                    Ok(values) => values,
                    Err(error) => return error,
                };
                with_player(&seek_players, id, "seekTo", |player| {
                    let duration = player.audio.duration();
                    let upper = if duration.is_finite() {
                        duration.max(0.0)
                    } else {
                        f64::INFINITY
                    };
                    player.audio.set_current_time(seconds.clamp(0.0, upper));
                    emit_status(id, &player.audio, emitter);
                })
            })
            .function("setVolume", move |args, _| {
                let (id, volume) = match id_and_float(args, "setVolume", "volume") {
                    Ok(values) => values,
                    Err(error) => return error,
                };
                with_player(&volume_players, id, "setVolume", |player| {
                    player.audio.set_volume(volume.clamp(0.0, 1.0));
                })
            })
            .function("setLoop", move |args, _| {
                let (id, looping) = match id_and_bool(args, "setLoop", "loop") {
                    Ok(values) => values,
                    Err(error) => return error,
                };
                with_player(&loop_players, id, "setLoop", |player| {
                    player.audio.set_loop(looping);
                })
            })
            .function("release", move |args, _| {
                let id = match id_argument(args, "release") {
                    Ok(id) => id,
                    Err(error) => return error,
                };
                if let Some(player) = release_players.borrow_mut().remove(&id) {
                    drop(player);
                }
                WhiskerValue::Null
            })
            .event("statusChanged")
    }
}

impl BrowserPlayer {
    fn new(id: i64, source: &str, emitter: ModuleEventEmitter) -> Result<Self, String> {
        let audio = web_sys::HtmlAudioElement::new_with_src(source)
            .map_err(|error| format!("create HTMLAudioElement failed: {error:?}"))?;
        audio.set_preload("auto");

        let mut listeners = Vec::new();
        for event in [
            "loadedmetadata",
            "durationchange",
            "play",
            "playing",
            "pause",
            "timeupdate",
            "seeking",
            "seeked",
            "waiting",
            "canplay",
            "ended",
            "error",
            "emptied",
        ] {
            let event_audio = audio.clone();
            let event_emitter = emitter.clone();
            let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                emit_status(id, &event_audio, &event_emitter);
            });
            audio
                .add_event_listener_with_callback(event, listener.as_ref().unchecked_ref())
                .map_err(|error| {
                    format!("register HTMLAudioElement {event} listener: {error:?}")
                })?;
            listeners.push((event, listener));
        }

        Ok(Self { audio, listeners })
    }
}

impl Drop for BrowserPlayer {
    fn drop(&mut self) {
        for (event, listener) in &self.listeners {
            let _ = self
                .audio
                .remove_event_listener_with_callback(event, listener.as_ref().unchecked_ref());
        }
        self.audio.pause().ok();
        self.audio.set_src("");
        self.audio.load();
    }
}

fn with_player(
    players: &Players,
    id: i64,
    operation: &str,
    apply: impl FnOnce(&mut BrowserPlayer),
) -> WhiskerValue {
    let mut players = players.borrow_mut();
    let Some(player) = players.get_mut(&id) else {
        return WhiskerValue::Error(format!(
            "WhiskerAudio.{operation} references unknown player {id}"
        ));
    };
    apply(player);
    WhiskerValue::Null
}

fn emit_status(id: i64, audio: &web_sys::HtmlAudioElement, emitter: &ModuleEventEmitter) {
    let duration = audio.duration();
    emitter.emit(
        "statusChanged",
        WhiskerValue::map([
            ("playerId", WhiskerValue::Int(id)),
            ("position", WhiskerValue::Float(audio.current_time())),
            (
                "duration",
                WhiskerValue::Float(if duration.is_finite() { duration } else { 0.0 }),
            ),
            ("isLoaded", WhiskerValue::Bool(audio.ready_state() >= 1)),
            (
                "isPlaying",
                WhiskerValue::Bool(!audio.paused() && !audio.ended()),
            ),
        ]),
    );
}

fn id_argument(args: &[WhiskerValue], operation: &str) -> Result<i64, WhiskerValue> {
    let [WhiskerValue::Int(id)] = args else {
        return Err(argument_error(operation, "one player id"));
    };
    Ok(*id)
}

fn id_and_string<'a>(
    args: &'a [WhiskerValue],
    operation: &str,
    value_name: &str,
) -> Result<(i64, &'a str), WhiskerValue> {
    let [WhiskerValue::Int(id), WhiskerValue::String(value)] = args else {
        return Err(argument_error(
            operation,
            &format!("a player id and {value_name} string"),
        ));
    };
    Ok((*id, value))
}

fn id_and_float(
    args: &[WhiskerValue],
    operation: &str,
    value_name: &str,
) -> Result<(i64, f64), WhiskerValue> {
    let [WhiskerValue::Int(id), value] = args else {
        return Err(argument_error(
            operation,
            &format!("a player id and {value_name} number"),
        ));
    };
    let number = match value {
        WhiskerValue::Float(value) => *value,
        WhiskerValue::Int(value) => *value as f64,
        _ => {
            return Err(argument_error(
                operation,
                &format!("a player id and {value_name} number"),
            ));
        }
    };
    if !number.is_finite() {
        return Err(WhiskerValue::Error(format!(
            "WhiskerAudio.{operation} requires a finite {value_name}"
        )));
    }
    Ok((*id, number))
}

fn id_and_bool(
    args: &[WhiskerValue],
    operation: &str,
    value_name: &str,
) -> Result<(i64, bool), WhiskerValue> {
    let [WhiskerValue::Int(id), WhiskerValue::Bool(value)] = args else {
        return Err(argument_error(
            operation,
            &format!("a player id and {value_name} boolean"),
        ));
    };
    Ok((*id, *value))
}

fn argument_error(operation: &str, expected: &str) -> WhiskerValue {
    WhiskerValue::Error(format!("WhiskerAudio.{operation} requires {expected}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_the_portable_audio_service() {
        let definition = AudioModule::definition();
        assert_eq!(
            definition.service_definition().module_name(),
            Some(MODULE_NAME)
        );
    }

    #[test]
    fn numeric_arguments_accept_ints_and_reject_non_finite_values() {
        assert_eq!(
            id_and_float(
                &[WhiskerValue::Int(7), WhiskerValue::Int(3)],
                "seekTo",
                "position"
            ),
            Ok((7, 3.0))
        );
        assert!(matches!(
            id_and_float(
                &[WhiskerValue::Int(7), WhiskerValue::Float(f64::NAN)],
                "seekTo",
                "position"
            ),
            Err(WhiskerValue::Error(_))
        ));
    }
}
