//! Low-latency sound for Whisker apps and games.
//!
//! Short voices, recorded or synthesized, mix through [`Bus`]es into a master bus that runs
//! through a compressor. Voices start on an audio clock ([`now`]), so a cue of several voices or a
//! bar of music is scheduled ahead and plays in time however frames arrive. The mixer provides
//! instruments and a clock; cue tables, sequencers, and modulation are app code.
//!
//! It uses Web Audio on the Web. Native platforms are silent for now and report
//! [`MixerState::Unavailable`]. For one long file with background playback, use `whisker-audio`.
//!
//! ```no_run
//! use whisker_mixer::{Bus, Envelope, Playback, Reverb, Sound, Tone, Wave};
//!
//! let sfx = Bus::new(0.9);
//! let room = Bus::reverb(Reverb { seconds: 2.4 }, 0.35);
//! let swing = Sound::load("sfx/swing.mp3");
//!
//! swing.play_with(Playback {
//!     pitch: 1.04,
//!     bus: sfx,
//!     send: Some((room, 0.15)),
//!     ..Playback::default()
//! });
//! Tone {
//!     wave: Wave::Triangle,
//!     hz: 880.0,
//!     to_hz: Some(440.0),
//!     envelope: Envelope { attack: 0.005, hold: 0.0, release: 0.3 },
//!     bus: sfx,
//!     at: whisker_mixer::now() + 0.5,
//!     ..Tone::default()
//! }
//! .play();
//! ```
//!
//! Everything here runs on the main thread.

use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod backend;
#[cfg(not(target_arch = "wasm32"))]
#[path = "silent.rs"]
mod backend;

/// The lowest frequency a glide reaches, since an exponential glide cannot reach zero.
const LOWEST_HZ: f32 = 20.0;

/// A group of voices sharing one volume, under master.
///
/// Muting is a bus volume of zero, which also silences looping and scheduled voices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bus(usize);

impl Bus {
    /// The bus every other bus feeds, which runs through the compressor to the output.
    pub fn master() -> Self {
        Self(0)
    }

    /// Adds a bus under master.
    pub fn new(volume: f32) -> Self {
        Self(backend::add_bus(volume.max(0.0), None))
    }

    /// Adds a reverb under master, fed through [`Playback::send`] and the voices' `send`.
    ///
    /// `volume` is how loud the reverberation is.
    pub fn reverb(reverb: Reverb, volume: f32) -> Self {
        Self(backend::add_bus(volume.max(0.0), Some(reverb)))
    }

    /// Moves the volume to `volume` over `ramp_seconds`, or at once for zero.
    pub fn set_volume(self, volume: f32, ramp_seconds: f32) {
        backend::set_bus_volume(self.0, volume.max(0.0), ramp_seconds.max(0.0));
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::master()
    }
}

/// A room's reverberation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reverb {
    /// How long the reverberation takes to die away.
    pub seconds: f32,
}

/// How master's level is held in, so loud moments do not clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compressor {
    /// The level above which sound is compressed, in decibels below full scale.
    pub threshold_db: f32,
    /// How many decibels over the threshold become one.
    pub ratio: f32,
}

impl Default for Compressor {
    fn default() -> Self {
        Self {
            threshold_db: -14.0,
            ratio: 4.0,
        }
    }
}

/// Replaces master's compressor settings.
pub fn set_compressor(compressor: Compressor) {
    backend::set_compressor(compressor);
}

/// Whether sound is coming out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerState {
    /// The browser allows sound only after the user first touches the page or presses a key.
    Locked,
    /// Sound plays and the clock moves.
    Running,
    /// The page is hidden or the system took the audio away.
    Suspended,
    /// This platform has no mixer yet.
    Unavailable,
}

/// Whether sound is coming out. The clock stands still unless this is [`MixerState::Running`], so
/// music should start once it is.
pub fn state() -> MixerState {
    backend::state()
}

/// The audio clock in seconds, which [`Playback::at`], [`Tone::at`], and [`Noise::at`] schedule
/// against.
pub fn now() -> f64 {
    backend::now()
}

/// Places the listener for voices with a [`Playback::position`].
pub fn set_listener(position: [f32; 3], forward: [f32; 3], up: [f32; 3]) {
    backend::set_listener(position, forward, up);
}

/// A recorded clip, loaded whole and played any number of times at once.
#[derive(Clone)]
pub struct Sound(Rc<backend::Clip>);

impl Sound {
    /// Starts loading a compressed audio file and returns at once.
    ///
    /// `location` is a URL on the Web, as `asset!` gives. A sound played before it loads is
    /// dropped. Failures are logged.
    pub fn load(location: impl Into<String>) -> Self {
        Self(backend::load(location.into()))
    }

    /// Whether it has loaded.
    pub fn is_ready(&self) -> bool {
        backend::is_ready(&self.0)
    }

    /// Plays it once now on master.
    pub fn play(&self) -> Voice {
        self.play_with(Playback::default())
    }

    /// Plays it as described.
    pub fn play_with(&self, playback: Playback) -> Voice {
        Voice(backend::play(&self.0, playback))
    }
}

/// How a [`Sound`] plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Playback {
    /// 1 is as recorded.
    pub volume: f32,
    /// The playback rate, which raises pitch and shortens the sound together. 1 is as recorded.
    pub pitch: f32,
    /// Whether it repeats until [`Voice::stop`].
    pub looping: bool,
    pub bus: Bus,
    /// A second bus, usually a reverb, that hears the voice at this volume as well.
    pub send: Option<(Bus, f32)>,
    /// When it starts on the audio clock. A time already past is now.
    pub at: f64,
    /// Where in the world it comes from, panning and fading around the listener. `None` is
    /// everywhere.
    pub position: Option<[f32; 3]>,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            bus: Bus::master(),
            send: None,
            at: 0.0,
            position: None,
        }
    }
}

