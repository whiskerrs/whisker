//! `whisker-video` — sample Whisker module showcasing
//! `@WhiskerUIMethod` + `ElementRef<T>` for imperative element-
//! method dispatch. Phase 7-Φ.H.2.6.
//!
//! Registers a single platform component with the Lynx tag
//! `whisker-video:Video` and exposes three sync methods callable
//! from Rust:
//!
//!   - `play()` — start / resume playback.
//!   - `pause()` — pause playback at the current position.
//!   - `seek(position_seconds)` — jump to an absolute position.
//!
//! Backed by `AVPlayer` on iOS and `VideoView` on Android. The
//! platform-side sources live in `src/ios/WhiskerVideoElement.
//! swift` and `src/android/WhiskerVideoElement.kt`.
//!
//! ## Usage
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_video::*;
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     let video = element_ref::<VideoProps>();
//!     let video_for_play = video.clone();
//!     let video_for_pause = video.clone();
//!     let video_for_seek = video.clone();
//!     render! {
//!         view(style: "flex-direction: column;") {
//!             Video(
//!                 ref: video,
//!                 src: "https://example.com/clip.mp4",
//!                 style: "width: 100%; height: 240px;"
//!             )
//!             view(style: "flex-direction: row;") {
//!                 text(value: "play",  on_tap: move || { video_for_play.play();  })
//!                 text(value: "pause", on_tap: move || { video_for_pause.pause(); })
//!                 text(value: "+10s",  on_tap: move || { video_for_seek.seek(10.0); })
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! ## Method dispatch path (Phase 7-Φ.H.2 stack)
//!
//! 1. `video.play()` (typed wrapper in this crate) builds `vec![]`
//!    and calls `VideoSys::play(&video, vec![])` via the
//!    `#[whisker::element_methods]`-generated impl.
//! 2. That impl calls `self.invoke("play", vec![])` on the
//!    `ElementRef<VideoProps>`.
//! 3. `ElementRef::invoke` resolves the bound `Element` handle
//!    and calls `whisker_driver::invoke_element_method(handle,
//!    "play", vec![])`.
//! 4. The driver looks up the platform `WhiskerElement*` via the
//!    renderer's `platform_component_ptr(handle)` and calls
//!    `whisker_bridge_invoke_element_method`.
//! 5. **(Pending Phase 7-Φ.H.2.7)** The C bridge resolves
//!    `WhiskerElement*` → Lynx UI sign → `LynxUI*` and dispatches
//!    `play:withResult:` / the `@LynxUIMethod`-tagged forwarder.
//!
//! Phase 7-Φ.H.2.7 closes step 5 — the C bridge calls the fork's
//! `lynx_ui_invoke_method`, which routes through `Catalyzer::Invoke`
//! to `LynxUIMethodProcessor.invokeMethod:forUI:` (iOS) /
//! `LynxUIMethodsExecutor.invokeMethod(...)` (Android), reaching
//! the `@WhiskerUIMethod`-emitted forwarder on the mounted
//! element's `WhiskerUI<View>` subclass.

use whisker::platform_module::WhiskerValue;
use whisker::{ElementRef, Signal};

/// `whisker-video:Video` platform component. The Lynx-side
/// implementations under `src/ios/` and `src/android/` declare
/// the same three `@WhiskerUIMethod`s as the `VideoSys` trait
/// below.
///
/// `src` is the media URL (the platform-side `@WhiskerProp("src")`
/// setter picks it up via Lynx's reflection-based prop dispatch).
/// `style` is the standard Whisker layout-styling string. Both
/// are optional — omitting them at the call site defaults the
/// prop to `Signal::Static(Default::default())` (empty string),
/// which platforms treat as "attribute not set".
#[whisker::platform_component("Video")]
pub fn video(src: Signal<String>, style: Signal<String>) {}

/// `-sys` proxy: each method is a thin pass-through that calls
/// `ElementRef::invoke(method, args)`. Typed wrappers live in
/// [`VideoControls`] below.
#[whisker::element_methods(VideoProps)]
pub trait VideoSys {
    fn play(&self, args: Vec<WhiskerValue>) -> WhiskerValue;
    fn pause(&self, args: Vec<WhiskerValue>) -> WhiskerValue;
    fn seek(&self, args: Vec<WhiskerValue>) -> WhiskerValue;
}

/// Typed Rust API on top of [`VideoSys`].
///
/// Same wrapper pattern as `whisker-local-store::WhiskerLocalStore`
/// — the `-sys` trait is the predictable WhiskerValue-only
/// bridge; the user-facing API is hand-written for ergonomics.
/// Errors from the underlying invoke (element not mounted,
/// platform-side failure, etc.) are intentionally swallowed —
/// these are fire-and-forget controls. Authors that want
/// stricter error handling can call [`VideoSys`] directly.
pub trait VideoControls {
    /// Start or resume playback.
    fn play(&self);
    /// Pause playback at the current position.
    fn pause(&self);
    /// Seek to an absolute position (seconds from the start).
    fn seek(&self, position_seconds: f64);
}

impl VideoControls for ElementRef {
    fn play(&self) {
        let _ = VideoSys::play(self, vec![]);
    }
    fn pause(&self) {
        let _ = VideoSys::pause(self, vec![]);
    }
    fn seek(&self, position_seconds: f64) {
        let _ = VideoSys::seek(self, vec![WhiskerValue::Float(position_seconds)]);
    }
}
