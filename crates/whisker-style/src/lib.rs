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
mod motion;
mod paint;
mod property;
mod resolution;
mod value;
mod value_tree;

pub use declaration::{CustomPropertyDeclaration, SpecifiedStyle, StyleDeclaration};
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
pub use motion::{
    AnimationValue, ComputedMotionStyle, ComputedTransition, ComputedTransitionProperty,
    KeyframeDefinition, KeyframesDefinition, MotionDirection, MotionEasing, MotionFillMode,
    MotionIterationCount, MotionPlayState, MotionStepPosition, MotionTime, TransitionPropertyValue,
    TransitionValue,
};
pub use paint::{
    BorderStyleValue, ComputedBackgroundImage, ComputedBackgroundLayerStyle,
    ComputedBackgroundPosition, ComputedBackgroundSize, ComputedBoxShadow, ComputedClipPath,
    ComputedClipPathCommand, ComputedClipPoint, ComputedClipShape, ComputedCornerRadius,
    ComputedGradient, ComputedGradientStop, ComputedInsetPathValue, ComputedOffsetPathValue,
    ComputedPaintStyle, ComputedTransformFunction, ComputedTransformStyle, Corners, OverflowValue,
    VisibilityValue,
};
pub use property::{
    PropertyMetadata, PropertyOrigin, StyleProperty, StylePropertyDomain, StylePropertyId,
};
pub use resolution::{
    ComputedLineHeight, ComputedStyle, ComputedTextDecoration, ComputedTextIndent,
    ComputedTextShadow, InheritedPropertySet, InheritedStyle, InheritedStyleChange,
    PropertyImpactSet, ResolvedNodeStyle, StyleEnvironment, StyleResolutionError, resolve_style,
    resolve_text_style,
};
pub use value::{
    BackdropFilterValue, BackgroundAttachmentValue, BackgroundBoxValue, BackgroundImageValue,
    BackgroundLayerValue, BackgroundPositionValue, BackgroundRepeatModeValue,
    BackgroundRepeatValue, BackgroundSizeValue, BackgroundValue, BorderRadiusValue, BoxShadowValue,
    CalcExpression, ClipBoxValue, ClipFillRuleValue, ClipPathCommandValue, ClipPathValue,
    ClipPointValue, ClipShapeValue, ColorValue, ComponentValue, CursorValue, CustomPropertyName,
    CustomPropertyReference, FontFamilyValue, FontFeatureValue, FontOpticalSizingValue,
    FontStyleValue, FontVariationValue, FontWeightValue, GradientStopValue, GradientValue,
    ImageRenderingValue, InsetPathValue, LengthPercentageValue, LengthUnit, LengthValue,
    LineHeightValue, MotionPathCommandValue, MotionPathPointValue, OffsetPathValue,
    OffsetRotateValue, OpenTypeTagValue, PointerEventsValue, RadialGradientValue, StyleNumber,
    StyleValue, TextAlignValue, TextDecorationLineValue, TextDecorationStyleValue,
    TextDecorationValue, TextOverflowValue, TextShadowValue, TransformFunctionValue,
    TransformOriginValue, TransformValue, WhiteSpaceValue, WordBreakValue,
};
