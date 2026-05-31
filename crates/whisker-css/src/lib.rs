//! # whisker-css
//!
//! Type-safe CSS [`Css`] builder for Whisker, mirroring the Lynx
//! CSS surface 1-to-1.
//!
//! The crate is split into four layers:
//!
//! - [`data_type`] — the 11 data types Lynx exposes at
//!   <https://lynxjs.org/api/css/data-type.html>. Each is mapped to a
//!   Rust `enum` or `struct` with a [`ToCss`] implementation that
//!   round-trips back to its CSS source form.
//! - [`data_type_ext`] — data types Lynx uses inline inside property
//!   pages but does not document independently (`<integer>`,
//!   `<easing-function>`, `<position>`, the 147 [`NamedColor`]s).
//! - [`keyword`] — closed keyword enums for property values
//!   (`Display`, `FlexDirection`, …). Values Lynx explicitly rejects
//!   (`position: static`, `overflow: scroll`) are absent from the
//!   enums so they cause compile errors instead of silent runtime
//!   warnings.
//! - [`prop`] — one method per CSS longhand property on [`Css`],
//!   each carrying a documentation link to the corresponding
//!   `lynxjs.org/api/css/properties/<name>` page.
//! - [`shorthand`] — compound builders (`Border`, `Background`,
//!   `Transform`, `Transition`, `Animation`, `Flex`) for properties
//!   whose CSS shorthand combines multiple longhands.
//!
//! Numeric literals get their unit through extension traits in
//! [`ext`]: write `px(8)`, `8.px()`, or `0.5.rem()` to construct a
//! [`data_type::Length`].
//!
//! ```ignore
//! use whisker_css::ext::*;
//! use whisker_css::{Css, FlexDirection, Color};
//!
//! let s = Css::new()
//!     .display_flex()
//!     .flex_direction(FlexDirection::Column)
//!     .padding(px(12))
//!     .background_color(Color::hex(0x1A1A2E))
//!     .border_radius(px(10));
//! ```

#![warn(missing_docs)]

mod css;
pub mod data_type;
pub mod data_type_ext;
pub mod ext;
pub mod keyword;
pub mod prop;
pub mod shorthand;
mod to_css;
pub mod value;

// `css!(name: value, …)` — kwarg syntax for the [`Css`] builder.
// Lives in `whisker-macros` so it can be a proc macro (the
// partial-input recovery driving rust-analyzer completion needs
// fine-grained control over the expansion). Re-exported here so
// callers can spell `whisker_css::css!` without a direct dep on
// the macros crate.
pub use whisker_macros::css;

pub use crate::css::{Css, CssProp};
pub use crate::data_type::{
    Angle, CalcExpr, Color, ColorStop, CssString, FitContent, Gradient, Length, LengthPercentage,
    LinearDirection, MaxContent, NamedColor, Number, Percentage, RadialShape, StopPosition, Time,
};
pub use crate::data_type_ext::{EasingFunction, Integer, Position};
pub use crate::keyword::*;
pub use crate::shorthand::{
    Animation, Background, BackgroundLayer, Border, Flex, Margin, MarginValue, Padding, Transform,
    TransformFn, Transition,
};
pub use crate::to_css::ToCss;
pub use crate::value::{
    BorderRadius, FlexBasis, GridLine, GridTemplate, ImageRef, LineHeight, Repeated, Size,
};
