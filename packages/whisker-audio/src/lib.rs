//! `whisker-audio` — audio playback for Whisker apps.
//!
//! **API shape — 3 (Clone value-type handle).** See
//! [`docs/module-api-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/module-api-design.md)
//! §"Shape 3". A view-less native resource: [`Player::new`] returns
//! a `Clone` handle, methods (`play` / `pause` / `seek_to` / …)
//! drive the underlying engine, and [`Player::status`] exposes a
//! reactive [`PlaybackStatus`] signal driven by native playback
//! callbacks. The native player releases when the last clone drops.
//!
//! Backed by AVPlayer (iOS) and AndroidX Media3 ExoPlayer (Android).
//! The surface mirrors the imperative half of
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
//! ## Implementation notes
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
//!   handle's signal.
//!
//! ## Native source
//!
//! Contributors: the matching platform module lives at
//!
//! - iOS: `packages/whisker-audio/ios/Sources/WhiskerAudio/AudioModule.swift`
//! - Android: `packages/whisker-audio/android/src/main/kotlin/rs/whisker/modules/audio/AudioModule.kt`

/// Whisker plugin — adds `Info.plist` / `AndroidManifest.xml`
/// entries when the consuming app declares
/// `app.plugin::<WhiskerAudio>(|c| …)` in `whisker.rs`. Always
/// available — independent of the `runtime` feature so the
/// `whisker.rs` config probe (which pulls this crate with
/// `default-features = false`) can still resolve `WhiskerAudio`.
mod plugin;
pub use plugin::*;

/// Player + reactive `PlaybackStatus` runtime. Gated behind the
/// default-on `runtime` feature so the config probe build path can
/// skip the heavyweight `whisker` umbrella crate (Lynx bridge,
/// driver, render layer). Apps depending on `whisker-audio` for
/// actual playback get this re-exported automatically; the probe
/// only sees the plugin types.
#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
pub use runtime::*;
