//! Stable identities and metadata for Whisker's common style properties.
//!
//! Numeric IDs are part of the renderer-facing schema. Existing assignments
//! must never be reordered or reused; new properties are appended.

/// A non-zero stable identifier for a common style property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StylePropertyId(u32);

impl StylePropertyId {
    /// Creates an ID, rejecting the reserved zero value.
    pub const fn new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// Returns the numeric identifier.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Where a property's spelling originates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyOrigin {
    /// A standard, non-deprecated CSS property in Whisker's conformance target.
    Css,
    /// A WebKit compatibility property supported by Lynx.
    Webkit,
    /// A Lynx-specific extension whose name starts with `-x-`.
    Lynx,
    /// A Lynx-specific extension without a vendor prefix.
    LynxUnprefixed,
}

/// Static metadata for one common style property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PropertyMetadata {
    /// Stable renderer-facing ID.
    pub id: StylePropertyId,
    /// Compatibility spelling used by the temporary Lynx serializer.
    pub css_name: &'static str,
    /// Origin of the compatibility spelling and semantics.
    pub origin: PropertyOrigin,
    /// Whether an omitted value inherits from the nearest ancestor.
    ///
    /// Version 1 deliberately limits inheritance to seven text properties.
    pub inherited: bool,
    /// Host-independent stage that owns the property's resolved semantics.
    pub domain: StylePropertyDomain,
}

/// Destination of a property after typed declaration resolution.
///
/// This classifies ownership, not implementation status. In particular, a
/// property can have a protocol domain before every Host implements it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StylePropertyDomain {
    /// Taffy or Whisker's surrounding layout integration consumes the value.
    Layout,
    /// Rust motion/timeline code samples the value before presentation.
    Motion,
    /// Background color or image-layer protocol state.
    Background,
    /// Border and radius protocol state.
    BoxPaint,
    /// Outline, shadow, mask, filter, or shape-clip protocol state.
    VisualEffects,
    /// Text shaping or text-paint protocol state.
    Text,
    /// Transform matrix or 3-D transform-group protocol state.
    Transform,
    /// Opacity, visibility, or stacking/compositing protocol state.
    Compositing,
    /// Descendant clipping or scrolling behavior.
    ClipAndScroll,
    /// Host hit testing, cursor, caret, or other input presentation.
    Interaction,
    /// Compatibility-only property excluded from the standard CSS target.
    CompatibilityExtension,
}

