use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    AudioBuffer, AudioContext, AudioContextState, AudioNode, AudioParam, AudioScheduledSourceNode,
    BiquadFilterNode, BiquadFilterType, DynamicsCompressorNode, GainNode, OscillatorType,
    PannerNode, PanningModelType,
};

use crate::{
    Bus, Compressor, Envelope, Filter, FilterKind, MixerState, Noise, Playback, Reverb, Tone, Wave,
    glide_target,
};

/// The quietest gain an exponential ramp starts from and fades to, since it cannot reach zero.
const FLOOR: f32 = 0.0001;
/// How long the shared noise buffer is.
const NOISE_SECONDS: f32 = 2.0;
/// How long a source runs past its envelope, so its last ramp finishes before it stops.
const TAIL_SECONDS: f64 = 0.05;
/// The gestures browsers accept for starting audio; pointerdown alone does not count for touches.
const GESTURES: [&str; 5] = ["pointerdown", "pointerup", "touchend", "keydown", "click"];

type Result<T> = std::result::Result<T, JsValue>;

pub(crate) struct Clip(RefCell<Option<AudioBuffer>>);

pub(crate) struct Nodes {
    level: GainNode,
    sources: Vec<AudioScheduledSourceNode>,
}

struct BusNodes {
    input: AudioNode,
    output: GainNode,
}

struct Mixer {
    context: AudioContext,
    compressor: DynamicsCompressorNode,
    buses: RefCell<Vec<BusNodes>>,
    noise: AudioBuffer,
    has_run: Rc<Cell<bool>>,
}

thread_local! {
    static MIXER: Option<Mixer> = match Mixer::new() {
        Ok(mixer) => Some(mixer),
        Err(error) => {
            warn(&format!("no audio: {error:?}"));
            None
        }
    };
}

fn with_mixer<T>(run: impl FnOnce(&Mixer) -> Result<T>) -> Option<T> {
    MIXER.with(|mixer| {
        let mixer = mixer.as_ref()?;
        match run(mixer) {
            Ok(value) => Some(value),
            Err(error) => {
                warn(&format!("audio failed: {error:?}"));
                None
            }
        }
    })
}

fn warn(message: &str) {
    web_sys::console::warn_1(&format!("[whisker-mixer] {message}").into());
}

fn random() -> f32 {
    js_sys::Math::random() as f32
}

impl Mixer {
    fn new() -> Result<Self> {
        let context = AudioContext::new()?;
        let compressor = context.create_dynamics_compressor()?;
        compressor.connect_with_audio_node(&context.destination())?;
        let master = context.create_gain()?;
        master.connect_with_audio_node(&compressor)?;
        let mixer = Self {
            noise: noise_buffer(&context)?,
            buses: RefCell::new(vec![BusNodes {
                input: master.clone().into(),
                output: master,
            }]),
            compressor,
            has_run: Rc::new(Cell::new(false)),
            context,
        };
        mixer.apply_compressor(Compressor::default());
        mixer.listen()?;
        Ok(mixer)
    }

    fn listen(&self) -> Result<()> {
        let window = web_sys::window().ok_or("no window")?;
        let context = self.context.clone();
        let resume = Closure::<dyn FnMut()>::new(move || {
            if context.state() != AudioContextState::Running {
                let _ = context.resume();
            }
        });
        for gesture in GESTURES {
            window.add_event_listener_with_callback_and_bool(
                gesture,
                resume.as_ref().unchecked_ref(),
                true,
            )?;
        }
        resume.forget();

        let document = window.document().ok_or("no document")?;
        let context = self.context.clone();
        let watched = document.clone();
        let visibility = Closure::<dyn FnMut()>::new(move || {
            let _ = if watched.hidden() {
                context.suspend()
            } else {
                context.resume()
            };
        });
        document.add_event_listener_with_callback(
            "visibilitychange",
            visibility.as_ref().unchecked_ref(),
        )?;
        visibility.forget();

        let context = self.context.clone();
        let has_run = self.has_run.clone();
        let changed = Closure::<dyn FnMut()>::new(move || {
            if context.state() == AudioContextState::Running {
                has_run.set(true);
            }
        });
        self.context
            .set_onstatechange(Some(changed.as_ref().unchecked_ref()));
        changed.forget();
        Ok(())
    }

