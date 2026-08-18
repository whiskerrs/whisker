//! Typed inline style semantics shared by Whisker's authoring, scene, and UI
//! layers.
//!
//! This crate owns stable property identities and will own typed declaration
//! storage, composition, inheritance, and computed-style resolution. It has no
//! dependency on a renderer, Host binding, or CSS parser.

#![warn(missing_docs)]

mod declaration;
mod property;
mod resolution;
mod value;

pub use declaration::{SpecifiedStyle, StyleDeclaration};
pub use property::{PropertyMetadata, PropertyOrigin, StyleProperty, StylePropertyId};
pub use resolution::{
    ComputedLineHeight, ComputedStyle, InheritedPropertySet, InheritedStyle, InheritedStyleChange,
    PropertyImpactSet, ResolvedNodeStyle, StyleEnvironment, StyleResolutionError,
    resolve_text_style,
};
pub use value::{
    CalcExpression, ColorValue, FontFamilyValue, FontStyleValue, FontWeightValue,
    LengthPercentageValue, LengthUnit, LengthValue, LineHeightValue, StyleNumber, StyleValue,
};