macro_rules! define_style_properties {
    ($($variant:ident = $id:literal => $name:literal,)*) => {
        /// A common style property known to Whisker.
        ///
        /// Discriminants are stable protocol schema assignments. Additions must
        /// be appended with a fresh ID; existing entries must not be reordered.
        #[repr(u32)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum StyleProperty {
            $(
                #[doc = concat!("The `", $name, "` property.")]
                $variant = $id,
            )*
        }

        impl StyleProperty {
            /// Every registered property in stable ID order.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];

            /// Returns the stable property ID.
            pub const fn id(self) -> StylePropertyId {
                // Every macro entry is explicitly non-zero.
                StylePropertyId(self as u32)
            }

            /// Returns the compatibility CSS/Lynx spelling.
            pub const fn css_name(self) -> &'static str {
                match self { $(Self::$variant => $name,)* }
            }

            /// Resolves a compatibility spelling to its stable property.
            pub fn from_css_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$variant),)* _ => None }
            }

            /// Resolves a stable ID to its property.
            pub fn from_id(id: StylePropertyId) -> Option<Self> {
                match id.get() { $($id => Some(Self::$variant),)* _ => None }
            }

            /// Returns registry metadata for the property.
            pub const fn metadata(self) -> PropertyMetadata {
                let origin = PropertyOrigin::Css;
                let inherited = matches!(
                    self,
                    Self::FontFamily
                        | Self::FontSize
                        | Self::FontWeight
                        | Self::FontStyle
                        | Self::LineHeight
                        | Self::LetterSpacing
                        | Self::Color
                        | Self::TextShadow
                );
                let domain = self.domain();
                PropertyMetadata {
                    id: self.id(),
                    css_name: self.css_name(),
                    origin,
                    inherited,
                    domain,
                }
            }

            /// Returns the Host-independent semantic destination.
            pub const fn domain(self) -> StylePropertyDomain {
                match self {
                    Self::Animation
                    | Self::AnimationDelay
                    | Self::AnimationDirection
                    | Self::AnimationDuration
                    | Self::AnimationFillMode
                    | Self::AnimationIterationCount
                    | Self::AnimationName
                    | Self::AnimationPlayState
                    | Self::AnimationTimingFunction
                    | Self::Transition
                    | Self::TransitionDelay
                    | Self::TransitionDuration
                    | Self::TransitionProperty
                    | Self::TransitionTimingFunction => StylePropertyDomain::Motion,

                    Self::Background
                    | Self::BackgroundAttachment
                    | Self::BackgroundClip
                    | Self::BackgroundColor
                    | Self::BackgroundImage
                    | Self::BackgroundOrigin
                    | Self::BackgroundPosition
                    | Self::BackgroundPositionX
                    | Self::BackgroundPositionY
                    | Self::BackgroundRepeat
                    | Self::BackgroundSize => StylePropertyDomain::Background,

                    Self::BorderBottomColor
                    | Self::BorderBottomLeftRadius
                    | Self::BorderBottomRightRadius
                    | Self::BorderBottomStyle
                    | Self::BorderBottomWidth
                    | Self::BorderLeftColor
                    | Self::BorderLeftStyle
                    | Self::BorderLeftWidth
                    | Self::BorderRadius
                    | Self::BorderRightColor
                    | Self::BorderRightStyle
                    | Self::BorderRightWidth
                    | Self::BorderTopColor
                    | Self::BorderTopLeftRadius
                    | Self::BorderTopRightRadius
                    | Self::BorderTopStyle
                    | Self::BorderTopWidth
                    | Self::Border
                    | Self::BorderBottom
                    | Self::BorderColor
                    | Self::BorderEndEndRadius
                    | Self::BorderEndStartRadius
                    | Self::BorderInlineEndColor
                    | Self::BorderInlineEndStyle
                    | Self::BorderInlineEndWidth
                    | Self::BorderInlineStartColor
                    | Self::BorderInlineStartStyle
                    | Self::BorderInlineStartWidth
                    | Self::BorderLeft
                    | Self::BorderRight
                    | Self::BorderStartEndRadius
                    | Self::BorderStartStartRadius
                    | Self::BorderStyle
                    | Self::BorderTop
                    | Self::BorderWidth => StylePropertyDomain::BoxPaint,

                    Self::BackdropFilter
                    | Self::BoxShadow
                    | Self::ClipPath
                    | Self::ImageRendering
                    | Self::Mask
                    | Self::MaskComposite
                    | Self::MaskImage
                    | Self::OutlineColor
                    | Self::OutlineOffset
                    | Self::OutlineStyle
                    | Self::OutlineWidth => StylePropertyDomain::VisualEffects,

                    Self::CaretColor | Self::Cursor | Self::PointerEvents => {
                        StylePropertyDomain::Interaction
                    }

                    Self::Color
                    | Self::Direction
                    | Self::FontFamily
                    | Self::FontFeatureSettings
                    | Self::FontOpticalSizing
                    | Self::FontSize
                    | Self::FontStyle
                    | Self::FontVariationSettings
                    | Self::FontVariant
                    | Self::FontWeight
                    | Self::LetterSpacing
                    | Self::LineHeight
                    | Self::OverflowWrap
                    | Self::TextAlign
                    | Self::TextDecoration
                    | Self::TextDecorationColor
                    | Self::TextDecorationLine
                    | Self::TextDecorationStyle
                    | Self::TextDecorationThickness
                    | Self::TextIndent
                    | Self::TextOverflow
                    | Self::TextShadow
                    | Self::TextTransform
                    | Self::WhiteSpace
                    | Self::WordBreak => StylePropertyDomain::Text,

                    Self::BackfaceVisibility
                    | Self::Perspective
                    | Self::PerspectiveOrigin
                    | Self::OffsetDistance
                    | Self::OffsetPath
                    | Self::OffsetRotate
                    | Self::Transform
                    | Self::TransformBox
                    | Self::TransformOrigin
                    | Self::TransformStyle => StylePropertyDomain::Transform,

                    Self::Opacity | Self::Visibility | Self::ZIndex => {
                        StylePropertyDomain::Compositing
                    }

                    Self::Overflow | Self::OverflowX | Self::OverflowY => {
                        StylePropertyDomain::ClipAndScroll
                    }

                    Self::AlignContent
                    | Self::AlignItems
                    | Self::AlignSelf
                    | Self::AspectRatio
                    | Self::Bottom
                    | Self::BoxSizing
                    | Self::ColumnGap
                    | Self::Display
                    | Self::Float
                    | Self::Clear
                    | Self::Flex
                    | Self::FlexBasis
                    | Self::FlexDirection
                    | Self::FlexFlow
                    | Self::FlexGrow
                    | Self::FlexShrink
                    | Self::FlexWrap
                    | Self::GridAutoColumns
                    | Self::GridAutoFlow
                    | Self::GridAutoRows
                    | Self::GridColumn
                    | Self::GridColumnEnd
                    | Self::GridColumnStart
                    | Self::GridRow
                    | Self::GridRowEnd
                    | Self::GridRowStart
                    | Self::GridTemplateColumns
                    | Self::GridTemplateAreas
                    | Self::GridTemplateRows
                    | Self::Height
                    | Self::InsetInlineEnd
                    | Self::InsetInlineStart
                    | Self::JustifyContent
                    | Self::JustifyItems
                    | Self::JustifySelf
                    | Self::Left
                    | Self::Margin
                    | Self::MarginBottom
                    | Self::MarginInlineEnd
                    | Self::MarginInlineStart
                    | Self::MarginLeft
                    | Self::MarginRight
                    | Self::MarginTop
                    | Self::MaxHeight
                    | Self::MaxWidth
                    | Self::MinHeight
                    | Self::MinWidth
                    | Self::Order
                    | Self::Padding
                    | Self::PaddingBottom
                    | Self::PaddingInlineEnd
                    | Self::PaddingInlineStart
                    | Self::PaddingLeft
                    | Self::PaddingRight
                    | Self::PaddingTop
                    | Self::Position
                    | Self::Right
                    | Self::Gap
                    | Self::RowGap
                    | Self::Top
                    | Self::VerticalAlign
                    | Self::Width => StylePropertyDomain::Layout,
                }
            }
        }
    };
}

