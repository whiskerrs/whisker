//! `whisker-video` — video playback element with imperative controls.
//!
//! **API shape — 2 (Component + ref-bound handle).** See
//! [`docs/module-api-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/module-api-design.md)
//! §"Shape 2". A native UI element ([`Video`]) plus a typed handle
//! ([`VideoHandle`]) bound on mount via `element_ref:`; methods dispatch
//! through the element handle. Backed by AVPlayer (iOS), Media3
//! ExoPlayer (Android), and `HTMLVideoElement` (Web).
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
//!         View(style: css!(flex_direction: FlexDirection::Column)) {
//!             Video(element_ref: video.r(), src: "https://example.com/clip.mp4",
//!                   style: css!(width: percent(100), height: px(240)))
//!             // `VideoHandle` is `Copy`, so each `move ||` closure
//!             // captures its own copy — no `clone()` / pre-copy.
//!             View(style: css!(flex_direction: FlexDirection::Row)) {
//!                 Text(value: "play",  on_tap: move |_| video.play())
//!                 Text(value: "pause", on_tap: move |_| video.pause())
//!                 Text(value: "+10s",  on_tap: move |_| video.seek(10.0))
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! ## Implementation notes
//!
//! - `#[whisker::module_element("Video")]` declares the element for
//!   `render!`. The native element tag is `whisker-video:Video` (the crate name
//!   is auto-prepended).
//! - [`VideoHandle`] wraps an `ElementRef` (the element-id handle
//!   bound on mount); methods dispatch through
//!   `ElementRef::command(name, parameters)` over the module protocol.
//! - No `status()` signal yet — see
//!   [#128](https://github.com/whiskerrs/whisker/issues/128) for
//!   the pending observable-state decision.
//!
//! ## Native source
//!
//! Contributors: the matching platform module lives at
//!
//! - iOS: `packages/whisker-video/ios/Sources/WhiskerVideo/VideoModule.swift`
//!   (view: `VideoView.swift`)
//! - Android: `packages/whisker-video/android/src/main/kotlin/rs/whisker/elements/video/VideoModule.kt`
//!   (view: `VideoView.kt`)
//! - Web: `packages/whisker-video/web/src/lib.rs`

use whisker::platform_module::WhiskerValue;
use whisker::{ElementRef, Signal};

/// `whisker-video:Video` element. The platform-side `@WhiskerModule`
/// (`VideoModule`) registers a `VideoView` for this tag plus the
/// `Prop("src")` setter + `play` / `pause` / `seek` commands. `src`
/// is the media URL; `style` carries structured layout declarations.
#[whisker::module_element(
    name = "whisker-video:Video",
    measurement = None,
    commands = [("play", Null), ("pause", Null), ("seek", Float)],
)]
pub fn video(src: Signal<String>, style: whisker::Style) {}

/// Typed imperative handle for a mounted `Video` element.
///
/// Wraps the `ElementRef` (element-id handle) bound on mount when
/// passed as the element's `element_ref:` prop. Each method dispatches the
/// matching platform `Command` through `ElementRef::command`. Errors
/// (element not mounted, platform-side failure) are swallowed because
/// these are fire-and-forget UI controls.
///
/// `Copy` (the inner `ElementRef` is a slotmap-handle), so passing
/// it to multiple `on_tap` closures is just a copy — no `clone()`.
#[derive(Copy, Clone)]
pub struct VideoHandle {
    r: ElementRef,
}

impl VideoHandle {
    /// Allocate a fresh, unbound handle. Pass `handle.r()` to the
    /// element's `element_ref:` prop in `render!` to bind it on mount.
    pub fn new() -> Self {
        Self {
            r: ElementRef::new(),
        }
    }

    /// The underlying `ElementRef` to pass to `Video(element_ref: …)`.
    pub fn r(&self) -> ElementRef {
        self.r
    }

    /// Start or resume playback from the current position.
    ///
    /// No-op if the element isn't mounted yet (the underlying
    /// `ElementRef::command` cannot enqueue the dispatch) or if the
    /// native player is still loading `src`. Safe to call from a
    /// user gesture before the source finishes loading — the
    /// native player will start as soon as it's ready.
    pub fn play(&self) {
        let _ = self.r.command("play", WhiskerValue::Null);
    }

    /// Pause playback at the current position.
    ///
    /// The native player stays loaded and seekable; [`Self::play`]
    /// resumes from the same spot without re-fetching. No-op if
    /// nothing is currently playing or if the element isn't
    /// mounted.
    pub fn pause(&self) {
        let _ = self.r.command("pause", WhiskerValue::Null);
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
        let _ = self
            .r
            .command("seek", WhiskerValue::Float(position_seconds));
    }
}

impl Default for VideoHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_schema_declares_only_one_way_commands() {
        let schema = video_schema::schema();
        assert_eq!(schema.name, "whisker-video:Video");
        assert_eq!(
            schema
                .commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            ["play", "pause", "seek"]
        );
    }
}