    fn apply_compressor(&self, compressor: Compressor) {
        self.compressor
            .threshold()
            .set_value(compressor.threshold_db);
        self.compressor.ratio().set_value(compressor.ratio.max(1.0));
    }

    fn now(&self) -> f64 {
        self.context.current_time()
    }

    /// The start time for `at`, which may not lie in the past or its ramps would already be over.
    fn start(&self, at: f64) -> f64 {
        at.max(self.now())
    }

    /// Connects a voice's level to its bus and send, through a panner when it has a position.
    fn route(
        &self,
        level: &GainNode,
        bus: Bus,
        send: Option<(Bus, f32)>,
        position: Option<[f32; 3]>,
    ) -> Result<()> {
        let buses = self.buses.borrow();
        let input = |bus: Bus| buses.get(bus.0).unwrap_or(&buses[0]).input.clone();
        let out: AudioNode = match position {
            Some([x, y, z]) => {
                let panner = PannerNode::new(&self.context)?;
                panner.set_panning_model(PanningModelType::Equalpower);
                panner.position_x().set_value(x);
                panner.position_y().set_value(y);
                panner.position_z().set_value(z);
                level.connect_with_audio_node(&panner)?;
                panner.into()
            }
            None => level.clone().into(),
        };
        out.connect_with_audio_node(&input(bus))?;
        if let Some((bus, volume)) = send {
            let gain = self.context.create_gain()?;
            gain.gain().set_value(volume.max(0.0));
            out.connect_with_audio_node(&gain)?;
            gain.connect_with_audio_node(&input(bus))?;
        }
        Ok(())
    }

    fn level(&self, volume: f32) -> Result<GainNode> {
        let level = self.context.create_gain()?;
        level.gain().set_value(volume.max(0.0));
        Ok(level)
    }

    fn filter(&self, filter: &Filter, start: f64, length: Option<f32>) -> Result<BiquadFilterNode> {
        let node = self.context.create_biquad_filter()?;
        node.set_type(match filter.kind {
            FilterKind::Lowpass => BiquadFilterType::Lowpass,
            FilterKind::Highpass => BiquadFilterType::Highpass,
            FilterKind::Bandpass => BiquadFilterType::Bandpass,
        });
        node.q().set_value(filter.q);
        glide(&node.frequency(), filter.hz, filter.to_hz, start, length)?;
        Ok(node)
    }

    /// Puts a synthesized source through its filter, envelope, and level, and starts it.
    fn voice(
        &self,
        source: AudioScheduledSourceNode,
        start: f64,
        begin: impl FnOnce() -> Result<()>,
        shape: Shape,
    ) -> Result<Nodes> {
        let envelope = self.context.create_gain()?;
        shape_envelope(&envelope.gain(), &shape.envelope, start)?;
        match &shape.filter {
            Some(filter) => {
                let node = self.filter(filter, start, shape.envelope.length())?;
                source.connect_with_audio_node(&node)?;
                node.connect_with_audio_node(&envelope)?;
            }
            None => {
                source.connect_with_audio_node(&envelope)?;
            }
        }
        let level = self.level(shape.volume)?;
        envelope.connect_with_audio_node(&level)?;
        self.route(&level, shape.bus, shape.send, None)?;
        begin()?;
        if let Some(length) = shape.envelope.length() {
            source.stop_with_when(start + f64::from(length) + TAIL_SECONDS)?;
        }
        Ok(Nodes {
            level,
            sources: vec![source],
        })
    }
}

/// What every synthesized voice shares.
struct Shape {
    volume: f32,
    envelope: Envelope,
    filter: Option<Filter>,
    bus: Bus,
    send: Option<(Bus, f32)>,
}

fn noise_buffer(context: &AudioContext) -> Result<AudioBuffer> {
    let rate = context.sample_rate();
    let samples: Vec<f32> = (0..(rate * NOISE_SECONDS) as usize)
        .map(|_| random() * 2.0 - 1.0)
        .collect();
    let buffer = context.create_buffer(1, samples.len() as u32, rate)?;
    buffer.copy_to_channel(&samples, 0)?;
    Ok(buffer)
}

