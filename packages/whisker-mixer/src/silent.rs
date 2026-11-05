use std::cell::Cell;
use std::rc::Rc;

use crate::{Compressor, MixerState, Noise, Playback, Reverb, Tone};

pub(crate) struct Clip;

pub(crate) enum Nodes {}

thread_local! {
    static BUSES: Cell<usize> = const { Cell::new(1) };
}

pub(crate) fn add_bus(_volume: f32, _reverb: Option<Reverb>) -> usize {
    BUSES.with(|count| {
        let index = count.get();
        count.set(index + 1);
        index
    })
}

pub(crate) fn set_bus_volume(_bus: usize, _volume: f32, _ramp_seconds: f32) {}

pub(crate) fn set_compressor(_compressor: Compressor) {}

pub(crate) fn state() -> MixerState {
    MixerState::Unavailable
}

pub(crate) fn now() -> f64 {
    0.0
}

pub(crate) fn set_listener(_position: [f32; 3], _forward: [f32; 3], _up: [f32; 3]) {}

pub(crate) fn load(_location: String) -> Rc<Clip> {
    Rc::new(Clip)
}

pub(crate) fn is_ready(_clip: &Clip) -> bool {
    false
}

pub(crate) fn play(_clip: &Clip, _playback: Playback) -> Option<Nodes> {
    None
}

pub(crate) fn play_tone(_tone: &Tone) -> Option<Nodes> {
    None
}

pub(crate) fn play_noise(_noise: &Noise) -> Option<Nodes> {
    None
}

pub(crate) fn stop(nodes: &Nodes, _release_seconds: f32) {
    match *nodes {}
}

pub(crate) fn set_voice_volume(nodes: &Nodes, _volume: f32, _ramp_seconds: f32) {
    match *nodes {}
}
