//! # whisker-css
//!
//! Type-safe CSS [`Css`] builder for Whisker.
//!
//! The crate is split into four layers:
//!
//! - [`data_type`] — reusable CSS value types represented as Rust enums and
//!   structs with [`ToCss`] implementations for diagnostics.
//! - [`data_type_ext`] — additional composite data types (`<integer>`,
//!   `<easing-function>`, `<position>`, the 147 [`NamedColor`]s).
//! - [`keyword`] — closed keyword enums for supported property values.
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
//! let s = Css::builder()
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
mod style_value;
mod to_css;
pub mod value;
mod variable;

// `css!` lives in `whisker-macros` because the partial-input recovery
// that drives rust-analyzer completion needs a proc macro. Re-exported
// here so callers can spell `whisker_css::css!` without depending on
// the macros crate directly.
pub use whisker_macros::css;

pub use crate::css::{Css, CssProp, CustomPropertyValue};
pub use crate::data_type::{
    Angle, CalcExpr, Color, ColorStop, CssString, FitContent, Gradient, Length, LengthPercentage,
    LinearDirection, MaxContent, NamedColor, Number, Percentage, RadialShape, StopPosition, Time,
};
pub use crate::data_type_ext::{EasingFunction, Integer, Position};
pub use crate::keyword::*;
pub use crate::shorthand::{
    Animation, AnimationTarget, Background, BackgroundLayer, Border, Flex, Keyframe, Keyframes,
    KeyframesBuildError, KeyframesBuilder, Margin, MarginValue, Padding, Transform, TransformFn,
    Transition,
};
pub use crate::to_css::ToCss;
pub use crate::value::{
    BackdropFilter, BorderRadius, BoxShadow, ClipBox, ClipFillRule, ClipPath, ClipPathCommand,
    ClipPoint, FlexBasis, GridArea, GridLine, GridRepeatCount, GridTemplate, GridTemplateAreas,
    GridTemplateComponent, GridTrack, GridTrackMax, GridTrackMin, ImageRef, InsetPath, LineHeight,
    MotionPathCommand, MotionPathPoint, OffsetDistance, OffsetPath, OffsetRotate, Repeated, Size,
};
pub use crate::variable::{ValueOrVariable, custom_var, custom_var_with_fallback};
pub use whisker_style::{
    CustomPropertyName, CustomPropertyReference, PropertyMetadata, PropertyOrigin, StyleProperty,
    StylePropertyId,
};