/// Decaying stereo noise, the impulse response of a room that rings for `seconds`.
fn impulse_response(context: &AudioContext, seconds: f32) -> Result<AudioBuffer> {
    let rate = context.sample_rate();
    let length = ((rate * seconds.max(0.01)) as usize).max(1);
    let buffer = context.create_buffer(2, length as u32, rate)?;
    for channel in 0..2 {
        let samples: Vec<f32> = (0..length)
            .map(|i| (random() * 2.0 - 1.0) * (1.0 - i as f32 / length as f32).powi(3))
            .collect();
        buffer.copy_to_channel(&samples, channel)?;
    }
    Ok(buffer)
}

/// Rises from the floor to 1, holds, and falls back, starting at `start`.
fn shape_envelope(gain: &AudioParam, envelope: &Envelope, start: f64) -> Result<()> {
    let attack_end = start + f64::from(envelope.attack.max(0.0));
    gain.set_value_at_time(FLOOR, start)?;
    if attack_end > start {
        gain.exponential_ramp_to_value_at_time(1.0, attack_end)?;
    } else {
        gain.set_value_at_time(1.0, start)?;
    }
    let hold = envelope.hold.max(0.0);
    if hold.is_finite() {
        let hold_end = attack_end + f64::from(hold);
        gain.set_value_at_time(1.0, hold_end)?;
        let release = envelope.release.max(0.0);
        if release > 0.0 {
            gain.exponential_ramp_to_value_at_time(FLOOR, hold_end + f64::from(release))?;
        } else {
            gain.set_value_at_time(0.0, hold_end)?;
        }
    }
    Ok(())
}

fn glide(
    param: &AudioParam,
    hz: f32,
    to_hz: Option<f32>,
    start: f64,
    length: Option<f32>,
) -> Result<()> {
    param.set_value_at_time(hz, start)?;
    if let (Some(to_hz), Some(length)) = (to_hz, length) {
        param.exponential_ramp_to_value_at_time(glide_target(to_hz), start + f64::from(length))?;
    }
    Ok(())
}

fn ramp(param: &AudioParam, now: f64, target: f32, seconds: f32) -> Result<()> {
    param.cancel_scheduled_values(now)?;
    if seconds > 0.0 {
        param.set_value_at_time(param.value(), now)?;
        param.linear_ramp_to_value_at_time(target, now + f64::from(seconds))?;
    } else {
        param.set_value_at_time(target, now)?;
    }
    Ok(())
}

pub(crate) fn add_bus(volume: f32, reverb: Option<Reverb>) -> usize {
    with_mixer(|mixer| {
        let output = mixer.level(volume)?;
        output.connect_with_audio_node(&mixer.buses.borrow()[0].input)?;
        let input: AudioNode = match reverb {
            Some(reverb) => {
                let convolver = mixer.context.create_convolver()?;
                convolver.set_buffer(Some(&impulse_response(&mixer.context, reverb.seconds)?));
                convolver.connect_with_audio_node(&output)?;
                convolver.into()
            }
            None => output.clone().into(),
        };
        let mut buses = mixer.buses.borrow_mut();
        buses.push(BusNodes { input, output });
        Ok(buses.len() - 1)
    })
    .unwrap_or(0)
}

pub(crate) fn set_bus_volume(bus: usize, volume: f32, ramp_seconds: f32) {
    with_mixer(|mixer| match mixer.buses.borrow().get(bus) {
        Some(nodes) => ramp(&nodes.output.gain(), mixer.now(), volume, ramp_seconds),
        None => Ok(()),
    });
}

pub(crate) fn set_compressor(compressor: Compressor) {
    with_mixer(|mixer| {
        mixer.apply_compressor(compressor);
        Ok(())
    });
}

pub(crate) fn state() -> MixerState {
    MIXER.with(|mixer| {
        let Some(mixer) = mixer else {
            return MixerState::Unavailable;
        };
        match mixer.context.state() {
            AudioContextState::Running => MixerState::Running,
            AudioContextState::Suspended if mixer.has_run.get() => MixerState::Suspended,
            AudioContextState::Suspended => MixerState::Locked,
            _ => MixerState::Unavailable,
        }
    })
}

pub(crate) fn now() -> f64 {
    with_mixer(|mixer| Ok(mixer.now())).unwrap_or(0.0)
}

