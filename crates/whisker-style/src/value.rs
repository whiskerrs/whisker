//! Renderer-independent values stored in specified style declarations.

/// An `f32` with equality and hashing defined by its IEEE-754 bit pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StyleNumber(u32);

impl StyleNumber {
    /// Stores a number without changing its representation.
    pub const fn new(value: f32) -> Self {
        Self(value.to_bits())
    }

    /// Returns the stored number.
    pub const fn get(self) -> f32 {
        f32::from_bits(self.0)
    }
}

/// A unit accepted by Whisker's length model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthUnit {
    /// Logical pixels: iOS points and Android density-independent pixels.
    Px,
    /// Units relative to the element's computed font size.
    Em,
    /// Units relative to the root computed font size.
    Rem,
    /// Percent of viewport height.
    Vh,
    /// Percent of viewport width.
    Vw,
}

/// A semantic length that does not require CSS parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthValue {
    /// Unitless zero.
    Zero,
    /// A number paired with an explicit unit.
    Dimension {
        /// Numeric magnitude.
        value: StyleNumber,
        /// Length unit.
        unit: LengthUnit,
    },
}

/// A semantic length-or-percentage value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LengthPercentageValue {
    /// Absolute or environment-relative length.
    Length(LengthValue),
    /// Percentage number before the `%` suffix.
    Percentage(StyleNumber),
    /// Typed arithmetic expression.
    Calc(Box<CalcExpression>),
}

/// A typed arithmetic expression used by length-percentage values.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CalcExpression {
    /// Length-percentage operand.
    Value(Box<LengthPercentageValue>),
    /// Unitless numeric operand.
    Number(StyleNumber),
    /// Typed custom-property operand retained until computed-style resolution.
    Variable(CustomPropertyReference),
    /// Addition.
    Add(Box<Self>, Box<Self>),
    /// Subtraction.
    Sub(Box<Self>, Box<Self>),
    /// Multiplication.
    Mul(Box<Self>, Box<Self>),
    /// Division.
    Div(Box<Self>, Box<Self>),
}

/// One typed component inside a composite specified value.
///
/// Variables are removed during computed-style materialization, before layout
/// or paint resolution observes the enclosing value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ComponentValue<T> {
    /// A literal typed component.
    Value(T),
    /// A typed custom-property reference.
    Variable(CustomPropertyReference),
}

impl<T> ComponentValue<T> {
    /// Returns the literal value after custom-property materialization.
    pub const fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Variable(_) => None,
        }
    }
}

impl<T> From<T> for ComponentValue<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

/// A font family selected by application code or by the platform default.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontFamilyValue {
    /// The platform's default application font.
    System,
    /// One explicitly named font family.
    Named(String),
}

/// A validated four-byte printable ASCII OpenType tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpenTypeTagValue([u8; 4]);

impl OpenTypeTagValue {
    /// Creates a tag from exactly four printable ASCII bytes.
    pub const fn new(value: [u8; 4]) -> Option<Self> {
        let mut index = 0;
        while index < value.len() {
            if value[index] < 0x20 || value[index] > 0x7e {
                return None;
            }
            index += 1;
        }
        Some(Self(value))
    }

    /// Returns the exact OpenType bytes.
    pub const fn get(self) -> [u8; 4] {
        self.0
    }
}

/// One CSS `font-feature-settings` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontFeatureValue {
    /// OpenType feature tag.
    pub tag: OpenTypeTagValue,
    /// Non-negative selector; `0` and `1` represent `off` and `on`.
    pub value: u32,
}

/// One CSS `font-variation-settings` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontVariationValue {
    /// OpenType variation-axis tag.
    pub tag: OpenTypeTagValue,
    /// Axis value preserved for computed-style validation.
    pub value: StyleNumber,
}

/// Lynx `font-optical-sizing` behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontOpticalSizingValue {
    /// Enable the font's optical-size axis using the computed font size.
    Auto,
    /// Do not synthesize an optical-size axis. This is Lynx's initial value.
    #[default]
    None,
}

