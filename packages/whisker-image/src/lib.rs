//! `whisker-image` — networked image component.
//!
//! **API shape — 1 (pure Component).** See
//! [`docs/module-api-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/module-api-design.md)
//! §"Shape 1". All state is captured by props; no imperative
//! handle. Mounts a `whisker-image:Image` element backed by:
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
//!             style: css!(width: px(240), height: px(240), border_radius: px(8)),
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
//! - `headers` — extra request headers for remote sources, as a JSON
//!   object. Hot-link protection is what this is for: those hosts 403
//!   without the `Referer` their own pages send.
//! - `style` — structured Whisker CSS declarations. Width / height must be
//!   set on the element (or via flex sizing) — Kingfisher / Coil
//!   target-size the fetched bitmap against the rendered size, so an
//!   element with `width: 0; height: 0;` would never paint.
//! - `on_load` / `on_error` — optional; the load finished, with the
//!   decoded size, or failed, with the reason.
//!
//! ## Native source
//!
//! Contributors: the matching platform module lives at
//!
//! - iOS: `packages/whisker-image/ios/Sources/WhiskerImage/ImageModule.swift`
//!   (view: `ImageView.swift`)
//! - Android: `packages/whisker-image/android/src/main/kotlin/rs/whisker/elements/image/ImageModule.kt`
//!   (view: `WhiskerImageView.kt`)

use whisker::prelude::*;
use whisker::runtime::view::Element;

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

/// What a finished or failed load reports.
///
/// The body of a custom event, whose payload lands under `detail` on
/// both platforms. Every field defaults, so a partial body still calls
/// the handler — the event firing is the signal; its contents are the
/// detail.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct ImageEvent {
    #[serde(default)]
    pub detail: ImageDetail,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct ImageDetail {
    /// Pixel size of the loaded bitmap. Zero on failure.
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
    /// Why the load failed, as the platform described it. Empty on
    /// success.
    #[serde(default)]
    pub error: String,
}

impl ImageEvent {
    /// Why the load failed — shorthand for `self.detail.error`.
    pub fn error(&self) -> &str {
        &self.detail.error
    }
}

/// Wraps `Rc<dyn Fn(ImageEvent)>` so it's `Clone` (required:
/// `#[component]` re-clones every prop for the hot-reload remount path)
/// and so a bare closure coerces into it via `Into` at the call site
/// (`on_load: move |event| …`). A `Box<dyn Fn>` has neither property.
#[derive(Clone)]
pub struct ImageCallback(std::rc::Rc<dyn Fn(ImageEvent) + 'static>);

impl ImageCallback {
    /// Invoke the wrapped callback.
    pub fn call(&self, event: ImageEvent) {
        (self.0)(event)
    }
}

impl<F: Fn(ImageEvent) + 'static> From<F> for ImageCallback {
    fn from(f: F) -> Self {
        ImageCallback(std::rc::Rc::new(f))
    }
}

// The `whisker-image:Image` element itself. Crate-internal — only the
// outer `image` component mounts it, because a `module_component`'s
// event props are all required and these two must not be.
#[doc(hidden)]
#[whisker::module_component("Image")]
pub fn native_image(
    src: Signal<String>,
    mode: Signal<ImageMode>,
    headers: Signal<String>,
    style: whisker::Style,
    on_load: ImageEvent,
    on_error: ImageEvent,
) {
}

/// `whisker-image:Image` — a networked image.
///
/// All props are reactive: the platform-side setters re-apply whenever
/// the bound signals change, so a `src` swap re-fetches and a `mode`
/// swap re-lays-out without a remount. Corners follow the standard CSS
/// `border-radius` in the `style:` cascade (iOS clips via
/// `UIView.layer.cornerRadius` + `clipsToBounds`; Android extracts the
/// parsed radius from Lynx's `onBorderRadiusUpdated` callback and feeds
/// it to Coil's `RoundedCornersTransformation`).
///
/// A wrapper around the element rather than the element itself so that
/// `on_load` / `on_error` can be optional: a `module_component`'s event
/// props are required, and most callers just want a picture.
#[component]
pub fn image(
    /// Image URL. `file://` loads from disk; `http(s)://` from the
    /// network.
    src: Signal<String>,
    /// Content fit. Defaults to [`ImageMode::AspectFill`].
    #[prop(default = ImageMode::default().into())]
    mode: Signal<ImageMode>,
    /// Extra request headers for remote sources, as a JSON object.
    ///
    /// ```ignore
    /// Image(src: url, headers: r#"{"Referer": "https://example.com/"}"#)
    /// // built from data:
    /// Image(src: url, headers: json!({ "Referer": origin }).to_string())
    /// ```
    ///
    /// A string rather than a map because element attributes cross to
    /// the platform as strings, and JSON is what every caller already
    /// has a way to write. Changing it re-fetches: a host that answers
    /// differently per header answers differently per change.
    ///
    /// Sites that block hot-linking are the reason this exists — their
    /// images 403 without the `Referer` their own pages send. A value
    /// that isn't a JSON object of strings is ignored.
    #[prop(default = String::new().into())]
    headers: Signal<String>,
    /// Structured Whisker CSS declarations. Width / height must be set on the
    /// element (or via flex sizing) — the platform image loaders
    /// target-size the fetched bitmap against the rendered size.
    style: Option<whisker::Style>,
    /// The bitmap arrived and is on screen; `detail` carries its size.
    on_load: Option<ImageCallback>,
    /// The load failed; `detail.error` says how. A reader that shows a
    /// site's own images needs this — a page that 403s is otherwise a
    /// blank the app never hears about.
    on_error: Option<ImageCallback>,
) -> Element {
    let style_prop: whisker::Style = style.clone().unwrap_or_default();
    let load = on_load.clone();
    let error = on_error.clone();
    NativeImage(
        NativeImageProps::builder()
            .src(src)
            .mode(mode)
            .headers(headers)
            .style(style_prop)
            .on_load(move |event: ImageEvent| {
                if let Some(cb) = &load {
                    cb.call(event);
                }
            })
            .on_error(move |event: ImageEvent| {
                if let Some(cb) = &error {
                    cb.call(event);
                }
            })
            .build(),
    )
}

/// Warm the cache for `urls` without showing them.
///
/// The next [`image`] pointing at one of these paints from cache
/// instead of the network — what a reader does with the pages after
/// the one being read. Fire-and-forget: failures are the next real
/// load's problem, not this call's.
pub fn prefetch(urls: &[String], headers: &str) {
    let list = urls.to_vec();
    let headers = headers.to_string();
    whisker::spawn_local(async move {
        let _ = whisker::module!("Image")
            .invoke_async(
                "prefetch",
                vec![
                    whisker::platform_module::WhiskerValue::Array(
                        list.into_iter()
                            .map(whisker::platform_module::WhiskerValue::String)
                            .collect(),
                    ),
                    whisker::platform_module::WhiskerValue::String(headers),
                ],
            )
            .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_needs_only_a_src() {
        // That this builds at all is most of the assertion: the element's
        // own event props are required, the wrapper's are not.
        let props = ImageProps::builder()
            .src("https://example.com/a.png")
            .build();
        assert!(props.on_load.is_none());
        assert!(props.on_error.is_none());
    }

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
