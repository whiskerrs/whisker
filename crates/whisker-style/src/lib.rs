//! Typed inline style semantics shared by Whisker's authoring, scene, and UI
//! layers.
//!
//! This crate owns stable property identities and will own typed declaration
//! storage, composition, inheritance, and computed-style resolution. It has no
//! dependency on a renderer, Host binding, or CSS parser.

#![warn(missing_docs)]

mod declaration;
mod layout;
mod layout_value;
mod property;
mod resolution;
mod value;

pub use declaration::{SpecifiedStyle, StyleDeclaration};
pub use layout::{
    Axes, ComputedFlexBasis, ComputedLayoutStyle, ComputedLengthPercentage,
    ComputedLengthPercentageAuto, ComputedSizeValue, Edges,
};
pub use layout_value::{
    AlignContentValue, AlignItemsValue, AlignSelfValue, AspectRatioValue, BoxSizingValue,
    DirectionValue, DisplayValue, FlexBasisValue, FlexDirectionValue, FlexWrapValue,
    JustifyContentValue, LengthPercentageAutoValue, PositionValue, SizeValue,
};
pub use property::{PropertyMetadata, PropertyOrigin, StyleProperty, StylePropertyId};
pub use resolution::{
    ComputedLineHeight, ComputedStyle, InheritedPropertySet, InheritedStyle, InheritedStyleChange,
    PropertyImpactSet, ResolvedNodeStyle, StyleEnvironment, StyleResolutionError, resolve_style,
    resolve_text_style,
};
pub use value::{
    CalcExpression, ColorValue, FontFamilyValue, FontStyleValue, FontWeightValue,
    LengthPercentageValue, LengthUnit, LengthValue, LineHeightValue, StyleNumber, StyleValue,
};