pub(crate) fn set_listener(position: [f32; 3], forward: [f32; 3], up: [f32; 3]) {
    with_mixer(|mixer| {
        let listener = mixer.context.listener();
        let [x, y, z] = position.map(f64::from);
        listener.set_position(x, y, z);
        let [fx, fy, fz] = forward.map(f64::from);
        let [ux, uy, uz] = up.map(f64::from);
        listener.set_orientation(fx, fy, fz, ux, uy, uz);
        Ok(())
    });
}

pub(crate) fn load(location: String) -> Rc<Clip> {
    let clip = Rc::new(Clip(RefCell::new(None)));
    let target = clip.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(reason) = fetch_and_decode(&location, &target).await {
            warn(&format!("failed to load {location}: {reason:?}"));
        }
    });
    clip
}

async fn fetch_and_decode(location: &str, target: &Clip) -> Result<()> {
    use wasm_bindgen_futures::JsFuture;
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(location))
        .await?
        .unchecked_into();
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()).into());
    }
    let bytes = JsFuture::from(response.array_buffer()?).await?;
    let decoding = MIXER
        .with(|mixer| {
            mixer
                .as_ref()
                .map(|mixer| mixer.context.decode_audio_data(&bytes.unchecked_into()))
        })
        .ok_or("no audio")??;
    let decoded = JsFuture::from(decoding).await?;
    *target.0.borrow_mut() = Some(decoded.unchecked_into());
    Ok(())
}

pub(crate) fn is_ready(clip: &Clip) -> bool {
    clip.0.borrow().is_some()
}

pub(crate) fn play(clip: &Clip, playback: Playback) -> Option<Nodes> {
    let buffer = clip.0.borrow().clone()?;
    with_mixer(|mixer| {
        let start = mixer.start(playback.at);
        let source = mixer.context.create_buffer_source()?;
        source.set_buffer(Some(&buffer));
        source.set_loop(playback.looping);
        source.playback_rate().set_value(playback.pitch.max(0.01));
        let level = mixer.level(playback.volume)?;
        source.connect_with_audio_node(&level)?;
        mixer.route(&level, playback.bus, playback.send, playback.position)?;
        source.start_with_when(start)?;
        Ok(Nodes {
            level,
            sources: vec![source.into()],
        })
    })
}

pub(crate) fn play_tone(tone: &Tone) -> Option<Nodes> {
    with_mixer(|mixer| {
        let start = mixer.start(tone.at);
        let oscillator = mixer.context.create_oscillator()?;
        oscillator.set_type(match tone.wave {
            Wave::Sine => OscillatorType::Sine,
            Wave::Triangle => OscillatorType::Triangle,
            Wave::Sawtooth => OscillatorType::Sawtooth,
            Wave::Square => OscillatorType::Square,
        });
        glide(
            &oscillator.frequency(),
            tone.hz,
            tone.to_hz,
            start,
            tone.envelope.length(),
        )?;
        let shape = Shape {
            volume: tone.volume,
            envelope: tone.envelope,
            filter: tone.filter,
            bus: tone.bus,
            send: tone.send,
        };
        let begin = || oscillator.start_with_when(start);
        mixer.voice(oscillator.clone().into(), start, begin, shape)
    })
}

pub(crate) fn play_noise(noise: &Noise) -> Option<Nodes> {
    with_mixer(|mixer| {
        let start = mixer.start(noise.at);
        let source = mixer.context.create_buffer_source()?;
        source.set_buffer(Some(&mixer.noise));
        source.set_loop(true);
        let shape = Shape {
            volume: noise.volume,
            envelope: noise.envelope,
            filter: noise.filter,
            bus: noise.bus,
            send: noise.send,
        };
        let offset = f64::from(random() * NOISE_SECONDS);
        let begin = || source.start_with_when_and_grain_offset(start, offset);
        mixer.voice(source.clone().into(), start, begin, shape)
    })
}

pub(crate) fn stop(nodes: &Nodes, release_seconds: f32) {
    with_mixer(|mixer| {
        let now = mixer.now();
        ramp(&nodes.level.gain(), now, 0.0, release_seconds)?;
        for source in &nodes.sources {
            source.stop_with_when(now + f64::from(release_seconds) + TAIL_SECONDS)?;
        }
        Ok(())
    });
}

pub(crate) fn set_voice_volume(nodes: &Nodes, volume: f32, ramp_seconds: f32) {
    with_mixer(|mixer| ramp(&nodes.level.gain(), mixer.now(), volume, ramp_seconds));
}