/// Keyword-only cursor values supported by Lynx and the Whisker Hosts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorValue {
    /// Let the Host choose the cursor for the element.
    #[default]
    Auto,
    /// Platform default arrow or equivalent.
    Default,
    /// Hide the pointing-device cursor.
    None,
    /// Context-menu affordance.
    ContextMenu,
    /// Help affordance.
    Help,
    /// Pointing or link affordance.
    Pointer,
    /// Progress without blocking interaction.
    Progress,
    /// Busy or waiting affordance.
    Wait,
    /// Cell selection.
    Cell,
    /// Crosshair.
    Crosshair,
    /// Horizontal text selection.
    Text,
    /// Vertical text selection.
    VerticalText,
    /// Alias creation.
    Alias,
    /// Copy affordance.
    Copy,
    /// Move affordance.
    Move,
    /// A drop is prohibited.
    NoDrop,
    /// Interaction is prohibited.
    NotAllowed,
    /// Grab affordance.
    Grab,
    /// Active grab affordance.
    Grabbing,
    /// Column resize.
    ColResize,
    /// Row resize.
    RowResize,
    /// North resize.
    NResize,
    /// East resize.
    EResize,
    /// South resize.
    SResize,
    /// West resize.
    WResize,
    /// North-east resize.
    NeResize,
    /// North-west resize.
    NwResize,
    /// South-east resize.
    SeResize,
    /// South-west resize.
    SwResize,
    /// Bidirectional east-west resize.
    EwResize,
    /// Bidirectional north-south resize.
    NsResize,
    /// Bidirectional north-east/south-west resize.
    NeswResize,
    /// Bidirectional north-west/south-east resize.
    NwseResize,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
}

/// Whether a node and its descendants participate in pointer hit testing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PointerEventsValue {
    /// Use normal Host hit testing.
    #[default]
    Auto,
    /// Neither the node nor its descendants receives pointer input.
    None,
}

/// Lynx-supported inline text alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlignValue {
    /// Logical inline start.
    #[default]
    Start,
    /// Logical inline end.
    End,
    /// Physical left edge.
    Left,
    /// Physical right edge.
    Right,
    /// Center each line.
    Center,
}

/// Lynx-supported `white-space` behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WhiteSpaceValue {
    /// Collapse whitespace and allow line wrapping.
    #[default]
    Normal,
    /// Collapse whitespace and keep the text on one logical line.
    NoWrap,
}

/// Lynx-supported line-breaking behavior within words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WordBreakValue {
    /// Use the Host's Unicode line-breaking rules.
    #[default]
    Normal,
    /// Permit a line break between any pair of characters.
    BreakAll,
    /// Suppress ordinary break opportunities inside CJK text.
    KeepAll,
}

/// Lynx-supported treatment of text that exceeds its line limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextOverflowValue {
    /// Clip glyphs at the text content boundary.
    #[default]
    Clip,
    /// Replace the end of the last visible line with an ellipsis.
    Ellipsis,
}

/// The face style used to render text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontStyleValue {
    /// An upright face.
    Normal,
    /// An italic face.
    Italic,
    /// An oblique or synthesized slanted face.
    Oblique,
}

/// A numeric font weight in the CSS-compatible `1..=1000` range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontWeightValue(u16);

impl FontWeightValue {
    /// The normal font weight.
    pub const NORMAL: Self = Self(400);

    /// The bold font weight.
    pub const BOLD: Self = Self(700);

    /// Creates a font weight when it is in the supported range.
    pub const fn new(value: u16) -> Option<Self> {
        if value >= 1 && value <= 1000 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Preserves an authoring value for later resolver validation.
    ///
    /// Prefer [`Self::new`] outside compatibility adapters.
    pub const fn from_raw(value: u16) -> Self {
        Self(value)
    }

    /// Returns the numeric weight.
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A semantic color that does not require parsing CSS text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColorValue {
    /// A named color accepted by Whisker's typed authoring API.
    Named(String),
    /// An sRGB color with a floating-point alpha channel.
    Rgba {
        /// Red channel.
        red: u8,
        /// Green channel.
        green: u8,
        /// Blue channel.
        blue: u8,
        /// Alpha in the range `0.0..=1.0`.
        alpha: StyleNumber,
    },
    /// An HSL color, normalized to degrees and percentages.
    Hsla {
        /// Hue in degrees.
        hue_degrees: StyleNumber,
        /// Saturation percentage.
        saturation: StyleNumber,
        /// Lightness percentage.
        lightness: StyleNumber,
        /// Alpha in the range `0.0..=1.0`.
        alpha: StyleNumber,
    },
}

/// A specified line height before environment and font-size resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LineHeightValue {
    /// Let the platform text shaper select its normal line height.
    Normal,
    /// A multiplier of the node's computed font size.
    Number(StyleNumber),
    /// An explicit length or percentage of the node's computed font size.
    LengthPercentage(LengthPercentageValue),
}

/// A specified corner radius with independent horizontal and vertical axes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BorderRadiusValue {
    /// Radius resolved against the border box width.
    pub horizontal: LengthPercentageValue,
    /// Radius resolved against the border box height.
    pub vertical: LengthPercentageValue,
}

