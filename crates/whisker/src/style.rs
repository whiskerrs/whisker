//! [`Style`] — input wrapper for the `style:` attribute on every
//! built-in element tag.
//!
//! The element builder's `style(...)` method accepts any value that
//! converts into a [`Style`], which absorbs four sources:
//!
//! 1. A [`whisker_css::Css`] builder value (`Css::new().padding(8.px())`).
//! 2. A raw CSS string (`String` or `&str` / `&String`).
//! 3. A reactive [`ReadSignal<T>`] / [`RwSignal<T>`] of either form.
//!
//! Having one wrapper lets the same `view(style: ...)` keyword
//! accept all three shapes without callers having to call
//! `.to_css_string()` themselves. Reactive paths re-fire the
//! attribute apply inside the element's `effect`, matching the
//! semantics every other `Signal<T>`-driven prop already has.
//!
//! `Style` is defined in the `whisker` umbrella crate (rather than
//! in `whisker-css`) so the `Css` crate stays `whisker-runtime`-free
//! and reusable in standalone contexts.

use std::rc::Rc;

use whisker_css::{Css, ToCss};
use whisker_runtime::reactive::{ReadSignal, RwSignal, effect};
use whisker_runtime::view::Element;
use whisker_runtime::view::set_inline_styles;

/// Value the `style:` builder method receives. One of the two
/// variants below.
///
/// `Clone` is cheap: the `Dynamic` variant holds an [`Rc`], so a
/// clone shares the same closure rather than re-boxing it. This lets
/// the `#[component]` / `#[module_component]` macros store a `Style`
/// prop and re-clone it on every re-invoke (hot-reload remount path).
#[derive(Clone)]
pub enum Style {
    /// CSS source the builder applies once, at element-construction
    /// time. Both [`Css`] builder values and raw strings collapse to
    /// this variant.
    Static(String),
    /// CSS source produced by a reactive subscription. The shared
    /// closure is called inside an `effect` and re-fires whenever
    /// any signal it reads changes.
    Dynamic(Rc<dyn Fn() -> String + 'static>),
}

impl Default for Style {
    /// An empty static style — what an element would see if no
    /// `style:` prop were declared. Lets the macros emit
    /// `self.style.unwrap_or_default()` for an omitted style prop.
    fn default() -> Self {
        Style::Static(String::new())
    }
}

// ---- Static sources --------------------------------------------------------

impl From<Css> for Style {
    fn from(s: Css) -> Self {
        Style::Static(s.to_css_string())
    }
}

impl From<&Css> for Style {
    fn from(s: &Css) -> Self {
        Style::Static(s.to_css_string())
    }
}

impl From<String> for Style {
    fn from(s: String) -> Self {
        Style::Static(s)
    }
}

impl From<&str> for Style {
    fn from(s: &str) -> Self {
        Style::Static(s.to_string())
    }
}

impl From<&String> for Style {
    fn from(s: &String) -> Self {
        Style::Static(s.clone())
    }
}

// ---- Reactive sources -------------------------------------------------------
//
// One impl per (`ReadSignal` × `RwSignal`) × (`Css` × `String`) pair.
// Hand-written rather than blanket so coherence has no chance of
// complaining and the user-facing type-inference error pointing at
// an unsupported `T` stays sharp.

impl From<ReadSignal<Css>> for Style {
    fn from(sig: ReadSignal<Css>) -> Self {
        Style::Dynamic(Rc::new(move || sig.get().to_css_string()))
    }
}

impl From<ReadSignal<String>> for Style {
    fn from(sig: ReadSignal<String>) -> Self {
        Style::Dynamic(Rc::new(move || sig.get()))
    }
}

impl From<RwSignal<Css>> for Style {
    fn from(sig: RwSignal<Css>) -> Self {
        Style::from(sig.read_only())
    }
}

impl From<RwSignal<String>> for Style {
    fn from(sig: RwSignal<String>) -> Self {
        Style::from(sig.read_only())
    }
}

/// Apply a [`Style`] to a Lynx element. The `Static` branch sets the
/// inline-styles attribute once; the `Dynamic` branch wraps the
/// closure in an `effect` so it re-applies whenever any signal it
/// reads fires.
pub fn apply_style(h: Element, v: impl Into<Style>) {
    match v.into() {
        Style::Static(css) => set_inline_styles(h, &css),
        Style::Dynamic(f) => {
            effect(move || set_inline_styles(h, &f()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_css::ext::*;

    fn css(d: Style) -> String {
        match d {
            Style::Static(s) => s,
            Style::Dynamic(f) => f(),
        }
    }

    #[test]
    fn from_css_serializes_via_to_css_string() {
        let s = Css::new().padding(px(8));
        let out = css(s.into());
        assert!(out.contains("padding-top: 8px"));
    }

    #[test]
    fn from_borrowed_css_keeps_owner_alive() {
        let s = Css::new().padding(px(8));
        let style: Style = (&s).into();
        let out = css(style);
        assert!(out.contains("padding-top: 8px"));
        // `s` still usable after the conversion.
        assert!(!s.is_empty());
    }

    #[test]
    fn from_str_passes_through_verbatim() {
        let out = css("color: red;".into());
        assert_eq!(out, "color: red;");
    }

    #[test]
    fn from_string_consumes_and_returns_same_text() {
        let out = css(String::from("color: blue;").into());
        assert_eq!(out, "color: blue;");
    }

    #[test]
    fn from_string_ref_clones() {
        let owner = String::from("color: green;");
        let out = css((&owner).into());
        assert_eq!(out, "color: green;");
        assert_eq!(owner, "color: green;");
    }

    // Reactive From impls rely on the reactive runtime arena; the
    // standalone test environment doesn't bootstrap one, so the
    // dynamic-branch tests are exercised by the end-to-end runs in
    // `examples/podcast`. The static cases above already cover
    // the discriminant + serialization paths.
}
