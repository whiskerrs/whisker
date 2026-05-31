//! Declarative-macro entry points.
//!
//! [`css!`](crate::css) builds a [`Css`](crate::Css) value from
//! `name: value` kwargs without naming the [`Css::new()`] entry
//! point on every call site.
//!
//! ```ignore
//! use whisker_css::ext::*;
//! use whisker_css::{css, Color, BorderStyle, Border};
//!
//! let s = css!(
//!     background_color: Color::hex(0x1A1330),
//!     padding: (px(8), px(16)),
//!     border: Border::new().width(px(1)).style(BorderStyle::Solid),
//! );
//! ```
//!
//! The expansion is the obvious chain of [`Css`](crate::Css)
//! builder calls — `Css::new().background_color(...).padding(...)
//! .border(...)`. Trailing commas are accepted; an empty `css!()`
//! collapses to `Css::new()`.
//!
//! **IDE completion**: the `$name:ident` matcher passes the kwarg
//! key through unchanged in the expansion's method-call position
//! (`.<name>(<value>)`), so rust-analyzer's standard method-name
//! completion fires inside the macro arguments — type
//! `css!(back|)` and the editor offers `background_color`,
//! `background_image`, `background_repeat`, … just as it would for
//! `Css::new().back|`.

/// Build a [`Css`](crate::Css) value from `name: value` kwargs.
///
/// See the [module docs](crate::macros) for examples and behavior.
#[macro_export]
macro_rules! css {
    () => {
        $crate::Css::new()
    };
    ($($name:ident : $value:expr),+ $(,)?) => {
        $crate::Css::new() $(.$name($value))+
    };
}