define_style_properties! {
    AlignContent = 7 => "align-content",
    AlignItems = 8 => "align-items",
    AlignSelf = 9 => "align-self",
    Animation = 10 => "animation",
    AnimationDelay = 11 => "animation-delay",
    AnimationDirection = 12 => "animation-direction",
    AnimationDuration = 13 => "animation-duration",
    AnimationFillMode = 14 => "animation-fill-mode",
    AnimationIterationCount = 15 => "animation-iteration-count",
    AnimationName = 16 => "animation-name",
    AnimationPlayState = 17 => "animation-play-state",
    AnimationTimingFunction = 18 => "animation-timing-function",
    AspectRatio = 19 => "aspect-ratio",
    BackfaceVisibility = 20 => "backface-visibility",
    Background = 21 => "background",
    BackgroundAttachment = 22 => "background-attachment",
    BackgroundClip = 23 => "background-clip",
    BackgroundColor = 24 => "background-color",
    BackgroundImage = 25 => "background-image",
    BackgroundOrigin = 26 => "background-origin",
    BackgroundPosition = 27 => "background-position",
    BackgroundPositionX = 28 => "background-position-x",
    BackgroundPositionY = 29 => "background-position-y",
    BackgroundRepeat = 30 => "background-repeat",
    BackgroundSize = 31 => "background-size",
    BorderBottomColor = 32 => "border-bottom-color",
    BorderBottomLeftRadius = 33 => "border-bottom-left-radius",
    BorderBottomRightRadius = 34 => "border-bottom-right-radius",
    BorderBottomStyle = 35 => "border-bottom-style",
    BorderBottomWidth = 36 => "border-bottom-width",
    BorderLeftColor = 37 => "border-left-color",
    BorderLeftStyle = 38 => "border-left-style",
    BorderLeftWidth = 39 => "border-left-width",
    BorderRadius = 40 => "border-radius",
    BorderRightColor = 41 => "border-right-color",
    BorderRightStyle = 42 => "border-right-style",
    BorderRightWidth = 43 => "border-right-width",
    BorderTopColor = 44 => "border-top-color",
    BorderTopLeftRadius = 45 => "border-top-left-radius",
    BorderTopRightRadius = 46 => "border-top-right-radius",
    BorderTopStyle = 47 => "border-top-style",
    BorderTopWidth = 48 => "border-top-width",
    Bottom = 49 => "bottom",
    BoxShadow = 50 => "box-shadow",
    BoxSizing = 51 => "box-sizing",
    CaretColor = 52 => "caret-color",
    ClipPath = 53 => "clip-path",
    Color = 54 => "color",
    ColumnGap = 55 => "column-gap",
    Cursor = 56 => "cursor",
    Direction = 57 => "direction",
    Display = 58 => "display",
    FlexBasis = 60 => "flex-basis",
    FlexDirection = 61 => "flex-direction",
    FlexGrow = 62 => "flex-grow",
    FlexShrink = 63 => "flex-shrink",
    FlexWrap = 64 => "flex-wrap",
    FontFamily = 65 => "font-family",
    FontSize = 66 => "font-size",
    FontStyle = 67 => "font-style",
    FontVariant = 68 => "font-variant",
    FontWeight = 69 => "font-weight",
    GridAutoColumns = 70 => "grid-auto-columns",
    GridAutoFlow = 71 => "grid-auto-flow",
    GridAutoRows = 72 => "grid-auto-rows",
    GridColumnEnd = 73 => "grid-column-end",
    GridColumnStart = 75 => "grid-column-start",
    GridRowEnd = 76 => "grid-row-end",
    GridRowStart = 78 => "grid-row-start",
    GridTemplateColumns = 79 => "grid-template-columns",
    GridTemplateRows = 80 => "grid-template-rows",
    Height = 81 => "height",
    InsetInlineEnd = 82 => "inset-inline-end",
    InsetInlineStart = 83 => "inset-inline-start",
    JustifyContent = 84 => "justify-content",
    Left = 85 => "left",
    LetterSpacing = 86 => "letter-spacing",
    LineHeight = 87 => "line-height",
    MarginBottom = 95 => "margin-bottom",
    MarginLeft = 96 => "margin-left",
    MarginRight = 97 => "margin-right",
    MarginTop = 98 => "margin-top",
    MaskImage = 99 => "mask-image",
    MaxHeight = 100 => "max-height",
    MaxWidth = 101 => "max-width",
    MinHeight = 102 => "min-height",
    MinWidth = 103 => "min-width",
    Opacity = 104 => "opacity",
    Order = 105 => "order",
    OutlineColor = 106 => "outline-color",
    OutlineOffset = 107 => "outline-offset",
    OutlineStyle = 108 => "outline-style",
    OutlineWidth = 109 => "outline-width",
    OverflowWrap = 110 => "overflow-wrap",
    OverflowX = 111 => "overflow-x",
    OverflowY = 112 => "overflow-y",
    PaddingBottom = 113 => "padding-bottom",
    PaddingLeft = 114 => "padding-left",
    PaddingRight = 115 => "padding-right",
    PaddingTop = 116 => "padding-top",
    Perspective = 117 => "perspective",
    PerspectiveOrigin = 118 => "perspective-origin",
    PointerEvents = 119 => "pointer-events",
    Position = 120 => "position",
    Right = 138 => "right",
    RowGap = 139 => "row-gap",
    TextAlign = 140 => "text-align",
    TextDecorationColor = 141 => "text-decoration-color",
    TextDecorationLine = 142 => "text-decoration-line",
    TextDecorationStyle = 143 => "text-decoration-style",
    TextDecorationThickness = 144 => "text-decoration-thickness",
    TextIndent = 145 => "text-indent",
    TextOverflow = 146 => "text-overflow",
    TextTransform = 149 => "text-transform",
    Top = 150 => "top",
    Transform = 151 => "transform",
    TransformBox = 152 => "transform-box",
    TransformOrigin = 153 => "transform-origin",
    TransformStyle = 154 => "transform-style",
    Transition = 155 => "transition",
    TransitionDelay = 156 => "transition-delay",
    TransitionDuration = 157 => "transition-duration",
    TransitionProperty = 158 => "transition-property",
    TransitionTimingFunction = 159 => "transition-timing-function",
    VerticalAlign = 160 => "vertical-align",
    Visibility = 161 => "visibility",
    WhiteSpace = 162 => "white-space",
    Width = 163 => "width",
    WordBreak = 164 => "word-break",
    ZIndex = 166 => "z-index",
    Border = 167 => "border",
    BorderBottom = 168 => "border-bottom",
    BorderColor = 169 => "border-color",
    BorderEndEndRadius = 170 => "border-end-end-radius",
    BorderEndStartRadius = 171 => "border-end-start-radius",
    BorderInlineEndColor = 172 => "border-inline-end-color",
    BorderInlineEndStyle = 173 => "border-inline-end-style",
    BorderInlineEndWidth = 174 => "border-inline-end-width",
    BorderInlineStartColor = 175 => "border-inline-start-color",
    BorderInlineStartStyle = 176 => "border-inline-start-style",
    BorderInlineStartWidth = 177 => "border-inline-start-width",
    BorderLeft = 178 => "border-left",
    BorderRight = 179 => "border-right",
    BorderStartEndRadius = 180 => "border-start-end-radius",
    BorderStartStartRadius = 181 => "border-start-start-radius",
    BorderStyle = 182 => "border-style",
    BorderTop = 183 => "border-top",
    BorderWidth = 184 => "border-width",
    Flex = 185 => "flex",
    FlexFlow = 186 => "flex-flow",
    FontFeatureSettings = 187 => "font-feature-settings",
    FontOpticalSizing = 188 => "font-optical-sizing",
    FontVariationSettings = 189 => "font-variation-settings",
    Gap = 190 => "gap",
    GridColumn = 191 => "grid-column",
    GridRow = 192 => "grid-row",
    ImageRendering = 193 => "image-rendering",
    JustifyItems = 194 => "justify-items",
    JustifySelf = 195 => "justify-self",
    Margin = 196 => "margin",
    MarginInlineEnd = 197 => "margin-inline-end",
    MarginInlineStart = 198 => "margin-inline-start",
    Mask = 199 => "mask",
    MaskComposite = 200 => "mask-composite",
    OffsetDistance = 201 => "offset-distance",
    OffsetPath = 202 => "offset-path",
    OffsetRotate = 203 => "offset-rotate",
    Overflow = 204 => "overflow",
    Padding = 205 => "padding",
    PaddingInlineEnd = 206 => "padding-inline-end",
    PaddingInlineStart = 207 => "padding-inline-start",
    TextDecoration = 208 => "text-decoration",
    TextShadow = 209 => "text-shadow",
    GridTemplateAreas = 210 => "grid-template-areas",
    Float = 211 => "float",
    Clear = 212 => "clear",
    BackdropFilter = 213 => "backdrop-filter",
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_ids_and_names_are_unique_and_round_trip() {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for &property in StyleProperty::ALL {
            assert!(ids.insert(property.id()));
            assert!(names.insert(property.css_name()));
            assert_eq!(StyleProperty::from_id(property.id()), Some(property));
            assert_eq!(
                StyleProperty::from_css_name(property.css_name()),
                Some(property)
            );
            assert_eq!(property.metadata().id, property.id());
        }
    }

    #[test]
    fn zero_and_unknown_ids_and_names_are_rejected() {
        assert_eq!(StylePropertyId::new(0), None);
        let unknown = StylePropertyId::new(u32::MAX).expect("non-zero");
        assert_eq!(StyleProperty::from_id(unknown), None);
        assert_eq!(StyleProperty::from_css_name("not-a-whisker-property"), None);
    }

    #[test]
    fn registry_contains_only_the_standard_property_target() {
        assert_eq!(StyleProperty::ALL.len(), 177);
        assert_eq!(StyleProperty::Color.metadata().origin, PropertyOrigin::Css);
        assert!(
            StyleProperty::ALL
                .iter()
                .all(|property| property.metadata().origin == PropertyOrigin::Css)
        );
        assert_eq!(StyleProperty::from_css_name("grid-column-gap"), None);
        assert_eq!(StyleProperty::from_css_name("grid-row-gap"), None);
        assert_eq!(StyleProperty::from_css_name("word-wrap"), None);
        assert_eq!(StyleProperty::from_css_name("-x-auto-font-size"), None);
        assert_eq!(StyleProperty::from_css_name("text-stroke-width"), None);
    }

    #[test]
    fn inheritance_is_limited_to_the_rfc_whitelist() {
        let inherited: Vec<_> = StyleProperty::ALL
            .iter()
            .copied()
            .filter(|property| property.metadata().inherited)
            .collect();
        assert_eq!(
            inherited,
            [
                StyleProperty::Color,
                StyleProperty::FontFamily,
                StyleProperty::FontSize,
                StyleProperty::FontStyle,
                StyleProperty::FontWeight,
                StyleProperty::LetterSpacing,
                StyleProperty::LineHeight,
                StyleProperty::TextShadow,
            ]
        );
        assert!(!StyleProperty::Opacity.metadata().inherited);
    }

    #[test]
    fn every_standard_property_has_a_semantic_destination() {
        for property in StyleProperty::ALL {
            let metadata = property.metadata();
            assert_ne!(
                metadata.domain,
                StylePropertyDomain::CompatibilityExtension,
                "unexpected destination for {}",
                metadata.css_name
            );
        }
    }
}