/// One specified background image that does not contain Host resource IDs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundImageValue {
    /// An explicit `none` layer retained for future list alignment.
    None,
    /// URL text resolved under Host resource policy.
    Url(String),
    /// A procedural gradient resolved without Host resource loading.
    Gradient(GradientValue),
}

/// One specified gradient color stop.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GradientStopValue {
    /// Stop color.
    pub color: ComponentValue<ColorValue>,
    /// Optional distance along the gradient line.
    pub position: Option<LengthPercentageValue>,
}

/// Shape and optional explicit radii of a radial gradient.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RadialGradientValue {
    /// Circle using the CSS farthest-corner extent.
    Circle,
    /// Ellipse using the CSS farthest-corner extent.
    Ellipse,
    /// Circle with an explicit radius.
    CircleSized(LengthPercentageValue),
    /// Ellipse with explicit horizontal and vertical radii.
    EllipseSized(LengthPercentageValue, LengthPercentageValue),
}

/// A specified procedural gradient image.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GradientValue {
    /// Linear gradient with a clockwise angle from the positive vertical axis.
    Linear {
        /// Direction in degrees.
        angle_degrees: ComponentValue<StyleNumber>,
        /// Ordered stops.
        stops: Vec<GradientStopValue>,
    },
    /// Radial gradient centered in its image box.
    Radial {
        /// Shape and sizing rule.
        shape: RadialGradientValue,
        /// Ordered stops.
        stops: Vec<GradientStopValue>,
    },
    /// Conic gradient around a configurable center.
    Conic {
        /// Starting angle in degrees.
        from_degrees: ComponentValue<StyleNumber>,
        /// Center in the image box.
        center: BackgroundPositionValue,
        /// Ordered stops.
        stops: Vec<GradientStopValue>,
    },
}

/// Tiling behavior on one background axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundRepeatModeValue {
    /// Repeat adjacent tiles without adding space.
    Repeat,
    /// Paint at most one tile.
    NoRepeat,
    /// Distribute whole tiles with equal gaps.
    Space,
    /// Resize tiles so a whole number fills the axis.
    Round,
}

/// Expanded two-axis value of one `background-repeat` layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BackgroundRepeatValue {
    /// Horizontal tiling behavior.
    pub horizontal: BackgroundRepeatModeValue,
    /// Vertical tiling behavior.
    pub vertical: BackgroundRepeatModeValue,
}

/// Specified position of one background layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BackgroundPositionValue {
    /// Horizontal position relative to the positioning area.
    pub horizontal: LengthPercentageValue,
    /// Vertical position relative to the positioning area.
    pub vertical: LengthPercentageValue,
}

/// Specified size of one background layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundSizeValue {
    /// Use the image's intrinsic dimensions.
    Auto,
    /// Preserve aspect ratio while covering the positioning area.
    Cover,
    /// Preserve aspect ratio while fitting inside the positioning area.
    Contain,
    /// Resolve an explicit width and height.
    Explicit {
        /// Specified image width, or intrinsic width for `auto`.
        width: Option<LengthPercentageValue>,
        /// Specified image height, or intrinsic height for `auto`.
        height: Option<LengthPercentageValue>,
    },
}

/// Box used to position or clip a background layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundBoxValue {
    /// The outer border box.
    Border,
    /// The inner padding box.
    Padding,
    /// The content box.
    Content,
    /// The area painted by the border.
    BorderArea,
}

/// Scrolling relationship of one background layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundAttachmentValue {
    /// Scroll with the element's border box.
    Scroll,
    /// Remain fixed relative to the viewport.
    Fixed,
    /// Scroll with the element's contents.
    Local,
}

/// One fully expanded background layer before environment resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BackgroundLayerValue {
    /// Image source painted by this layer.
    pub image: BackgroundImageValue,
    /// Position within the selected origin box.
    pub position: BackgroundPositionValue,
    /// Image sizing behavior.
    pub size: BackgroundSizeValue,
    /// Two-axis tiling behavior.
    pub repeat: BackgroundRepeatValue,
    /// Box defining the positioning area.
    pub origin: BackgroundBoxValue,
    /// Box clipping background paint.
    pub clip: BackgroundBoxValue,
    /// Relationship to scrolling.
    pub attachment: BackgroundAttachmentValue,
}

/// Expanded semantic value of the `background` shorthand.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BackgroundValue {
    /// Ordered image layers, front to back.
    pub layers: Vec<BackgroundLayerValue>,
    /// The shorthand's resolved specified color, including transparent default.
    pub color: ComponentValue<ColorValue>,
}

/// Supported subset of the CSS `backdrop-filter` property.
///
/// Whisker intentionally supports only `none` and one `blur(<length>)`
/// function. Color transforms and filter chains remain out of scope.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackdropFilterValue {
    /// Do not filter the pixels behind the element.
    None,
    /// Apply a Gaussian blur with the specified non-negative radius.
    Blur(ComponentValue<LengthValue>),
}

