# whisker-mixer design

Status: **planned**

## Decision

`whisker-mixer` is a module crate for low-latency sound: effects, ambience,
and music composed while the app runs. It mixes many short voices through buses
and effects on an audio clock. `whisker-audio` stays the module for playing one
long file through the platform's media player.

| | `whisker-audio` | `whisker-mixer` |
|---|---|---|
| Plays | One file at a time, streamed | Many voices at once, loaded whole or synthesized |
| Built on | `AVPlayer`, ExoPlayer, `<audio>` | Web Audio, or a Rust mixer on CoreAudio and AAudio |
| Owns | Background playback, lock screen, seeking | Buses, effects, pitch, synthesis, 3D position, audio-clock timing |
| Typical use | Podcasts, a track list, a video's sound | Tap feedback, game cues, generative music |

The two run side by side. Kiba builds its game audio on `whisker-mixer`
(Kiba DESIGN.md, D13), and a non-game Whisker app uses it for UI sound.

The mixer provides instruments and a clock. Apps compose: cue tables, sequencers,
and modulation are app code over the primitives below.

## API

```rust
use whisker_mixer::{Bus, Envelope, Filter, Noise, Playback, Reverb, Sound, Tone, Wave};

// Once, at start. Buses sit under master, which runs through a compressor.
let sfx = Bus::new(0.9);
let music = Bus::new(0.28);
let room = Bus::reverb(Reverb { seconds: 2.4 }, 0.35);

let swing = Sound::load(asset!("sfx/swing.mp3"));

// A recorded cue with random pitch and a reverb send.
swing.play_with(Playback {
    volume: 0.5,
    pitch: 1.0 + jitter(0.06),
    bus: sfx,
    send: Some((room, 0.15)),
    ..Playback::default()
});

// A synthesized cue, half a second from now.
Tone {
    wave: Wave::Triangle,
    hz: 880.0,
    to_hz: 440.0,
    envelope: Envelope { attack: 0.005, hold: 0.0, release: 0.3 },
    bus: music,
    send: Some((room, 0.8)),
    at: whisker_mixer::now() + 0.5,
    ..Tone::default()
}
.play();

// A loop whose volume the app moves.
let wind = noise_loop.play_with(Playback { looping: true, volume: 0.05, ..Playback::default() });
wind.set_volume(0.07, 2.0);

// Mute is a bus volume, so loops and scheduled voices fall silent too.
music.set_volume(0.0, 0.05);
```

- `Sound` is a loaded clip: a `Clone` handle, like `whisker-audio`'s `Player`.
  It plays any number of times at once.
- `Voice` is one playing instance, returned by `play_with`, `Tone::play`, and
  `Noise::play`. It has `stop(release_seconds)` and `set_volume(target, ramp_seconds)`.
  Dropping it lets the voice finish.
- `Bus` is a `Copy` key. `Bus::master()` is the root.
- `Tone` glides exponentially from `hz` to `to_hz` and may pass through a `Filter`.
  `Noise` reads a shared noise buffer from a random start through a `Filter`.
  A filter is a biquad kind, a Q, and a frequency that may glide.
- `Envelope` is attack, hold, and release. A pad that rises, holds for a bar, and
  fades past it needs all three.
- `now()` is the audio clock in seconds. `at` schedules against it, and a bar of
  music is scheduled ahead of it.
- `state()` returns `ReadSignal<MixerState>`: `Locked` until the browser allows
  sound, `Running`, `Suspended` while the app is in the background, or
  `Unavailable`. The clock stands still unless the state is `Running`, so apps
  start composing on `Running`.
- `set_listener(position, forward, up)` and `Playback::position` place voices in
  3D, as `[f32; 3]` so the crate does not depend on a math library.

The API is main-thread only, and its handles are not `Send`, as with the other
modules.

## Rust calls the backend directly

The mixer does not go through the module bridge. Apps create voices many times a
second and time them against the audio clock, and the bridge serializes every
call into `WhiskerValue` and, on native, crosses into Swift or Kotlin. App Rust
already runs where the audio API is: in the browser on the Web, and in the app
process on native. Swift and Kotlin code handles only what the platform reports
there: the audio session, interruptions, and the app lifecycle.

## Two backends with one meaning

