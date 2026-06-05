//! `whisker-audio` — audio playback for Whisker apps.
//!
//! View-less module backed by AndroidX Media3 ExoPlayer (Android)
//! and AVPlayer (iOS). Construct a [`Player`] anywhere a normal Rust
//! value can live — no element to mount, no `ref:` wiring; the
//! handle owns a native player instance, releases it on drop, and
//! exposes a reactive [`PlaybackStatus`] signal driven by native
//! playback callbacks.
//!
//! The API surface mirrors the imperative half of
//! [Expo's `expo-audio`](https://docs.expo.dev/versions/latest/sdk/audio/):
//! a player object you call `play` / `pause` / `seek_to` on, plus a
//! status field that ticks as the underlying engine reports
//! progress.
//!
//! ## Usage
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_audio::Player;
//!
//! #[component]
//! fn screen() -> Element {
//!     // Constructed once on mount; the handle owns the native
//!     // player and releases it when the surrounding owner disposes.
//!     let player = Player::new("https://example.com/clip.mp3");
//!     let status = player.status();
//!
//!     render! {
//!         view(style: "flex-direction: column; padding: 16px;") {
//!             text(value: move || format!(
//!                 "{:.1}s / {:.1}s",
//!                 status.get().position,
//!                 status.get().duration,
//!             ))
//!             view(on_tap: {
//!                 let p = player.clone();
//!                 move |_| p.play()
//!             }) { text(value: "play") }
//!             view(on_tap: {
//!                 let p = player.clone();
//!                 move |_| p.pause()
//!             }) { text(value: "pause") }
//!         }
//!     }
//! }
//! ```
//!
//! ## Shape
//!
//! - [`Player`] is `Clone` — internally an `Rc<PlayerInner>`. Each
//!   clone shares the same native player; the underlying player is
//!   released only after the last clone drops.
//! - Methods (`play`, `pause`, `stop`, `seek_to`, `set_source`,
//!   `set_volume`, `set_loop`) dispatch through
//!   `whisker::module!("WhiskerAudio").invoke(method, args)`.
//! - The native module emits a per-player `statusChanged` event
//!   every time playback state changes (and at a ~200 ms cadence
//!   while playing); [`Player::status`] lazily installs the
//!   dispatch table on first call and routes events to the matching
//!   handle's [`RwSignal<PlaybackStatus>`].

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use whisker::platform_module::WhiskerValue;
use whisker::{module, ArcReadSignal, ArcRwSignal, ReadSignal};

/// Current playback state. Updated by the native side and read
/// through the reactive signal returned by [`Player::status`].
///
/// All times are in seconds. `duration` is `0.0` until the native
/// player has finished its async load — UI that renders a progress
/// bar can branch on `is_loaded` to fade in once the value is
/// meaningful.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlaybackStatus {
    /// Current playback position from the start, in seconds.
    pub position: f64,
    /// Total media duration in seconds. `0.0` while still loading
    /// or for live streams without a known length.
    pub duration: f64,
    /// `true` once the player has loaded the source headers and
    /// reported a meaningful duration. `false` during the initial
    /// load and after a seek to an unbuffered region.
    pub is_loaded: bool,
    /// `true` while the engine is actively producing audio. Flips
    /// to `false` on `pause()`, `stop()`, and at end-of-media
    /// (unless `set_loop(true)` is set).
    pub is_playing: bool,
}

/// Typed handle for one audio player. Cheap to clone (an `Rc`-based
/// refcount); the underlying native player is released only after
/// every clone drops.
#[derive(Clone)]
pub struct Player {
    inner: Rc<PlayerInner>,
}

/// The owned half of [`Player`]. Stashes the bridge-side id and
/// runs `release` from `Drop`, so disposing the owner of the
/// last clone releases the native player without manual book-
/// keeping.
struct PlayerInner {
    id: u64,
}

impl PlayerInner {
    fn invoke(&self, method: &str, mut args: Vec<WhiskerValue>) -> WhiskerValue {
        // Every dispatch carries the player id as the first arg —
        // the native side keys its player map on it. Inserting
        // up-front keeps each caller from having to remember the
        // calling convention.
        args.insert(0, WhiskerValue::Int(self.id as i64));
        module!("WhiskerAudio").invoke(method, args)
    }
}