/// Raster-image scaling algorithm supported by Lynx-compatible styling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageRenderingValue {
    /// Use the Host's normal interpolation policy.
    #[default]
    Auto,
    /// Preserve hard source-pixel edges with nearest-neighbor sampling.
    Pixelated,
    /// Lynx-compatible keyword currently equivalent to `auto`.
    CrispEdges,
}

/// One specified box shadow before environment-dependent lengths are resolved.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BoxShadowValue {
    /// Horizontal offset.
    pub offset_x: ComponentValue<LengthValue>,
    /// Vertical offset.
    pub offset_y: ComponentValue<LengthValue>,
    /// Non-negative blur radius.
    pub blur_radius: ComponentValue<LengthValue>,
    /// Signed spread radius.
    pub spread_radius: ComponentValue<LengthValue>,
    /// Shadow color.
    pub color: ComponentValue<ColorValue>,
    /// Paint inside the border box when true.
    pub inset: bool,
}

/// Reference box used to resolve a basic-shape clip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClipBoxValue {
    /// Border box.
    #[default]
    Border,
    /// Padding box.
    Padding,
    /// Content box.
    Content,
    /// Object bounding box for vector content.
    Fill,
    /// Stroke bounding box for vector content.
    Stroke,
    /// Nearest vector viewport box.
    View,
}

/// Fill rule used by clip paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClipFillRuleValue {
    /// Non-zero winding rule.
    #[default]
    NonZero,
    /// Even-odd winding rule.
    EvenOdd,
}

/// One point in a specified clip path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClipPointValue {
    /// Horizontal coordinate.
    pub x: LengthPercentageValue,
    /// Vertical coordinate.
    pub y: LengthPercentageValue,
}

/// One command in a structured clip path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ClipPathCommandValue {
    /// Start a new subpath.
    MoveTo(ClipPointValue),
    /// Add a straight segment.
    LineTo(ClipPointValue),
    /// Add a quadratic Bezier segment.
    QuadraticTo {
        /// Control point.
        control: ClipPointValue,
        /// Segment endpoint.
        end: ClipPointValue,
    },
    /// Add a cubic Bezier segment.
    CubicTo {
        /// First control point.
        control_1: ClipPointValue,
        /// Second control point.
        control_2: ClipPointValue,
        /// Segment endpoint.
        end: ClipPointValue,
    },
    /// Close the current subpath.
    Close,
}

/// A specified basic shape used by `clip-path`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ClipShapeValue {
    /// Rectangle inset from the reference box.
    Inset {
        /// Top, right, bottom, and left offsets.
        offsets: [LengthPercentageValue; 4],
        /// Optional per-corner radii.
        radii: Option<[BorderRadiusValue; 4]>,
    },
    /// Circle.
    Circle {
        /// Radius.
        radius: LengthPercentageValue,
        /// Horizontal center.
        center_x: LengthPercentageValue,
        /// Vertical center.
        center_y: LengthPercentageValue,
    },
    /// Ellipse.
    Ellipse {
        /// Horizontal radius.
        radius_x: LengthPercentageValue,
        /// Vertical radius.
        radius_y: LengthPercentageValue,
        /// Horizontal center.
        center_x: LengthPercentageValue,
        /// Vertical center.
        center_y: LengthPercentageValue,
    },
    /// Structured path commands.
    Path {
        /// Fill rule.
        fill_rule: ClipFillRuleValue,
        /// Command stream.
        commands: Vec<ClipPathCommandValue>,
    },
}

/// Typed `clip-path` value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClipPathValue {
    /// Disable shape clipping.
    #[default]
    None,
    /// Clip to a shape resolved against a reference box.
    Shape {
        /// Reference box.
        reference_box: ClipBoxValue,
        /// Basic shape.
        shape: ClipShapeValue,
    },
}

