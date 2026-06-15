//! `whisker-svg` — inline SVG widget.
//!
//! **API shape — 1 (pure Component).** See
//! [`docs/module-api-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/module-api-design.md)
//! §"Shape 1". All state is captured by props; no imperative
//! handle. The user-facing surface is [`Svg`] plus
//! [`compile`] / [`Compiled`] / [`CompileError`] for callers who
//! want to drive the compiler directly; everything else under this
//! crate is `#[doc(hidden)]` internal.
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_svg::Svg;
//!
//! #[component]
//! fn check_icon() -> Element {
//!     render! {
//!         Svg(
//!             content: r#"<svg viewBox="0 0 24 24">
//!                 <path d="M 5 12 L 10 17 L 19 7"
//!                       stroke="currentColor" stroke-width="2" fill="none"/>
//!             </svg>"#,
//!             color: "#1d9bf0",
//!             style: "width: 24px; height: 24px;",
//!         )
//!     }
//! }
//! ```
//!
//! ## How the bytes flow
//!
//! Rust calls [`compile`] on every change to `content`, producing
//! the v1 display-list byte stream defined in
//! `packages/whisker-svg/SPEC.md`. The bytes are base64-encoded
//! and sent through the standard Whisker `apply_attr` stringified-
//! value path — Whisker's existing C ABI doesn't expose a "binary
//! Prop" channel, and the iOS / Android replayer modules accept
//! the base64 directly. A future PR can swap the transport without
//! touching the platform replayer logic (the base64 layer is paint
//! around the `display_list` Prop only).
//!
//! ## Tinting
//!
//! `color:` is plumbed unmodified to the platform replayer. When
//! the source SVG uses `fill="currentColor"` (or `stroke="..."`),
//! the compiler emits `OP_PAINT_FILL_TINT` / `OP_PAINT_STROKE_TINT`
//! instead of a literal colour — the replayer substitutes the host's
//! `color:` value at draw time. See `SPEC.md` §"Tint propagation".
//!
//! ## Native source
//!
//! Contributors: the matching platform replayer lives at
//!
//! - iOS: `packages/whisker-svg/ios/Sources/WhiskerSvg/SvgModule.swift`
//!   (view: `WhiskerSvgView.swift`, decoder: `DisplayListReplayer.swift`)
//! - Android: `packages/whisker-svg/android/src/main/kotlin/rs/whisker/modules/svg/SvgModule.kt`
//!   (view: `WhiskerSvgView.kt`, decoder: `DisplayListReplayer.kt`)

// Modules below are `#[doc(hidden)]` — `pub` for the `tests/` crate
// integration tests, but not part of the SemVer surface. The
// user-facing API is `Svg` + `compile` / `Compiled` / `CompileError`
// re-exported below.
#[doc(hidden)]
pub mod builder;
#[doc(hidden)]
pub mod compile;
#[doc(hidden)]
pub mod format;
#[doc(hidden)]
pub mod path_parse;
#[doc(hidden)]
pub mod replay;

pub use compile::{compile, CompileError, Compiled};

#[doc(hidden)]
pub use builder::{Color, DisplayListBuilder, Transform};
#[doc(hidden)]
pub use replay::{replay, ReplayError, TraceVisitor, Visitor};

use base64::Engine;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker::Style;

/// `<Svg>` widget. Render arbitrary inline SVG inside the host
/// element. Tracks `content` reactively — a content swap recompiles
/// + re-renders.
///
/// Props:
///
/// - `content` — SVG XML source. Must contain a top-level `<svg>`
///   with a `viewBox`. Empty string renders nothing.
/// - `color` — CSS-style colour applied to any `fill="currentColor"` /
///   `stroke="currentColor"` paint inside the SVG. Defaults to the
///   host's inherited foreground colour.
/// - `style` — standard Whisker inline-style string for the host
///   `<view>`. Width / height MUST be set here (or via flex), the
///   replayer scales the SVG's viewBox to fill these bounds with
///   `preserveAspectRatio="xMidYMid meet"` semantics.
#[component]
pub fn svg(content: Signal<String>, color: Signal<String>, style: Style) -> Element {
    // `Signal<T>` is `Copy`, so `content` is freely moved into the
    // closure and `color` into the builder below even though the
    // `#[component]` body re-fires as a `FnMut` (whisker #8).
    let display_list = computed(move || encode(&content.get()));

    render! {
        SvgRenderer(
            display_list: display_list,
            color: color,
            style: style.clone(),
        )
    }
}

/// Internal `module_component` — the platform-side `WhiskerSvgView`
/// receives `display_list` (base64 of the binary SPEC bytes),
/// `color` (CSS colour string for tint substitution), and the
/// standard `style:` cascade. Users don't touch this directly;
/// it's hidden behind the `Svg` wrapper above.
///
/// The leading underscore on `display_list` is a convention to
/// signal "internal transport, not a user-facing API surface".
/// (Whisker doesn't enforce visibility on Prop names yet — this
/// is documentation-level.)
#[whisker::module_component("Svg")]
pub fn svg_renderer(display_list: Signal<String>, color: Signal<String>, style: Style) {}

/// Compile `svg_xml` to a base64-cased display list. Returns empty
/// string on parse failure (the replayer treats that as
/// "render nothing", same as an empty SVG). A best-effort surface
/// — surfacing the underlying [`CompileError`] to the user would
/// mean threading errors through the reactive graph, which doesn't
/// compose well with `Signal<String>` props. Diagnostics print to
/// stderr via `eprintln!`.
fn encode(svg_xml: &str) -> String {
    if svg_xml.is_empty() {
        return String::new();
    }
    match compile(svg_xml) {
        Ok(c) => {
            for w in &c.warnings {
                eprintln!("[whisker-svg] {w}");
            }
            base64::engine::general_purpose::STANDARD.encode(&c.bytes)
        }
        Err(e) => {
            eprintln!("[whisker-svg] compile failed: {e:?}");
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_empty_string() {
        assert_eq!(encode(""), "");
    }

    #[test]
    fn valid_svg_yields_nonempty_base64() {
        let b64 = encode(r#"<svg viewBox="0 0 24 24"><rect width="20" height="20"/></svg>"#);
        assert!(!b64.is_empty());
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .expect("valid base64");
        assert_eq!(&bytes[0..4], b"WSDL");
    }

    #[test]
    fn invalid_xml_yields_empty_string_not_panic() {
        // `encode` must be forgiving — surfacing CompileError through
        // Signal<String> doesn't compose.
        assert_eq!(encode("<svg>"), "");
    }
}
