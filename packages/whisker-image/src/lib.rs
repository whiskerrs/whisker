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
//! use whisker_image::{Image, ImageMode, ImageProps};
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     render! {
//!         Image(
//!             src: "https://example.com/cover.jpg",
//!             mode: ImageMode::AspectFill,
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
//! - `mode` — content fit. Takes the typed [`ImageMode`] enum;
//!   defaults to [`ImageMode::AspectFill`].
//! - `style` — standard Whisker style string. Width / height must be
//!   set on the element (or via flex sizing) — Kingfisher / Coil
//!   target-size the fetched bitmap against the rendered size, so an
//!   element with `width: 0; height: 0;` would never paint.

use whisker::Signal;

/// Content-fit mode for an [`Image`]. The variant names mirror the
/// camelCase wire strings the iOS and Android image-view modules
/// dispatch on (`packages/whisker-image/ios/Sources/WhiskerImage/`,
/// `packages/whisker-image/android/src/main/kotlin/.../WhiskerImageView.kt`).
///
/// `#[non_exhaustive]` so a future fit mode (cover, contain, …) can
/// be added without breaking exhaustive matches downstream.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ImageMode {
    /// `"aspectFill"` — scale to fill the box while preserving aspect
    /// ratio, cropping the long edge. The default.
    #[default]
    AspectFill,
    /// `"aspectFit"` — scale to fit inside the box while preserving
    /// aspect ratio, letterboxing the short edge.
    AspectFit,
    /// `"scaleToFill"` — stretch to exactly fill the box, ignoring
    /// the aspect ratio.
    ScaleToFill,
    /// `"center"` — render at the source's intrinsic size, centered.
    Center,
}

impl ImageMode {
    /// Canonical wire string. Locked by unit tests against the
    /// native module's string dispatch table.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AspectFill => "aspectFill",
            Self::AspectFit => "aspectFit",
            Self::ScaleToFill => "scaleToFill",
            Self::Center => "center",
        }
    }
}

impl std::fmt::Display for ImageMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `whisker-image:Image` element. All props are reactive — the
/// platform-side setters re-apply whenever the bound signals change,
/// so a `src` swap re-fetches and a `mode` swap re-lays-out without
/// remount. Corners follow the standard CSS `border-radius` in the
/// `style:` cascade (iOS clips via `UIView.layer.cornerRadius` +
/// `clipsToBounds`; Android extracts the parsed radius from Lynx's
/// `onBorderRadiusUpdated` callback and feeds it to Coil's
/// `RoundedCornersTransformation`).
#[whisker::module_component("Image")]
pub fn image(src: Signal<String>, mode: Signal<ImageMode>, style: Signal<String>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_mode_wire_strings() {
        assert_eq!(ImageMode::AspectFill.as_str(), "aspectFill");
        assert_eq!(ImageMode::AspectFit.as_str(), "aspectFit");
        assert_eq!(ImageMode::ScaleToFill.as_str(), "scaleToFill");
        assert_eq!(ImageMode::Center.as_str(), "center");
    }

    #[test]
    fn image_mode_default_is_aspect_fill() {
        assert_eq!(ImageMode::default(), ImageMode::AspectFill);
    }

    #[test]
    fn image_mode_display_matches_as_str() {
        assert_eq!(ImageMode::AspectFill.to_string(), "aspectFill");
    }
}
