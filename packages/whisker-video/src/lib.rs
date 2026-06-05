//! `whisker-video` — sample Whisker view module: a `whisker-video:Video`
//! element backed by AVPlayer (iOS) / Media3 ExoPlayer (Android),
//! with imperative `play` / `pause` / `seek` methods.
//!
//! ## Shape
//!
//! - `#[whisker::module_component("Video")]` declares the element for
//!   `render!`. The Lynx tag is `whisker-video:Video` (the crate name
//!   is auto-prepended).
//! - `VideoHandle` is the typed, imperative API end-users hold. It
//!   wraps an `ElementRef` (the element-id handle bound on mount) and
//!   each method dispatches through `ElementRef::invoke(method, args)`
//!   — the raw `Vec<WhiskerValue>` wire (case ②).
//!
//! ## Usage
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_video::{Video, VideoHandle};
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     let video = VideoHandle::new();
//!     render! {
//!         view(style: "flex-direction: column;") {
//!             Video(ref: video.r(), src: "https://example.com/clip.mp4",
//!                   style: "width: 100%; height: 240px;")
//!             // `VideoHandle` is `Copy`, so each `move ||` closure
//!             // captures its own copy — no `clone()` / pre-copy.
//!             view(style: "flex-direction: row;") {
//!                 text(value: "play",  on_tap: move |_| video.play())
//!                 text(value: "pause", on_tap: move |_| video.pause())
//!                 text(value: "+10s",  on_tap: move |_| video.seek(10.0))
//!             }
//!         }
//!     }
//! }
//! ```

use whisker::platform_module::WhiskerValue;
use whisker::{ElementRef, Signal};

/// `whisker-video:Video` element. The platform-side `@WhiskerModule`
/// (`VideoModule`) registers a `VideoView` for this tag plus the
/// `Prop("src")` setter + `play` / `pause` / `seek` functions. `src`
/// is the media URL; `style` is the standard layout-styling string.
#[whisker::module_component("Video")]
pub fn video(src: Signal<String>, style: Signal<String>) {}

/// Typed imperative handle for a mounted `Video` element.
///
/// Wraps the `ElementRef` (element-id handle) bound on mount when
/// passed as the element's `ref:` prop. Each method dispatches the
/// matching platform `Function` through `ElementRef::invoke`. Errors
/// (element not mounted, platform-side failure) are swallowed — these
/// are fire-and-forget UI controls; call `r().invoke(...)` directly
/// for the raw `WhiskerValue` if you need to inspect failures.
///
/// `Copy` (the inner `ElementRef` is a slotmap-handle), so passing
/// it to multiple `on_tap` closures is just a copy — no `clone()`.
#[derive(Copy, Clone)]
pub struct VideoHandle {
    r: ElementRef,
}

impl VideoHandle {
    /// Allocate a fresh, unbound handle. Pass `handle.r()` to the
    /// element's `ref:` prop in `render!` to bind it on mount.
    pub fn new() -> Self {
        Self {
            r: ElementRef::new(),
        }
    }

    /// The underlying `ElementRef` to pass to `Video(ref: …)`.
    pub fn r(&self) -> ElementRef {
        self.r
    }

    /// Start or resume playback from the current position.
    ///
    /// No-op if the element isn't mounted yet (the underlying
    /// `ElementRef::invoke` swallows the dispatch) or if the
    /// native player is still loading `src`. Safe to call from a
    /// user gesture before the source finishes loading — the
    /// native player will start as soon as it's ready.
    pub fn play(&self) {
        let _ = self.r.invoke("play", WhiskerValue::args([]));
    }

    /// Pause playback at the current position.
    ///
    /// The native player stays loaded and seekable; [`Self::play`]
    /// resumes from the same spot without re-fetching. No-op if
    /// nothing is currently playing or if the element isn't
    /// mounted.
    pub fn pause(&self) {
        let _ = self.r.invoke("pause", WhiskerValue::args([]));
    }

    /// Seek to an absolute position (seconds from the start).
    ///
    /// Values outside `[0, duration]` are clamped on the native
    /// side. Seeking on a paused player keeps it paused; seeking
    /// while playing keeps it playing. The seek may stall briefly
    /// if the destination falls outside the buffered region —
    /// `whisker-video` does not currently expose a buffering
    /// signal (track via the platform's native controls instead).
    pub fn seek(&self, position_seconds: f64) {
        let _ = self.r.invoke(
            "seek",
            WhiskerValue::args([WhiskerValue::Float(position_seconds)]),
        );
    }
}

impl Default for VideoHandle {
    fn default() -> Self {
        Self::new()
    }
}
