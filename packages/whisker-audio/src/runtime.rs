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
///
/// `#[non_exhaustive]` so the surface can grow (e.g. a future
/// `is_buffering` flag, an `error: Option<...>`) without breaking
/// downstream code. Users read fields directly but should not
/// match the struct exhaustively — `PlaybackStatus { position,
/// duration, .. }` is the supported destructure form, and
/// construction from outside the crate is intentionally not
/// supported (the value is produced by the native module, never by
/// the consumer).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
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

/// Typed handle for one audio player. Cheap to clone (`Rc`-based
/// refcount); the underlying native player releases only after the
/// last clone drops.
///
/// # Example
///
/// ```ignore
/// use whisker_audio::Player;
///
/// let player = Player::new("https://example.com/clip.mp3");
/// player.play();
/// // Drop the last `player` clone to release the native engine.
/// ```
///
/// Methods are fire-and-forget — the native side reports state
/// changes through [`Player::status`] rather than method returns.
#[derive(Clone)]
pub struct Player {
    inner: Rc<PlayerInner>,
}

// Owned half of `Player`. Stashes the bridge-side id and runs
// `release` from `Drop`, so the last clone disposing releases the
// native player without manual book-keeping.
struct PlayerInner {
    id: u64,
}

impl PlayerInner {
    fn invoke(&self, method: &str, mut args: Vec<WhiskerValue>) -> WhiskerValue {
        // Native side keys its player map on arg 0; prepend so
        // callers don't have to remember the convention.
        args.insert(0, WhiskerValue::Int(self.id as i64));
        module!("WhiskerAudio").invoke(method, args)
    }
}

