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
    /// A standard CSS property also supported by Lynx.
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
                let origin = match self {
                    Self::WebkitLineClamp => PropertyOrigin::Webkit,
                    Self::XAutoFontSize
                    | Self::XAutoFontSizePresetSizes
                    | Self::XCaretWidth
                    | Self::XHandleColor
                    | Self::XHandleSize => PropertyOrigin::Lynx,
                    Self::LinearCrossGravity
                    | Self::LinearDirection
                    | Self::LinearGravity
                    | Self::LinearLayoutGravity
                    | Self::LinearOrientation
                    | Self::LinearWeight
                    | Self::LinearWeightSum
                    | Self::RelativeAlignBottom
                    | Self::RelativeAlignInlineEnd
                    | Self::RelativeAlignInlineStart
                    | Self::RelativeAlignLeft
                    | Self::RelativeAlignRight
                    | Self::RelativeAlignTop
                    | Self::RelativeBottomOf
                    | Self::RelativeCenter
                    | Self::RelativeCenterHorizontal
                    | Self::RelativeCenterVertical
                    | Self::RelativeId
                    | Self::RelativeInlineEndOf
                    | Self::RelativeInlineStartOf
                    | Self::RelativeLayoutOnce
                    | Self::RelativeLeftOf
                    | Self::RelativeRightOf
                    | Self::RelativeTopOf => PropertyOrigin::LynxUnprefixed,
                    _ => PropertyOrigin::Css,
                };
                let inherited = matches!(
                    self,
                    Self::FontFamily
                        | Self::FontSize
                        | Self::FontWeight
                        | Self::FontStyle
                        | Self::LineHeight
                        | Self::LetterSpacing
                        | Self::Color
                );
                PropertyMetadata {
                    id: self.id(),
                    css_name: self.css_name(),
                    origin,
                    inherited,
                }
            }
        }
    };
}

define_style_properties! {
    WebkitLineClamp = 1 => "-webkit-line-clamp",
    XAutoFontSize = 2 => "-x-auto-font-size",
    XAutoFontSizePresetSizes = 3 => "-x-auto-font-size-preset-sizes",
    XCaretWidth = 4 => "-x-caret-width",
    XHandleColor = 5 => "-x-handle-color",
    XHandleSize = 6 => "-x-handle-size",
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
    Filter = 59 => "filter",
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
    GridColumnGap = 74 => "grid-column-gap",
    GridColumnStart = 75 => "grid-column-start",
    GridRowEnd = 76 => "grid-row-end",
    GridRowGap = 77 => "grid-row-gap",
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
    LinearCrossGravity = 88 => "linear-cross-gravity",
    LinearDirection = 89 => "linear-direction",
    LinearGravity = 90 => "linear-gravity",
    LinearLayoutGravity = 91 => "linear-layout-gravity",
    LinearOrientation = 92 => "linear-orientation",
    LinearWeight = 93 => "linear-weight",
    LinearWeightSum = 94 => "linear-weight-sum",
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
    RelativeAlignBottom = 121 => "relative-align-bottom",
    RelativeAlignInlineEnd = 122 => "relative-align-inline-end",
    RelativeAlignInlineStart = 123 => "relative-align-inline-start",
    RelativeAlignLeft = 124 => "relative-align-left",
    RelativeAlignRight = 125 => "relative-align-right",
    RelativeAlignTop = 126 => "relative-align-top",
    RelativeBottomOf = 127 => "relative-bottom-of",
    RelativeCenter = 128 => "relative-center",
    RelativeCenterHorizontal = 129 => "relative-center-horizontal",
    RelativeCenterVertical = 130 => "relative-center-vertical",
    RelativeId = 131 => "relative-id",
    RelativeInlineEndOf = 132 => "relative-inline-end-of",
    RelativeInlineStartOf = 133 => "relative-inline-start-of",
    RelativeLayoutOnce = 134 => "relative-layout-once",
    RelativeLeftOf = 135 => "relative-left-of",
    RelativeRightOf = 136 => "relative-right-of",
    RelativeTopOf = 137 => "relative-top-of",
    Right = 138 => "right",
    RowGap = 139 => "row-gap",
    TextAlign = 140 => "text-align",
    TextDecorationColor = 141 => "text-decoration-color",
    TextDecorationLine = 142 => "text-decoration-line",
    TextDecorationStyle = 143 => "text-decoration-style",
    TextDecorationThickness = 144 => "text-decoration-thickness",
    TextIndent = 145 => "text-indent",
    TextOverflow = 146 => "text-overflow",
    TextStrokeColor = 147 => "text-stroke-color",
    TextStrokeWidth = 148 => "text-stroke-width",
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
    WordWrap = 165 => "word-wrap",
    ZIndex = 166 => "z-index",
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
    fn origin_categories_cover_compatibility_extensions() {
        assert_eq!(StyleProperty::Color.metadata().origin, PropertyOrigin::Css);
        assert_eq!(
            StyleProperty::WebkitLineClamp.metadata().origin,
            PropertyOrigin::Webkit
        );
        assert_eq!(
            StyleProperty::XHandleSize.metadata().origin,
            PropertyOrigin::Lynx
        );
        assert_eq!(
            StyleProperty::RelativeId.metadata().origin,
            PropertyOrigin::LynxUnprefixed
        );
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
            ]
        );
        assert!(!StyleProperty::Opacity.metadata().inherited);
    }
}