**Web: Web Audio nodes.** Each primitive is one node, so the backend is thin.
Mixing runs on the browser's audio thread, files decode through
`decodeAudioData`, and no DSP or decoder ships in wasm. A Rust mixer on the Web
was rejected: `cpal`'s default Web backend mixes on the main thread, where game
frames would starve it, and its AudioWorklet backend needs nightly Rust,
`-Zbuild-std`, atomics, and cross-origin isolation **[verified]**.

**Native: a Rust mixer on `kira` over `cpal`.** `cpal` outputs through CoreAudio
on iOS and AAudio on Android **[verified]**. `kira` provides mixer tracks,
effects including reverb, compressor, and filter, clocks with scheduled starts,
playback-rate tweens, spatial listeners, and a `Sound` trait that `Tone` and
`Noise` implement **[verified]**. Whether it has send tracks for the reverb is
**[unverified]**; without them a reverb send becomes a second voice into a
reverb track. Platform audio graphs were rejected: Android has no node graph,
only `SoundPool` and effects bound to audio sessions, so it needs a mixer
anyway, and a second one for iOS alone saves nothing.

Parameters are defined by what they sound like, not by an algorithm. The Web
reverb convolves a generated impulse response and `kira`'s reverb is algorithmic,
so the two sound alike, not identical. `Reverb { seconds }` is the decay time on
both.

## Platform rules

- **Autoplay (Web).** Browsers allow sound only during a gesture. The mixer
  resumes its context on the first `pointerdown` or `keydown` on the window and
  reports `Locked` until then.
- **Background.** A hidden page or a backgrounded app suspends output, and the
  state becomes `Suspended`. Background playback is `whisker-audio`'s job.
- **iOS audio session.** The mixer sets the `.ambient` category: it follows the
  silent switch and mixes with other apps' music, as games do. The session is
  shared by the process, and `whisker-audio` sets `.playback` while it plays,
  which the mixer's output follows too. An interruption, such as a call,
  suspends the mixer until it ends.
- **Android.** The mixer requests no audio focus, since short sounds need none.
  AAudio needs API 26, and Whisker's minimum is 24, so the mixer reports
  `Unavailable` on API 24 and 25. `cpal` panics unless the Android context is
  set first, so the module calls `ndk_context::initialize_android_context` from
  the Host's JavaVM before opening a stream **[verified]**.
- **Desktop.** `cpal` covers macOS, Windows, and Linux, so the native backend
  should run there unchanged **[unverified]**.

The module needs no plugin at first, because playing sound needs no permission.

## Phases

| Phase | Scope | Exit criteria |
|---|---|---|
| W1 | Crate, Web backend: buses, compressor, reverb, pitch, `Tone`, `Noise`, `Voice`, clock, state | The slime port plays its 35 cues, wind, and music on the Web from `whisker-mixer` |
| W2 | Kiba's `Sound` replaced by `whisker-mixer`, the camera driving the listener | Kiba has no audio code of its own besides the listener |
| W3 | Native backend on iOS | The same app sounds on the iOS Simulator and a device |
| W4 | Native backend on Android | The same app sounds on an API 26+ device and reports `Unavailable` below |

W1 comes first because the slime port needs it before slime.chanto.games
switches to Kiba. Its order follows Kiba D13: buses and pitch, then synthesis,
then voices, then the clock.

## Verification

- Native unit tests check parameter validation and app schedulers with a seeded
  random source.
- On the Web, an `AnalyserNode` on master reads the level after each cue and after
  the first bar.
- On native, a test renders the mixer into a buffer instead of a device and
  checks its level, if `kira` exposes an offline backend **[unverified]**.
- Host conformance does not cover audio.

## Left out, with reasons

- **Streamed music.** A three-minute track decoded whole is about 60 MB. It comes
  with the first app whose music is a recording, through `kira`'s streaming
  sounds on native and a media element source on the Web.
- **Recording and microphone input.** They need permissions and a different
  pipeline, and belong with `whisker-audio`'s recording flags.
- **Sequencers, LFOs, and MIDI.** They are composition, which belongs to apps.
- **Effects beyond reverb, compressor, and filter.** Delay and distortion are
  added when an app asks.
- **3D position on buses.** Position is per voice.

## Open questions

- **iOS session conflicts.** An app that plays a podcast through `whisker-audio`
  while game sound runs leaves the category at `.playback`; whether the mixer
  should restore `.ambient` afterwards is undecided.
- **Android audio focus for music.** An app whose generated music should pause
  other apps' music would need focus. No app asks yet.