impl Drop for PlayerInner {
    fn drop(&mut self) {
        // Best-effort: a bridge teardown before us turns this into a
        // silent `WhiskerValue::Error`; the OS reclaims at exit.
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
    ///
    /// No-op if no source is loaded yet — the player will not start
    /// queuing playback ahead of the load completing; call again
    /// after `is_loaded` transitions to `true` (or simply call once
    /// at user gesture time and let the native player schedule).
    ///
    /// `PlaybackStatus` effect: `is_playing` → `true` (the next
    /// status tick); `position` resumes ticking from the current
    /// value.
    pub fn play(&self) {
        let _ = self.inner.invoke("play", vec![]);
    }

    /// Pause playback at the current position.
    ///
    /// `PlaybackStatus` effect: `is_playing` → `false`; `position`
    /// stops advancing but keeps its current value. The source and
    /// loaded state are untouched, so [`Self::play`] resumes from
    /// the same spot.
    pub fn pause(&self) {
        let _ = self.inner.invoke("pause", vec![]);
    }

    /// Stop playback and rewind to position 0. The native player
    /// stays loaded — [`Self::play`] resumes from the start
    /// without re-fetching.
    ///
    /// `PlaybackStatus` effect: `is_playing` → `false`; `position`
    /// → `0.0`. `duration` and `is_loaded` are preserved.
    pub fn stop(&self) {
        let _ = self.inner.invoke("stop", vec![]);
    }

    /// Seek to an absolute position (seconds from the start).
    ///
    /// Values outside `[0, duration]` are clamped on the native
    /// side. Seeking on a paused player keeps it paused; seeking
    /// while playing keeps it playing — there is no implicit
    /// play / pause toggle.
    ///
    /// `PlaybackStatus` effect: `position` jumps to the requested
    /// value on the next status tick. If the destination falls
    /// outside the buffered region the native player may stall
    /// briefly while re-buffering — surface to users via the
    /// `is_loaded` flag if needed (the platform side flips
    /// `is_loaded` back to `false` while it waits).
    pub fn seek_to(&self, position_seconds: f64) {
        let _ = self
            .inner
            .invoke("seekTo", vec![WhiskerValue::Float(position_seconds)]);
    }

    /// Replace the loaded media with `source`. Pass an empty string
    /// to release the current source without queueing a new one.
    ///
    /// **The player is reset by this call.** `PlaybackStatus`
    /// effects: `is_loaded` → `false` (until the new source's
    /// headers arrive); `duration` → `0.0`; `position` → `0.0`;
    /// `is_playing` → `false`. Equivalently to constructing a fresh
    /// [`Player`], you should re-call [`Self::play`] after the
    /// status reports `is_loaded = true` if you want auto-resume
    /// behaviour across source swaps.
    ///
    /// Passing the same source string again is *not* a no-op — the
    /// native player still tears down and re-loads. Compare with
    /// the previous source in user code if you want to skip the
    /// reload.
    pub fn set_source(&self, source: impl Into<String>) {
        let _ = self
            .inner
            .invoke("setSource", vec![WhiskerValue::String(source.into())]);
    }

    /// Set output gain on `[0.0, 1.0]`. Values outside the range
    /// get clamped on the native side.
    ///
    /// Does not affect `PlaybackStatus` — `is_playing` and
    /// `position` stay where they are. Setting volume to `0.0` is
    /// the conventional "mute" path; the native player keeps
    /// running.
    pub fn set_volume(&self, value: f64) {
        let _ = self
            .inner
            .invoke("setVolume", vec![WhiskerValue::Float(value)]);
    }

    /// Loop the source: when playback reaches the end, the native
    /// player rewinds to 0 and resumes. Idempotent — calling with
    /// the same flag again is a no-op.
    ///
    /// Does not affect `PlaybackStatus` until a loop boundary
    /// actually fires. At the wrap point the `position` resets to
    /// `0.0` and `is_playing` stays `true` (no momentary
    /// `false`-tick).
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
    ///
    /// Tick cadence: ≈ 200 ms while playing, plus an immediate
    /// tick on every state transition (`play` / `pause` / `stop`
    /// / `seekTo` / `setSource` / load-completion / loop-wrap).
    /// The first tick after `Player::new` lands once the native
    /// side has resolved the source URL and reports a non-zero
    /// `duration` — at that point `is_loaded` flips to `true`.
    /// All clones of the same `Player` see the identical signal.
    pub fn status(&self) -> ReadSignal<PlaybackStatus> {
        register_status(self.inner.id)
    }
}

// ---- Process-global status subscription dispatch --------------------------

// Atomic so any thread that managed to construct a `Player` can
// allocate; the dispatch path that consumes the id is main-thread-only.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

// Shared between the bridge-callback closure and `register_status`
// inserts. Wrapped in `MainThreadOnly` so `OnceLock<T>`'s `T: Sync`
// bound passes — see [`MainThreadOnly`] below.
type StatusEntries = Rc<RefCell<HashMap<u64, ArcRwSignal<PlaybackStatus>>>>;

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

/// Remove `id` from the dispatch table on player drop so a long-
/// lived process doesn't accumulate dead per-player slots.
fn unregister_status(id: u64) {
    if let Some(table) = STATUS_TABLE.get() {
        if let Ok(mut entries) = table.entries.inner.try_borrow_mut() {
            entries.remove(&id);
        }
    }
}

/// One-shot install of the `statusChanged` subscription. Stale
/// events for ids that were already released drop silently.
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
        // captures move the `Send + Sync` impl as a whole.
        let table = &entries_for_listener;
        let borrow = table.inner.borrow();
        if let Some(rw) = borrow.get(&id) {
            rw.set(status);
        }
    });
    // Leak: listener lives for the process; dropping the
    // subscription would also drop the closure the bridge holds.
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

/// Main-thread-only wrapper (mirrors `whisker-safe-area`'s). Asserts
/// the Lynx TASM-thread contract so `OnceLock<T>`'s `T: Sync` bound
/// passes without making `Rc` actually `Sync`.
#[derive(Clone)]
struct MainThreadOnly<T> {
    inner: T,
}
// Safety: every consumer runs on the reactive thread by contract
// (the bridge dispatches status events on the main thread). Misuse
// would corrupt the arena — same risk as any signal-touching code
// off the main thread.
unsafe impl<T> Send for MainThreadOnly<T> {}
unsafe impl<T> Sync for MainThreadOnly<T> {}
