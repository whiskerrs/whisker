//! `whisker-image` — networked image component.
//!
//! Mounts a `whisker-image:Image` element backed by:
//!
//! - **iOS**: `UIImageView` + [Kingfisher](https://github.com/onevcat/Kingfisher)
//!   for URL fetching, in-memory `NSCache`, and on-disk cache.
//! - **Android**: `ImageView` + [Coil](https://coil-kt.github.io/coil/)
//!   for URL fetching, `LruCache`, and disk cache.
//!
//! ## Why a separate module instead of Lynx's `<image>`?
//!
//! Lynx ships a `LynxServiceImageProtocol` interface that's expected
//! to be implemented + registered by the host app (Lynx's own
//! `LynxImageService` uses SDWebImage on iOS / Fresco on Android,
//! but it's a separate subspec that consumers wire themselves). The
//! Whisker iOS / Android distribution doesn't include any
//! implementation, so a bare `<image src="…">` mounts a `UIImageView`
//! whose `image` property never gets assigned. `whisker-image` skips
//! the Lynx image stack entirely and drives the URL load from the
//! native module directly — same idea as `whisker-video` for media
//! playback.
//!
//! ## Usage
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_image::{Image, ImageProps};
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     render! {
//!         Image(
//!             src: "https://example.com/cover.jpg",
//!             mode: "aspectFill",
//!             style: "width: 240px; height: 240px; border-radius: 8px;",
//!         )
//!     }
//! }
//! ```
//!
//! ## Props
//!
//! - `src` — image URL (HTTPS recommended; `http://` works if the
//!   host app's network security config allows cleartext).
//! - `mode` — content fit. `"aspectFill"` (centre-crop, default),
//!   `"aspectFit"` (letterbox), `"scaleToFill"` (stretch).
//! - `style` — standard Whisker style string. Width / height must be
//!   set on the element (or via flex sizing) — Kingfisher / Coil
//!   target-size the fetched bitmap against the rendered size, so an
//!   element with `width: 0; height: 0;` would never paint.

use whisker::Signal;

/// `whisker-image:Image` element. All props are reactive — the
/// platform-side setters re-apply whenever the bound signals change,
/// so a `src` swap re-fetches and a `mode` swap re-lays-out without
/// remount. Corners follow the standard CSS `border-radius` in the
/// `style:` cascade (iOS clips via `UIView.layer.cornerRadius` +
/// `clipsToBounds`; Android extracts the parsed radius from Lynx's
/// `onBorderRadiusUpdated` callback and feeds it to Coil's
/// `RoundedCornersTransformation`).
#[whisker::module_component("Image")]
pub fn image(src: Signal<String>, mode: Signal<String>, style: Signal<String>) {}