/// One typed transform function before environment and box-size resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransformFunctionValue {
    /// Two-axis translation.
    Translate(LengthPercentageValue, LengthPercentageValue),
    /// Horizontal translation.
    TranslateX(LengthPercentageValue),
    /// Vertical translation.
    TranslateY(LengthPercentageValue),
    /// Depth translation.
    TranslateZ(ComponentValue<LengthValue>),
    /// Three-axis translation.
    Translate3d(
        LengthPercentageValue,
        LengthPercentageValue,
        ComponentValue<LengthValue>,
    ),
    /// Rotation around the z axis, in degrees.
    Rotate(ComponentValue<StyleNumber>),
    /// Rotation around the x axis, in degrees.
    RotateX(ComponentValue<StyleNumber>),
    /// Rotation around the y axis, in degrees.
    RotateY(ComponentValue<StyleNumber>),
    /// Rotation around the z axis, in degrees.
    RotateZ(ComponentValue<StyleNumber>),
    /// Two-axis scale.
    Scale(ComponentValue<StyleNumber>, ComponentValue<StyleNumber>),
    /// Horizontal scale.
    ScaleX(ComponentValue<StyleNumber>),
    /// Vertical scale.
    ScaleY(ComponentValue<StyleNumber>),
    /// Two-axis skew, in degrees.
    Skew(ComponentValue<StyleNumber>, ComponentValue<StyleNumber>),
    /// Horizontal skew, in degrees.
    SkewX(ComponentValue<StyleNumber>),
    /// Vertical skew, in degrees.
    SkewY(ComponentValue<StyleNumber>),
    /// CSS six-value affine matrix.
    Matrix([StyleNumber; 6]),
    /// Column-major four-by-four matrix.
    Matrix3d([StyleNumber; 16]),
}

/// Ordered typed value of the `transform` property.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TransformValue(pub Vec<TransformFunctionValue>);

/// Two-dimensional `transform-origin` before border-box resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TransformOriginValue {
    /// Horizontal origin coordinate.
    pub horizontal: LengthPercentageValue,
    /// Vertical origin coordinate.
    pub vertical: LengthPercentageValue,
}

/// One absolute point in a Lynx `offset-path: path()` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MotionPathPointValue {
    /// Horizontal logical-pixel coordinate.
    pub x: StyleNumber,
    /// Vertical logical-pixel coordinate.
    pub y: StyleNumber,
}

/// One command in an absolute SVG motion path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionPathCommandValue {
    /// Start a new subpath.
    MoveTo(MotionPathPointValue),
    /// Add a straight segment.
    LineTo(MotionPathPointValue),
    /// Add a quadratic Bezier segment.
    QuadraticTo {
        /// Curve control point.
        control: MotionPathPointValue,
        /// Segment endpoint.
        to: MotionPathPointValue,
    },
    /// Add a cubic Bezier segment.
    CubicTo {
        /// First curve control point.
        control1: MotionPathPointValue,
        /// Second curve control point.
        control2: MotionPathPointValue,
        /// Segment endpoint.
        to: MotionPathPointValue,
    },
    /// Add an absolute SVG elliptical arc segment.
    ArcTo {
        /// Horizontal ellipse radius.
        radius_x: StyleNumber,
        /// Vertical ellipse radius.
        radius_y: StyleNumber,
        /// Clockwise rotation of the ellipse x axis, in degrees.
        x_axis_rotation: StyleNumber,
        /// Select the arc spanning at least 180 degrees.
        large_arc: bool,
        /// Sweep through increasing angles.
        sweep: bool,
        /// Segment endpoint.
        to: MotionPathPointValue,
    },
    /// Close the current subpath with a straight segment.
    Close,
}

/// A specified `inset()` motion path before border-box resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InsetPathValue {
    /// Top, right, bottom, and left offsets from the border box.
    pub offsets: [LengthPercentageValue; 4],
    /// Optional top-left, top-right, bottom-right, and bottom-left radii.
    pub radii: Option<[BorderRadiusValue; 4]>,
}

/// Typed value of `offset-path`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum OffsetPathValue {
    /// Disable motion-path translation and rotation.
    #[default]
    None,
    /// Follow an absolute path in the node's local logical-pixel space.
    Path(Vec<MotionPathCommandValue>),
    /// Follow a circle resolved against the node border box.
    Circle {
        /// Radius; percentages use the normalized border-box diagonal.
        radius: LengthPercentageValue,
        /// Center position relative to border-box width.
        center_x: LengthPercentageValue,
        /// Center position relative to border-box height.
        center_y: LengthPercentageValue,
    },
    /// Follow an ellipse resolved against the node border box.
    Ellipse {
        /// Horizontal radius relative to border-box width.
        radius_x: LengthPercentageValue,
        /// Vertical radius relative to border-box height.
        radius_y: LengthPercentageValue,
        /// Center position relative to border-box width.
        center_x: LengthPercentageValue,
        /// Center position relative to border-box height.
        center_y: LengthPercentageValue,
    },
    /// Follow a possibly-rounded rectangle inset from the node border box.
    Inset(Box<InsetPathValue>),
}

/// Typed value of `offset-rotate`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OffsetRotateValue {
    /// Follow the path tangent.
    #[default]
    Auto,
    /// Use a fixed clockwise angle in degrees.
    Angle(StyleNumber),
}