/// A synthesized voice's shape over time, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    /// The rise from silence to full volume.
    pub attack: f32,
    /// How long full volume holds. Infinity holds until [`Voice::stop`].
    pub hold: f32,
    /// The fall from full volume to silence.
    pub release: f32,
}

impl Envelope {
    /// The whole length, or `None` for a voice held until stopped.
    pub fn length(&self) -> Option<f32> {
        let length = self.attack.max(0.0) + self.hold.max(0.0) + self.release.max(0.0);
        length.is_finite().then_some(length)
    }
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            attack: 0.005,
            hold: 0.0,
            release: 0.2,
        }
    }
}

/// An oscillator's waveform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wave {
    #[default]
    Sine,
    Triangle,
    Sawtooth,
    Square,
}

/// Which frequencies a [`Filter`] keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterKind {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
}

/// A biquad filter whose frequency may glide over the voice's envelope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    pub kind: FilterKind,
    pub hz: f32,
    /// Where the frequency glides to by the end of the envelope. `None` holds `hz`.
    pub to_hz: Option<f32>,
    /// How narrow the filter is around `hz`.
    pub q: f32,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            kind: FilterKind::Lowpass,
            hz: 1000.0,
            to_hz: None,
            q: 1.0,
        }
    }
}

/// A synthesized tone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    pub wave: Wave,
    pub hz: f32,
    /// Where the pitch glides to, exponentially, by the end of the envelope. `None` holds `hz`.
    pub to_hz: Option<f32>,
    /// The volume at the envelope's peak.
    pub volume: f32,
    pub envelope: Envelope,
    pub filter: Option<Filter>,
    pub bus: Bus,
    /// A second bus, usually a reverb, that hears the voice at this volume as well.
    pub send: Option<(Bus, f32)>,
    /// When it starts on the audio clock. A time already past is now.
    pub at: f64,
}

impl Default for Tone {
    fn default() -> Self {
        Self {
            wave: Wave::Sine,
            hz: 440.0,
            to_hz: None,
            volume: 1.0,
            envelope: Envelope::default(),
            filter: None,
            bus: Bus::master(),
            send: None,
            at: 0.0,
        }
    }
}

impl Tone {
    pub fn play(&self) -> Voice {
        Voice(backend::play_tone(self))
    }
}

/// Synthesized white noise, read from a random point of a shared buffer, usually shaped by a
/// filter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    /// The volume at the envelope's peak.
    pub volume: f32,
    pub envelope: Envelope,
    pub filter: Option<Filter>,
    pub bus: Bus,
    /// A second bus, usually a reverb, that hears the voice at this volume as well.
    pub send: Option<(Bus, f32)>,
    /// When it starts on the audio clock. A time already past is now.
    pub at: f64,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            volume: 1.0,
            envelope: Envelope::default(),
            filter: None,
            bus: Bus::master(),
            send: None,
            at: 0.0,
        }
    }
}

impl Noise {
    pub fn play(&self) -> Voice {
        Voice(backend::play_noise(self))
    }
}

/// One playing voice. Dropping it lets the voice play on to its end, or forever if it loops or
/// holds.
#[derive(Default)]
pub struct Voice(Option<backend::Nodes>);

impl Voice {
    /// Fades it out over `release_seconds` and ends it.
    pub fn stop(&self, release_seconds: f32) {
        if let Some(nodes) = &self.0 {
            backend::stop(nodes, release_seconds.max(0.0));
        }
    }

    /// Moves its volume to `volume` over `ramp_seconds`, or at once for zero.
    pub fn set_volume(&self, volume: f32, ramp_seconds: f32) {
        if let Some(nodes) = &self.0 {
            backend::set_voice_volume(nodes, volume.max(0.0), ramp_seconds.max(0.0));
        }
    }
}

/// Where a glide toward `hz` ends, kept above zero.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn glide_target(hz: f32) -> f32 {
    hz.max(LOWEST_HZ)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_envelope_held_forever_has_no_length() {
        let held = Envelope {
            hold: f32::INFINITY,
            ..Envelope::default()
        };
        assert_eq!(held.length(), None);
        let pad = Envelope {
            attack: 1.4,
            hold: 2.2,
            release: 2.7,
        };
        assert!((pad.length().unwrap() - 6.3).abs() < 1e-6);
    }

    #[test]
    fn negative_envelope_stages_count_as_zero() {
        let envelope = Envelope {
            attack: -1.0,
            hold: 0.5,
            release: -0.2,
        };
        assert_eq!(envelope.length(), Some(0.5));
    }

    #[test]
    fn glides_stop_short_of_zero() {
        assert_eq!(glide_target(0.0), LOWEST_HZ);
        assert_eq!(glide_target(-5.0), LOWEST_HZ);
        assert_eq!(glide_target(22.0), 22.0);
    }

    #[test]
    fn a_native_mixer_is_silent_but_hands_out_buses() {
        assert_eq!(state(), MixerState::Unavailable);
        let music = Bus::new(0.28);
        let room = Bus::reverb(Reverb { seconds: 2.4 }, 0.35);
        assert_ne!(music, Bus::master());
        assert_ne!(music, room);
        let voice = Tone::default().play();
        voice.set_volume(0.5, 0.1);
        voice.stop(0.1);
        assert!(!Sound::load("missing.mp3").is_ready());
    }
}
