//! Bridge from [`Style`] to `whisker_runtime::reactive::Signal<String>`.
//!
//! Gated behind the `runtime-bridge` feature so the crate stays
//! usable without the rest of Whisker — only the umbrella `whisker`
//! crate turns this on.

use whisker_runtime::reactive::Signal;

use crate::{Style, ToCss};

impl From<Style> for Signal<String> {
    fn from(s: Style) -> Self {
        Signal::Static(s.to_css_string())
    }
}

impl From<&Style> for Signal<String> {
    fn from(s: &Style) -> Self {
        Signal::Static(s.to_css_string())
    }
}

// For reactive style values (`computed(move || some_style())`), the
// upstream `ReadSignal<Style>` ↔ `Signal<String>` conversion would
// have to live in `whisker-runtime` to satisfy Rust's orphan rules —
// neither crate is local to *both* `Signal` and `Style`. Until that
// hook lands, write the `.to_css_string()` call inside the
// `computed` closure: `computed(move || my_style().to_css_string())`
// produces a plain `ReadSignal<String>` that the builder already
// accepts.