/// The Lynx-compatible single `text-shadow` value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextShadowValue {
    /// Disable inherited text shadow paint.
    None,
    /// Paint one shadow behind the glyphs.
    Shadow {
        /// Horizontal offset.
        offset_x: ComponentValue<LengthValue>,
        /// Vertical offset.
        offset_y: ComponentValue<LengthValue>,
        /// Non-negative blur radius.
        blur_radius: ComponentValue<LengthValue>,
        /// Shadow color.
        color: ComponentValue<ColorValue>,
    },
}

/// The single decoration line supported by Lynx's `text-decoration` shorthand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextDecorationLineValue {
    /// No decoration.
    None,
    /// A line below the glyphs.
    Underline,
    /// A line through the glyphs.
    LineThrough,
}

/// Lynx-compatible decoration stroke style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextDecorationStyleValue {
    /// One solid line.
    Solid,
    /// Two parallel lines.
    Double,
    /// A sequence of dots.
    Dotted,
    /// A sequence of dashes.
    Dashed,
    /// A wave-shaped line.
    Wavy,
}

/// Typed value for Lynx's inherited, single-line `text-decoration` shorthand.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextDecorationValue {
    /// Selected line kind.
    pub line: TextDecorationLineValue,
    /// Selected stroke style.
    pub style: TextDecorationStyleValue,
    /// Explicit decoration color, or the resolved text color when omitted.
    pub color: Option<ComponentValue<ColorValue>>,
}

