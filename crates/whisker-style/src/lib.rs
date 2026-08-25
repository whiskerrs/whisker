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
mod paint;
mod property;
mod resolution;
mod value;

pub use declaration::{SpecifiedStyle, StyleDeclaration};
pub use layout::{
    Axes, ComputedFlexBasis, ComputedGridMaxTrackSizing, ComputedGridMinTrackSizing,
    ComputedGridTemplate, ComputedGridTemplateComponent, ComputedGridTemplateRepetition,
    ComputedGridTrackSizing, ComputedLayoutStyle, ComputedLengthPercentage,
    ComputedLengthPercentageAuto, ComputedSizeValue, Edges, GridAutoFlowValue,
    GridPlacementLineValue, GridPlacementValue, GridRepetitionCountValue, GridTemplateAreaValue,
    GridTemplateAreasValue,
};
pub use layout_value::{
    AlignContentValue, AlignItemsValue, AlignSelfValue, AspectRatioValue, BoxSizingValue,
    ClearValue, DirectionValue, DisplayValue, FlexBasisValue, FlexDirectionValue, FlexWrapValue,
    FloatValue, GridMaxTrackSizingValue, GridMinTrackSizingValue, GridTemplateComponentValue,
    GridTemplateRepetitionValue, GridTemplateValue, GridTrackSizingValue, JustifyContentValue,
    LengthPercentageAutoValue, PositionValue, SizeValue,
};
pub use paint::{
    BorderStyleValue, ComputedBackgroundImage, ComputedBackgroundLayerStyle,
    ComputedBackgroundPosition, ComputedBackgroundSize, ComputedCornerRadius, ComputedGradient,
    ComputedGradientStop, ComputedPaintStyle, ComputedTransformFunction, ComputedTransformStyle,
    Corners, OverflowValue, VisibilityValue,
};
pub use property::{
    PropertyMetadata, PropertyOrigin, StyleProperty, StylePropertyDomain, StylePropertyId,
};
pub use resolution::{
    ComputedLineHeight, ComputedStyle, InheritedPropertySet, InheritedStyle, InheritedStyleChange,
    PropertyImpactSet, ResolvedNodeStyle, StyleEnvironment, StyleResolutionError, resolve_style,
    resolve_text_style,
};
pub use value::{
    BackdropFilterValue, BackgroundAttachmentValue, BackgroundBoxValue, BackgroundImageValue,
    BackgroundLayerValue, BackgroundPositionValue, BackgroundRepeatModeValue,
    BackgroundRepeatValue, BackgroundSizeValue, BackgroundValue, BorderRadiusValue, CalcExpression,
    ColorValue, FontFamilyValue, FontStyleValue, FontWeightValue, GradientStopValue, GradientValue,
    LengthPercentageValue, LengthUnit, LengthValue, LineHeightValue, MotionPathCommandValue,
    MotionPathPointValue, OffsetPathValue, OffsetRotateValue, RadialGradientValue, StyleNumber,
    StyleValue, TransformFunctionValue, TransformOriginValue, TransformValue,
};