impl Drop for PlayerInner {
    fn drop(&mut self) {
        // Best-effort release — if the bridge tears down before us
        // the invoke fails silently (returns `WhiskerValue::Error`)
        // and the native player gets cleaned up by the process exit
        // anyway.
        let _ = module!("WhiskerAudio").invoke("release", vec![WhiskerValue::Int(self.id as i64)]);
        unregister_status(self.id);
    }
}

impl Player {
    /// Construct a new player playing from `source` (HTTP/HTTPS or
    /// `file://`). The native player starts loading immediately;
    /// playback begins when [`Player::play`] is called.
    ///
    /// Returns a `Player` handle. The native player stays alive as
    /// long as at least one clone of the handle does — drop the
    /// last clone (typically by letting the surrounding component
    /// owner dispose) to release.
    pub fn new(source: impl Into<String>) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let source = source.into();
        module!("WhiskerAudio").invoke(
            "create",
            vec![WhiskerValue::Int(id as i64), WhiskerValue::String(source)],
        );
        Self {
            inner: Rc::new(PlayerInner { id }),
        }
    }

    /// Start or resume playback from the current position.
    pub fn play(&self) {
        let _ = self.inner.invoke("play", vec![]);
    }

    /// Pause playback at the current position.
    pub fn pause(&self) {
        let _ = self.inner.invoke("pause", vec![]);
    }

    /// Stop playback and rewind to position 0. The native player
    /// stays loaded — [`Player::play`] resumes from the start
    /// without re-fetching.
    pub fn stop(&self) {
        let _ = self.inner.invoke("stop", vec![]);
    }

    /// Seek to an absolute position (seconds from the start).
    pub fn seek_to(&self, position_seconds: f64) {
        let _ = self
            .inner
            .invoke("seekTo", vec![WhiskerValue::Float(position_seconds)]);
    }

    /// Replace the loaded media. Pass an empty string to release
    /// the current source without queueing a new one.
    pub fn set_source(&self, source: impl Into<String>) {
        let _ = self
            .inner
            .invoke("setSource", vec![WhiskerValue::String(source.into())]);
    }

    /// Set output gain on `[0.0, 1.0]`. Values outside the range
    /// get clamped on the native side.
    pub fn set_volume(&self, value: f64) {
        let _ = self
            .inner
            .invoke("setVolume", vec![WhiskerValue::Float(value)]);
    }

    /// Loop the source: when playback reaches the end, the native
    /// player rewinds to 0 and resumes. Idempotent.
    pub fn set_loop(&self, looping: bool) {
        let _ = self
            .inner
            .invoke("setLoop", vec![WhiskerValue::Bool(looping)]);
    }

    /// Reactive playback status (position / duration / loaded /
    /// playing flags). First call from any clone installs the
    /// per-player signal and the process-level subscription that
    /// dispatches `statusChanged` events to the matching handle.
    ///
    /// Reads from the returned `ReadSignal<PlaybackStatus>` register
    /// the calling effect / computed as a dependent, so a `text`
    /// bound to `status.get().position` re-renders as the native
    /// player ticks through the file.
    pub fn status(&self) -> ReadSignal<PlaybackStatus> {
        register_status(self.inner.id)
    }
}

// ---- Process-global status subscription dispatch --------------------------

/// Monotonic source of fresh player ids. The atomic increment lets
/// any thread that managed to instantiate a `Player` allocate
/// without coordination; the bridge dispatch path that consumes
/// the id is main-thread-only.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Player id → reactive signal. Lives behind an `Rc<RefCell<…>>`
/// so the bridge-callback closure and the `register_status` insert
/// path share one table, and the whole thing rides in a
/// [`MainThreadOnly`] wrapper so the `Sync` bound on `OnceLock<T>`
/// passes (access is on the bridge's main thread by contract —
/// same model as `whisker-safe-area`).
type StatusEntries = Rc<RefCell<HashMap<u64, ArcRwSignal<PlaybackStatus>>>>;