/// An owned value in a specified inline-style declaration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StyleValue {
    /// A reference to an inherited custom property, with an optional fallback.
    Variable(CustomPropertyReference),
    /// Boolean value.
    Bool(bool),
    /// Signed integer value.
    Integer(i64),
    /// Unitless real number.
    Number(StyleNumber),
    /// Angle normalized to clockwise degrees for custom-property storage.
    Angle(StyleNumber),
    /// UTF-8 text whose interpretation is defined by its property schema.
    Text(String),
    /// Length value.
    Length(LengthValue),
    /// Length, percentage, or typed `calc` expression.
    LengthPercentage(LengthPercentageValue),
    /// One border corner's horizontal and vertical radii.
    BorderRadius(BorderRadiusValue),
    /// Ordered background images, front to back.
    BackgroundImages(Vec<BackgroundImageValue>),
    /// Fully expanded `background` shorthand.
    Background(BackgroundValue),
    /// Expanded tiling behavior of one background layer.
    BackgroundRepeat(BackgroundRepeatValue),
    /// Specified position of one background layer.
    BackgroundPosition(BackgroundPositionValue),
    /// Specified size of one background layer.
    BackgroundSize(BackgroundSizeValue),
    /// Positioning or clipping box of one background layer.
    BackgroundBox(BackgroundBoxValue),
    /// Scrolling relationship of one background layer.
    BackgroundAttachment(BackgroundAttachmentValue),
    /// Background-pixel filter applied behind this node.
    BackdropFilter(BackdropFilterValue),
    /// Raster-image sampling behavior for this element's own image paint.
    ImageRendering(ImageRenderingValue),
    /// Ordered box shadows, front to back.
    BoxShadows(Vec<BoxShadowValue>),
    /// Basic-shape clip applied to this node.
    ClipPath(ClipPathValue),
    /// Ordered transform function list.
    Transform(TransformValue),
    /// Transform origin resolved against the node border box.
    TransformOrigin(TransformOriginValue),
    /// Motion path followed by the current node.
    OffsetPath(OffsetPathValue),
    /// Motion-path tangent or fixed rotation.
    OffsetRotate(OffsetRotateValue),
    /// Font family.
    FontFamily(FontFamilyValue),
    /// OpenType feature settings. An empty vector is CSS `normal`.
    FontFeatures(Vec<FontFeatureValue>),
    /// Variable-font axis settings. An empty vector is CSS `normal`.
    FontVariations(Vec<FontVariationValue>),
    /// Optical sizing behavior.
    FontOpticalSizing(FontOpticalSizingValue),
    /// Keyword-only pointing-device cursor.
    Cursor(CursorValue),
    /// Pointer hit-test participation.
    PointerEvents(PointerEventsValue),
    /// Fully expanded `transition` shorthand layers.
    Transitions(Vec<crate::TransitionValue>),
    /// `transition-property` list.
    TransitionProperties(Vec<crate::TransitionPropertyValue>),
    /// `transition-duration` list.
    TransitionDurations(Vec<crate::MotionTime>),
    /// `transition-timing-function` list.
    TransitionEasings(Vec<crate::MotionEasing>),
    /// `transition-delay` list.
    TransitionDelays(Vec<crate::MotionTime>),
    /// Fully expanded `animation` shorthand layers.
    Animations(Vec<crate::AnimationValue>),
    /// `animation-name` list; `None` is the CSS `none` keyword.
    AnimationNames(Vec<Option<String>>),
    /// `animation-duration` list.
    AnimationDurations(Vec<crate::MotionTime>),
    /// `animation-timing-function` list.
    AnimationEasings(Vec<crate::MotionEasing>),
    /// `animation-delay` list.
    AnimationDelays(Vec<crate::MotionTime>),
    /// `animation-iteration-count` list.
    AnimationIterationCounts(Vec<crate::MotionIterationCount>),
    /// `animation-direction` list.
    AnimationDirections(Vec<crate::MotionDirection>),
    /// `animation-fill-mode` list.
    AnimationFillModes(Vec<crate::MotionFillMode>),
    /// `animation-play-state` list.
    AnimationPlayStates(Vec<crate::MotionPlayState>),
    /// Font face style.
    FontStyle(FontStyleValue),
    /// Numeric font weight.
    FontWeight(FontWeightValue),
    /// Text color.
    Color(ColorValue),
    /// Inline text alignment.
    TextAlign(TextAlignValue),
    /// Whitespace collapse and wrapping policy.
    WhiteSpace(WhiteSpaceValue),
    /// Word-breaking policy.
    WordBreak(WordBreakValue),
    /// Text overflow treatment.
    TextOverflow(TextOverflowValue),
    /// Single inherited text shadow.
    TextShadow(TextShadowValue),
    /// Single inherited Lynx text decoration.
    TextDecoration(TextDecorationValue),
    /// Border line style.
    BorderStyle(crate::BorderStyleValue),
    /// Overflow behavior on one axis.
    Overflow(crate::OverflowValue),
    /// Paint visibility.
    Visibility(crate::VisibilityValue),
    /// Line height.
    LineHeight(LineHeightValue),
    /// Layout algorithm.
    Display(crate::DisplayValue),
    /// Block-layout float side.
    Float(crate::FloatValue),
    /// Block-layout clearance rule.
    Clear(crate::ClearValue),
    /// Positioning model.
    Position(crate::PositionValue),
    /// Box sizing model.
    BoxSizing(crate::BoxSizingValue),
    /// Inline writing direction.
    Direction(crate::DirectionValue),
    /// Width or height constraint.
    Size(crate::SizeValue),
    /// Margin or inset value.
    LengthPercentageAuto(crate::LengthPercentageAutoValue),
    /// Flex main-axis direction.
    FlexDirection(crate::FlexDirectionValue),
    /// Flex wrapping behavior.
    FlexWrap(crate::FlexWrapValue),
    /// Flex basis.
    FlexBasis(crate::FlexBasisValue),
    /// Main-axis distribution.
    JustifyContent(crate::JustifyContentValue),
    /// Cross-axis item alignment.
    AlignItems(crate::AlignItemsValue),
    /// Per-item cross-axis alignment.
    AlignSelf(crate::AlignSelfValue),
    /// Cross-axis line distribution.
    AlignContent(crate::AlignContentValue),
    /// Explicit Grid track template for one axis.
    GridTemplate(crate::GridTemplateValue),
    /// Implicit Grid track sizing functions for one axis.
    GridTracks(Vec<crate::GridTrackSizingValue>),
    /// Grid auto-placement direction and density.
    GridAutoFlow(crate::GridAutoFlowValue),
    /// One Grid item edge placement.
    GridPlacement(crate::GridPlacementValue),
    /// Named Grid area rectangles.
    GridTemplateAreas(crate::GridTemplateAreasValue),
    /// Width-to-height ratio.
    AspectRatio(crate::AspectRatioValue),
}

/// A case-sensitive CSS custom-property name such as `--spacing`.
///
/// Whisker preserves the spelling because custom properties are not entries in
/// the fixed [`StyleProperty`](crate::StyleProperty) registry.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CustomPropertyName(String);

impl CustomPropertyName {
    /// Validates and owns a custom-property name.
    ///
    /// The common CSS identifier form is accepted, including non-ASCII
    /// characters. Whitespace, control characters, and a bare `--` are
    /// rejected so invalid names cannot enter computed style.
    pub fn new(name: impl Into<String>) -> Option<Self> {
        let name = name.into();
        let mut suffix = name.strip_prefix("--")?.chars();
        let first = suffix.next()?;
        if !is_custom_property_ident_character(first)
            || suffix.any(|character| !is_custom_property_ident_character(character))
        {
            return None;
        }
        Some(Self(name))
    }

    /// Returns the exact case-sensitive property name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_custom_property_ident_character(character: char) -> bool {
    character == '_'
        || character == '-'
        || character.is_ascii_alphanumeric()
        || !character.is_ascii()
}

