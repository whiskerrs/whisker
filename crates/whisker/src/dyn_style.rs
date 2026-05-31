//! [`DynStyle`] — input wrapper for the `style:` attribute on every
//! built-in element tag.
//!
//! The element builder's `style(...)` method accepts any value that
//! converts into a [`DynStyle`], which absorbs four sources:
//!
//! 1. A [`whisker_style::Style`] builder value (`Style::new().padding(8.px())`).
//! 2. A raw CSS string (`String` or `&str` / `&String`).
//! 3. A reactive [`ReadSignal<T>`] / [`RwSignal<T>`] of either form.
//!
//! Having one wrapper lets the same `view(style: ...)` keyword
//! accept all three shapes without callers having to call
//! `.to_css_string()` themselves. Reactive paths re-fire the
//! attribute apply inside the element's `effect`, matching the
//! semantics every other `Signal<T>`-driven prop already has.
//!
//! `DynStyle` is defined in the `whisker` umbrella crate (rather
//! than in `whisker-style`) so the `Style` crate stays
//! `whisker-runtime`-free and reusable in standalone contexts.

use whisker_runtime::reactive::{effect, ReadSignal, RwSignal};
use whisker_runtime::view::set_inline_styles;
use whisker_runtime::view::Element;
use whisker_style::{Style, ToCss};

/// Value the `style:` builder method receives. One of the two
/// variants below.
pub enum DynStyle {
    /// CSS source the builder applies once, at element-construction
    /// time. Both `Style` and `String` collapse to this variant.
    Static(String),
    /// CSS source produced by a reactive subscription. The boxed
    /// closure is called inside an `effect` and re-fires whenever
    /// any signal it reads changes.
    Dynamic(Box<dyn Fn() -> String + 'static>),
}

// ---- Static sources --------------------------------------------------------

impl From<Style> for DynStyle {
    fn from(s: Style) -> Self {
        DynStyle::Static(s.to_css_string())
    }
}

impl From<&Style> for DynStyle {
    fn from(s: &Style) -> Self {
        DynStyle::Static(s.to_css_string())
    }
}

impl From<String> for DynStyle {
    fn from(s: String) -> Self {
        DynStyle::Static(s)
    }
}

impl From<&str> for DynStyle {
    fn from(s: &str) -> Self {
        DynStyle::Static(s.to_string())
    }
}

impl From<&String> for DynStyle {
    fn from(s: &String) -> Self {
        DynStyle::Static(s.clone())
    }
}

// ---- Reactive sources -------------------------------------------------------
//
// One impl per (`ReadSignal` × `RwSignal`) × (`Style` × `String`)
// pair. Hand-written rather than blanket so coherence has no chance
// of complaining and the user-facing type-inference error pointing
// at an unsupported `T` stays sharp.

impl From<ReadSignal<Style>> for DynStyle {
    fn from(sig: ReadSignal<Style>) -> Self {
        DynStyle::Dynamic(Box::new(move || sig.get().to_css_string()))
    }
}

impl From<ReadSignal<String>> for DynStyle {
    fn from(sig: ReadSignal<String>) -> Self {
        DynStyle::Dynamic(Box::new(move || sig.get()))
    }
}

impl From<RwSignal<Style>> for DynStyle {
    fn from(sig: RwSignal<Style>) -> Self {
        DynStyle::from(sig.read_only())
    }
}

impl From<RwSignal<String>> for DynStyle {
    fn from(sig: RwSignal<String>) -> Self {
        DynStyle::from(sig.read_only())
    }
}

/// Apply a [`DynStyle`] to a Lynx element. The `Static` branch sets
/// the inline-styles attribute once; the `Dynamic` branch wraps the
/// closure in an `effect` so it re-applies whenever any signal it
/// reads fires.
pub fn apply_dyn_style(h: Element, v: impl Into<DynStyle>) {
    match v.into() {
        DynStyle::Static(css) => set_inline_styles(h, &css),
        DynStyle::Dynamic(f) => {
            effect(move || set_inline_styles(h, &f()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_style::ext::*;

    fn css(d: DynStyle) -> String {
        match d {
            DynStyle::Static(s) => s,
            DynStyle::Dynamic(f) => f(),
        }
    }

    #[test]
    fn from_style_serializes_via_to_css_string() {
        let s = Style::new().padding(px(8));
        let css = css(s.into());
        assert!(css.contains("padding-top: 8px"));
    }

    #[test]
    fn from_borrowed_style_keeps_owner_alive() {
        let s = Style::new().padding(px(8));
        let dyn_style: DynStyle = (&s).into();
        let css = css(dyn_style);
        assert!(css.contains("padding-top: 8px"));
        // `s` still usable after the conversion.
        assert!(!s.is_empty());
    }

    #[test]
    fn from_str_passes_through_verbatim() {
        let css = css("color: red;".into());
        assert_eq!(css, "color: red;");
    }

    #[test]
    fn from_string_consumes_and_returns_same_text() {
        let css = css(String::from("color: blue;").into());
        assert_eq!(css, "color: blue;");
    }

    #[test]
    fn from_string_ref_clones() {
        let owner = String::from("color: green;");
        let css = css((&owner).into());
        assert_eq!(css, "color: green;");
        assert_eq!(owner, "color: green;");
    }

    // Reactive From impls rely on the reactive runtime arena; the
    // standalone test environment doesn't bootstrap one, so the
    // dynamic-branch tests are exercised by the end-to-end runs in
    // `examples/hello-world`. The static cases above already cover
    // the discriminant + serialization paths.
}