/// Process-global dispatch table. Wraps [`StatusEntries`] so the
/// `OnceLock<StatusTable>` `Sync` requirement is satisfied by the
/// surrounding `MainThreadOnly` rather than by the `Rc` itself.
struct StatusTable {
    entries: MainThreadOnly<StatusEntries>,
}

static STATUS_TABLE: OnceLock<StatusTable> = OnceLock::new();

/// Install the global `statusChanged` listener (once) and return
/// the per-player signal handle. Re-entering for the same id
/// returns the existing signal so two clones of the same `Player`
/// see identical updates.
fn register_status(id: u64) -> ReadSignal<PlaybackStatus> {
    let table = STATUS_TABLE.get_or_init(install_status_listener);
    let entries = &table.entries.inner;
    let mut entries = entries.borrow_mut();
    let signal = entries
        .entry(id)
        .or_insert_with(|| ArcRwSignal::new(PlaybackStatus::default()));
    let (read, _write): (ArcReadSignal<_>, _) = signal.clone().split();
    read.into()
}

/// Remove the entry for `id` from the dispatch table — called from
/// `PlayerInner::drop` so a long-lived process doesn't accumulate
/// dead per-player signal slots.
fn unregister_status(id: u64) {
    if let Some(table) = STATUS_TABLE.get() {
        if let Ok(mut entries) = table.entries.inner.try_borrow_mut() {
            entries.remove(&id);
        }
    }
}

/// One-shot install of the `statusChanged` subscription. The
/// closure pulls the player id out of the event payload, looks
/// up the matching signal, and writes the decoded status. Stale
/// events for ids that were already released are dropped silently.
fn install_status_listener() -> StatusTable {
    let entries: StatusEntries = Rc::new(RefCell::new(HashMap::new()));
    let entries_for_listener = MainThreadOnly {
        inner: entries.clone(),
    };
    let sub = module!("WhiskerAudio").on_event("statusChanged", move |payload| {
        let WhiskerValue::Map(fields) = payload else {
            return;
        };
        let id = match fields.get("playerId") {
            Some(WhiskerValue::Int(v)) => *v as u64,
            Some(WhiskerValue::Float(v)) => *v as u64,
            _ => return,
        };
        let status = PlaybackStatus {
            position: read_f64(&fields, "position"),
            duration: read_f64(&fields, "duration"),
            is_loaded: read_bool(&fields, "isLoaded"),
            is_playing: read_bool(&fields, "isPlaying"),
        };
        // Bind the wrapper (not `.inner`) so Rust 2021 disjoint
        // captures move the `Send + Sync` impl as a whole. Same
        // dance as `whisker-safe-area` does for its writer.
        let table = &entries_for_listener;
        let borrow = table.inner.borrow();
        if let Some(rw) = borrow.get(&id) {
            rw.set(status);
        }
    });
    // Leak — the listener lives for the process lifetime; dropping
    // the subscription would also drop the closure pointer the
    // bridge holds.
    std::mem::forget(sub);
    StatusTable {
        entries: MainThreadOnly { inner: entries },
    }
}

fn read_f64(fields: &BTreeMap<String, WhiskerValue>, key: &str) -> f64 {
    match fields.get(key) {
        Some(WhiskerValue::Float(v)) => *v,
        Some(WhiskerValue::Int(v)) => *v as f64,
        _ => 0.0,
    }
}

fn read_bool(fields: &BTreeMap<String, WhiskerValue>, key: &str) -> bool {
    matches!(fields.get(key), Some(WhiskerValue::Bool(true)))
}

/// Main-thread-only wrapper, same shape as the one in
/// `whisker-safe-area`. The contract is that every access path
/// runs on the Lynx TASM thread; the unsafe `Send + Sync` impls
/// satisfy `OnceLock<T>`'s `T: Sync` bound by asserting that
/// constraint rather than enforcing it at compile time.
#[derive(Clone)]
struct MainThreadOnly<T> {
    inner: T,
}
// Safety: see module docs — every consumer runs on the reactive
// thread; misuse would corrupt the arena, but that's the standard
// risk for any signal-touching code off the main thread.
unsafe impl<T> Send for MainThreadOnly<T> {}
unsafe impl<T> Sync for MainThreadOnly<T> {}