/// A whole-value `var()` reference retained until computed-style resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CustomPropertyReference {
    /// Referenced custom-property name.
    pub name: CustomPropertyName,
    /// Value used when the reference is missing, invalid, or cyclic.
    pub fallback: Option<Box<StyleValue>>,
}

impl CustomPropertyReference {
    /// Creates `var(<name>)` without a fallback.
    pub fn new(name: CustomPropertyName) -> Self {
        Self {
            name,
            fallback: None,
        }
    }

    /// Creates `var(<name>, <fallback>)`.
    pub fn with_fallback(name: CustomPropertyName, fallback: StyleValue) -> Self {
        Self {
            name,
            fallback: Some(Box::new(fallback)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn style_number_preserves_bits_and_hashes() {
        let number = StyleNumber::new(-0.0);
        assert_eq!(number.get().to_bits(), (-0.0_f32).to_bits());
        let mut values = HashSet::new();
        assert!(values.insert(number));
        assert!(!values.insert(number));
    }

    #[test]
    fn semantic_values_clone_and_compare_without_text() {
        let length = LengthValue::Dimension {
            value: StyleNumber::new(12.5),
            unit: LengthUnit::Px,
        };
        let calc = CalcExpression::Add(
            Box::new(CalcExpression::Value(Box::new(
                LengthPercentageValue::Length(length),
            ))),
            Box::new(CalcExpression::Value(Box::new(
                LengthPercentageValue::Percentage(StyleNumber::new(50.0)),
            ))),
        );
        let value = StyleValue::LengthPercentage(LengthPercentageValue::Calc(Box::new(calc)));
        assert_eq!(value.clone(), value);
        assert_ne!(value, StyleValue::Text("calc(12.5px + 50%)".into()));
    }

    #[test]
    fn scalar_variants_remain_distinct() {
        assert_ne!(StyleValue::Bool(true), StyleValue::Integer(1));
        assert_ne!(
            StyleValue::Number(StyleNumber::new(1.0)),
            StyleValue::Text("1".into())
        );
        assert_eq!(
            StyleValue::Length(LengthValue::Zero),
            StyleValue::Length(LengthValue::Zero)
        );
    }

    #[test]
    fn font_weight_accepts_only_the_supported_range() {
        assert_eq!(FontWeightValue::new(1).unwrap().get(), 1);
        assert_eq!(FontWeightValue::new(1000).unwrap().get(), 1000);
        assert_eq!(FontWeightValue::new(0), None);
        assert_eq!(FontWeightValue::new(1001), None);
        assert_eq!(FontWeightValue::NORMAL.get(), 400);
        assert_eq!(FontWeightValue::BOLD.get(), 700);
    }

    #[test]
    fn typography_variants_are_semantically_distinct() {
        let tag = OpenTypeTagValue::new(*b"kern").unwrap();
        assert_eq!(tag.get(), *b"kern");
        assert_eq!(OpenTypeTagValue::new([0x1f, b'e', b'r', b'n']), None);
        assert_eq!(OpenTypeTagValue::new([b'k', b'e', b'r', 0x7f]), None);
        assert_ne!(
            StyleValue::FontFamily(FontFamilyValue::System),
            StyleValue::Text("system-ui".into())
        );
        assert_ne!(
            StyleValue::FontStyle(FontStyleValue::Italic),
            StyleValue::FontWeight(FontWeightValue::BOLD)
        );
        assert_ne!(
            StyleValue::Color(ColorValue::Named("red".into())),
            StyleValue::LineHeight(LineHeightValue::Normal)
        );
    }

    #[test]
    fn custom_property_names_require_a_nonempty_whitespace_free_suffix() {
        assert_eq!(
            CustomPropertyName::new("--accent").unwrap().as_str(),
            "--accent"
        );
        assert_eq!(
            CustomPropertyName::new("--Accent").unwrap().as_str(),
            "--Accent"
        );
        assert!(CustomPropertyName::new("--色").is_some());
        assert!(CustomPropertyName::new("--accent-2").is_some());
        assert!(CustomPropertyName::new("--_private").is_some());
        assert!(CustomPropertyName::new("accent").is_none());
        assert!(CustomPropertyName::new("--").is_none());
        assert!(CustomPropertyName::new("--bad name").is_none());
        assert!(CustomPropertyName::new("--1bad").is_some());
        assert!(CustomPropertyName::new("--bad:value").is_none());

        let unresolved = ComponentValue::<ColorValue>::Variable(CustomPropertyReference::new(
            CustomPropertyName::new("--accent").unwrap(),
        ));
        assert!(unresolved.value().is_none());
    }
}
