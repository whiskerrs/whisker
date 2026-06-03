//! `whisker-svg` — inline SVG widget.
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

pub mod builder;
pub mod compile;
pub mod format;
pub mod path_parse;
pub mod replay;

pub use builder::{Color, DisplayListBuilder, Transform};
pub use compile::{compile, CompileError, Compiled};
pub use replay::{replay, ReplayError, TraceVisitor, Visitor};

use base64::Engine;
use whisker::prelude::*;
use whisker::runtime::view::Element;

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
pub fn svg(content: Signal<String>, color: Signal<String>, style: Signal<String>) -> Element {
    // Re-compile on every content change. The closure clones
    // `content` (Signal isn't Copy because the Static variant
    // owns its T) so the FnMut `#[component]` body can re-fire
    // without consuming the prop. The resulting `ReadSignal<String>`
    // is the base64-cased display list passed to the platform.
    let display_list = {
        let content = content.clone();
        computed(move || encode(&content.get()))
    };

    // Same clone trick for the pass-through props — the `render!`
    // macro internally captures each prop in a Fn closure to set
    // up reactive re-application, and a non-Copy `Signal` would
    // be moved out of the outer FnMut on the first re-fire.
    render! {
        SvgRenderer(
            display_list: display_list,
            color: color.clone(),
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
pub fn svg_renderer(display_list: Signal<String>, color: Signal<String>, style: Signal<String>) {}

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
        // Decoded bytes must start with the SPEC's magic.
        assert_eq!(&bytes[0..4], b"WSDL");
    }

    #[test]
    fn invalid_xml_yields_empty_string_not_panic() {
        // Producer must be forgiving — surfacing CompileError back
        // through Signal<String> doesn't compose. Stderr is enough.
        assert_eq!(encode("<svg>"), "");
    }
}
