//! Shared, test-only model for Host conformance scenarios.
//!
//! JSON under `tests/host-conformance` is the language-neutral source of
//! truth. This crate is only the Rust decoder used by Desktop and Web; Kotlin
//! and Swift decode the same versioned schema in their native test targets.

use std::path::PathBuf;

use serde::Deserialize;

/// Current manifest and scenario schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// One Host identifier used by the manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// Shared native Desktop renderer.
    Desktop,
    /// Browser DOM renderer.
    Web,
    /// Android View renderer.
    Android,
    /// iOS UIKit renderer.
    Ios,
}

impl Host {
    /// Stable spelling stored in `required_hosts`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Web => "web",
            Self::Android => "android",
            Self::Ios => "ios",
        }
    }
}

/// Top-level list of shared cases and their required Host coverage.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest format version.
    pub schema: u32,
    /// Pinned WPT commit used by every adapted WPT case.
    pub wpt_revision: String,
    /// Cases in deterministic execution order.
    pub cases: Vec<ManifestCase>,
}

/// One entry in [`Manifest`].
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManifestCase {
    /// Globally unique case identifier.
    pub id: String,
    /// Capability or CSS feature exercised by the case.
    pub feature: String,
    /// Path relative to the Host conformance root.
    pub fixture: PathBuf,
    /// Hosts which must execute this case.
    pub required_hosts: Vec<String>,
    /// Required checkpoint kinds.
    pub checkpoints: Vec<String>,
}

impl ManifestCase {
    /// Whether this case is required for `host`.
    pub fn requires(&self, host: Host) -> bool {
        self.required_hosts
            .iter()
            .any(|value| value == host.as_str())
    }
}

/// One language-neutral Host scenario.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Scenario format version.
    pub schema: u32,
    /// Identifier matching its manifest entry.
    pub id: String,
    /// WPT provenance, present only on WPT adaptations.
    #[serde(default)]
    pub upstream: Option<Upstream>,
    /// Commands sent to the Host under test.
    pub test: ScenarioSide,
    /// Independent reference commands for reftests.
    #[serde(default)]
    pub reference: Option<ScenarioSide>,
}

/// Provenance for a WPT-derived scenario.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    /// Source repository.
    pub repository: String,
    /// Exact source commit.
    pub revision: String,
    /// Source test path.
    pub path: String,
    /// Source reference path, when WPT has one.
    pub reference_path: Option<String>,
    /// Upstream license identifier.
    pub license: String,
    /// Semantic assertion retained by the adaptation.
    pub assertion: String,
    /// Explicit record of adaptation and omitted document behavior.
    pub adaptation: String,
}

/// Ordered commands for one side of a scenario.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSide {
    /// Commands executed in order.
    pub commands: Vec<Command>,
}

/// Host-boundary command vocabulary shared by every runner.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Creates a Host surface with logical viewport metrics.
    AttachSurface {
        /// Logical width.
        width: f32,
        /// Logical height.
        height: f32,
        /// Physical pixels per logical pixel.
        scale: f32,
    },
    /// Registers one already-decoded raster in the mock Host resource store.
    /// Production providers reach the same store after URL, asset, or byte
    /// acquisition succeeds.
    RegisterRasterResource {
        /// Stable non-zero resource identifier referenced by paint commands.
        id: u64,
        /// Raster width in physical pixels.
        width: u32,
        /// Raster height in physical pixels.
        height: u32,
        /// Row-major pixels from top-left to bottom-right.
        pixels: Vec<ColorFixture>,
    },
    /// Asks the production Host resource service to acquire and decode a
    /// raster outside the frame transaction.
    LoadRasterResource {
        /// Stable non-zero resource identifier.
        id: u64,
        /// Monotonic non-zero generation for replacement safety.
        generation: u64,
        /// Encoded source consumed once by the Host resource service.
        source: ResourceSourceFixture,
    },
    /// Releases one exact resource generation after accepted frames no longer
    /// reference it.
    ReleaseRasterResource {
        /// Stable non-zero resource identifier.
        id: u64,
        /// Exact non-zero generation to release.
        generation: u64,
    },
    /// Checks the latest event and retained state for one resource generation.
    CheckpointResource {
        /// Stable non-zero resource identifier.
        id: u64,
        /// Exact non-zero generation being observed.
        generation: u64,
        /// Expected resource lifecycle state.
        state: ResourceStateFixture,
        /// Expected intrinsic raster width for a ready resource.
        #[serde(default)]
        width: Option<u32>,
        /// Expected intrinsic raster height for a ready resource.
        #[serde(default)]
        height: Option<u32>,
    },
    /// Presents one semantic box through the production Host path.
    PresentBox {
        /// Frame target revision.
        revision: u64,
        /// X, y, width, height in logical pixels.
        rect: [f32; 4],
        /// Background color.
        background: ColorFixture,
        /// Optional border semantics.
        #[serde(default)]
        border: Option<BorderFixture>,
    },
    /// Presents a retained tree of semantic boxes through the production Host path.
    PresentScene {
        /// Frame target revision.
        revision: u64,
        /// Nodes ordered independently of their parent relationships.
        nodes: Vec<SceneNodeFixture>,
    },
    /// Captures one named presentation checkpoint.
    Checkpoint {
        /// Checkpoint contract name.
        name: String,
        /// Optional logical-pixel samples for visual tests without a WPT
        /// reference document.
        #[serde(default)]
        samples: Vec<PixelSampleFixture>,
        /// Optional relative luminance assertions for CSS rendering whose
        /// exact colors are intentionally Host-defined.
        #[serde(default)]
        relations: Vec<PixelRelationFixture>,
    },
    /// Sends a text measurement request to the production Host measurer.
    MeasureText {
        /// Measurement correlation key.
        key: u64,
        /// Text content.
        text: String,
        /// Ordered font-family fallback list. `system` selects the Host UI font.
        #[serde(default = "default_font_families")]
        font_families: Vec<String>,
        /// Font size in logical pixels.
        font_size: f32,
        /// CSS numeric font weight.
        #[serde(default = "default_font_weight")]
        font_weight: u16,
        /// Resolved font posture.
        #[serde(default)]
        font_style: FontStyleFixture,
        /// Line height in logical pixels.
        line_height: f32,
        /// Additional logical pixels between glyph advances.
        #[serde(default)]
        letter_spacing: f32,
        /// Ordered OpenType feature settings.
        #[serde(default)]
        font_features: Vec<FontFeatureFixture>,
        /// Ordered variable-font axis settings.
        #[serde(default)]
        font_variations: Vec<FontVariationFixture>,
        /// Lynx optical-sizing behavior.
        #[serde(default)]
        font_optical_sizing: FontOpticalSizingFixture,
        /// Lynx `white-space` value projected to the Host wrapping policy.
        #[serde(default)]
        white_space: WhiteSpaceFixture,
        /// Lynx `word-break` value used when wrapping is enabled.
        #[serde(default)]
        word_break: WordBreakFixture,
        /// Maximum visible line count; zero means unlimited.
        #[serde(default)]
        max_lines: u32,
        /// Lynx `text-overflow` value applied at the line limit.
        #[serde(default)]
        overflow: TextOverflowFixture,
        /// Base shaping direction.
        #[serde(default)]
        direction: TextDirectionFixture,
        /// Inline-axis alignment inside the measured text box.
        #[serde(default)]
        alignment: TextAlignmentFixture,
        /// First-line indentation as a resolved length-plus-percentage pair.
        #[serde(default)]
        indent: TextIndentFixture,
        /// Definite available width.
        available_width: f32,
        /// Available-space mode forwarded to the Host measurer.
        #[serde(default)]
        available_width_kind: AvailableWidthFixture,
    },
    /// Checks one previously produced text measurement.
    CheckpointMeasurement {
        /// Measurement correlation key.
        key: u64,
        /// Inclusive minimum width.
        min_width: f32,
        /// Inclusive maximum width.
        max_width: f32,
        /// Inclusive minimum height.
        min_height: f32,
        /// Inclusive maximum height.
        max_height: f32,
        /// Whether reusable prepared content is required, when the Host
        /// exposes that optimization through its measurement API.
        #[serde(default)]
        prepared_content: Option<bool>,
    },
    /// Emits one normalized pointer event into the mock runtime sink.
    EmitPointer {
        /// Pointer event phase.
        event: PointerEventFixture,
        /// Physical pointer source after Host normalization.
        pointer_kind: PointerKindFixture,
        /// Stable pointer identifier.
        pointer_id: u64,
        /// Host monotonic timestamp.
        timestamp_ms: f64,
        /// Logical x coordinate.
        x: f32,
        /// Logical y coordinate.
        y: f32,
        /// Active button bitset.
        buttons: u32,
        /// Changed button, or the Host sentinel.
        changed_button: i16,
    },
    /// Checks the last normalized pointer event.
    CheckpointInput {
        /// Expected pointer event phase.
        event: PointerEventFixture,
        /// Expected physical pointer source.
        pointer_kind: PointerKindFixture,
        /// Expected pointer identifier.
        pointer_id: u64,
        /// Expected logical x coordinate.
        x: f32,
        /// Expected logical y coordinate.
        y: f32,
    },
}

/// Encoded source variants accepted by the mock Host resource boundary.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceSourceFixture {
    /// URL acquired under Host network and security policy. Data URLs keep
    /// conformance deterministic while exercising the URL source path.
    Url {
        /// Absolute source URL.
        value: String,
    },
    /// One-time encoded bytes represented as base64 in the language-neutral fixture.
    Bytes {
        /// Non-empty MIME media type.
        media_type: String,
        /// Standard padded base64 encoded contents.
        base64: String,
    },
}

/// Observable resource lifecycle states used by conformance checkpoints.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStateFixture {
    /// Decode completed and the image is retained for painting.
    Ready,
    /// The exact generation failed acquisition or decode.
    Failed,
    /// The exact generation was released and is no longer paintable.
    Released,
}

/// Pointer phase used by input fixtures.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PointerEventFixture {
    /// Pointer pressed.
    Down,
    /// Pointer moved.
    Move,
    /// Pointer released.
    Up,
    /// Pointer sequence cancelled.
    Cancel,
}

/// Physical pointer source used by input fixtures.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PointerKindFixture {
    /// Mouse or trackpad cursor.
    Mouse,
    /// Direct touch contact.
    Touch,
    /// Stylus or pen contact.
    Pen,
    /// Host source not otherwise represented.
    Unknown,
}

/// Color syntax accepted by Host scenarios.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColorFixture {
    /// CSS named color.
    Named {
        /// Color name.
        value: String,
    },
    /// Explicit sRGB color.
    Srgba {
        /// Red channel.
        red: u8,
        /// Green channel.
        green: u8,
        /// Blue channel.
        blue: u8,
        /// Alpha channel in `[0, 1]`.
        alpha: f32,
    },
}

/// One logical-pixel color assertion captured at a paint checkpoint.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PixelSampleFixture {
    /// Logical x/y coordinate within the attached surface.
    pub point: [f32; 2],
    /// Expected unpremultiplied sRGB color.
    pub color: ColorFixture,
    /// Maximum per-channel difference accepted by native rasterizers.
    #[serde(default)]
    pub tolerance: u8,
}

/// One relative luminance assertion between two logical pixels.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PixelRelationFixture {
    /// First logical x/y coordinate.
    pub first: [f32; 2],
    /// Second logical x/y coordinate.
    pub second: [f32; 2],
    /// Required ordering of the first sample relative to the second.
    pub relation: PixelRelationKind,
    /// Minimum luminance distance on an 8-bit scale.
    #[serde(default)]
    pub minimum_difference: u8,
}

/// Relative luminance ordering used by platform-defined CSS shading.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PixelRelationKind {
    /// The first pixel must be lighter than the second.
    Lighter,
    /// The first pixel must be darker than the second.
    Darker,
}

/// Physical border semantics in top, right, bottom, left order.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BorderFixture {
    /// Widths in logical pixels.
    pub widths: [f32; 4],
    /// Colors.
    pub colors: [ColorFixture; 4],
    /// Line styles.
    pub styles: [BorderStyleFixture; 4],
    /// Corner radii in top-left, top-right, bottom-right, bottom-left order.
    pub radii: [CornerRadiusFixture; 4],
}

/// One resolved CSS box shadow.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BoxShadowFixture {
    /// Horizontal and vertical logical-pixel offset.
    pub offset: [f32; 2],
    /// Non-negative logical-pixel blur radius.
    pub blur_radius: f32,
    /// Signed logical-pixel spread radius.
    pub spread_radius: f32,
    /// Shadow color.
    pub color: ColorFixture,
    /// Whether the shadow is painted inside the border box.
    #[serde(default)]
    pub inset: bool,
}

/// One resolved `clip-path` and its geometry reference box.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClipPathFixture {
    /// Box used to resolve shape coordinates.
    #[serde(default)]
    pub reference_box: ClipReferenceBoxFixture,
    /// Resolved basic shape.
    pub shape: ClipShapeFixture,
}

/// Basic shape vocabulary currently exercised by shared Host fixtures.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClipShapeFixture {
    /// Inset rectangle with independent edges and corner radii.
    Inset {
        /// Top, right, bottom, and left length-percentage insets.
        edges: [LengthPercentageFixture; 4],
        /// Top-left, top-right, bottom-right, and bottom-left radii.
        radii: [CornerRadiusFixture; 4],
    },
    /// Circle with an explicit radius and center.
    Circle {
        /// Radius length-percentage.
        radius: LengthPercentageFixture,
        /// Center x/y coordinates.
        center: [LengthPercentageFixture; 2],
    },
    /// Ellipse with explicit horizontal/vertical radii and center.
    Ellipse {
        /// Horizontal and vertical radii.
        radii: [LengthPercentageFixture; 2],
        /// Center x/y coordinates.
        center: [LengthPercentageFixture; 2],
    },
    /// Arbitrary path with an explicit winding rule.
    Path {
        /// Rule used to determine the filled region.
        #[serde(default)]
        fill_rule: FillRuleFixture,
        /// Normalized absolute path command stream.
        commands: Vec<PathCommandFixture>,
    },
}

/// Fill rule used by path fixtures.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FillRuleFixture {
    /// Non-zero winding rule.
    #[default]
    NonZero,
    /// Even-odd rule.
    EvenOdd,
}

/// One absolute command in a path fixture.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathCommandFixture {
    /// Move the current point.
    MoveTo {
        /// Destination point.
        point: [LengthPercentageFixture; 2],
    },
    /// Add a straight line.
    LineTo {
        /// Destination point.
        point: [LengthPercentageFixture; 2],
    },
    /// Add a quadratic Bezier segment.
    QuadraticTo {
        /// Control point.
        control: [LengthPercentageFixture; 2],
        /// End point.
        end: [LengthPercentageFixture; 2],
    },
    /// Add a cubic Bezier segment.
    CubicTo {
        /// First control point.
        control_1: [LengthPercentageFixture; 2],
        /// Second control point.
        control_2: [LengthPercentageFixture; 2],
        /// End point.
        end: [LengthPercentageFixture; 2],
    },
    /// Close the current subpath.
    Close,
}

/// CSS geometry boxes accepted as clip-path reference boxes.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipReferenceBoxFixture {
    /// Border box, and the Lynx-compatible default.
    #[default]
    Border,
    /// Padding box.
    Padding,
    /// Content box.
    Content,
}

/// Raster-image scaling algorithm requested by a scene fixture.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageRenderingFixture {
    /// Use the Host's normal interpolation policy.
    #[default]
    Auto,
    /// Use nearest-neighbor sampling while scaling raster images.
    Pixelated,
    /// Lynx-compatible alias currently rendered with the `auto` policy.
    CrispEdges,
}

/// Keyword cursor requested by a retained scene fixture.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CursorFixture {
    /// Let the Host select its default cursor.
    #[default]
    Auto,
    /// Link or interactive pointing affordance.
    Pointer,
    /// Text-selection affordance.
    Text,
    /// Grab affordance.
    Grab,
    /// Hide the cursor.
    None,
}

/// Lynx pointer hit-test participation requested by a scene fixture.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PointerEventsFixture {
    /// Use normal hit testing.
    #[default]
    Auto,
    /// Disable the node and its descendants.
    None,
}

/// One node in a retained Host scene fixture.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneNodeFixture {
    /// Stable non-zero node identifier.
    pub id: u64,
    /// Optional parent node identifier.
    pub parent: Option<u64>,
    /// Parent-relative x, y, width, and height in logical pixels.
    pub rect: [f32; 4],
    /// Content box relative to this node's border-box origin. When omitted,
    /// the content box is the complete border box.
    #[serde(default)]
    pub content_box: Option<[f32; 4]>,
    /// Box background color.
    pub background: ColorFixture,
    /// Optional native text content and its resolved paint values.
    #[serde(default)]
    pub text: Option<TextFixture>,
    /// Optional border semantics.
    #[serde(default)]
    pub border: Option<BorderFixture>,
    /// Box shadows in CSS front-to-back order.
    #[serde(default)]
    pub box_shadows: Vec<BoxShadowFixture>,
    /// Resolved `backdrop-filter: blur()` radius in logical pixels.
    #[serde(default)]
    pub backdrop_blur: Option<f32>,
    /// Raster-image scaling algorithm for this element's own image paint.
    #[serde(default)]
    pub image_rendering: ImageRenderingFixture,
    /// Resolved keyword cursor.
    #[serde(default)]
    pub cursor: CursorFixture,
    /// Resolved pointer hit-test behavior.
    #[serde(default)]
    pub pointer_events: PointerEventsFixture,
    /// Optional basic-shape clip applied to the node and its descendants.
    #[serde(default)]
    pub clip_path: Option<ClipPathFixture>,
    /// Descendant overflow clipping semantics.
    #[serde(default)]
    pub clip: BoxClipFixture,
    /// Optional column-major 4-by-4 transform matrix.
    #[serde(default)]
    pub transform: Option<[f32; 16]>,
    /// Optional resolved group opacity in `[0, 1]`.
    #[serde(default)]
    pub opacity: Option<f32>,
    /// Optional resolved paint visibility.
    #[serde(default)]
    pub visibility: Option<VisibilityFixture>,
    /// Optional resolved sibling stacking order.
    #[serde(default)]
    pub z_order: Option<i32>,
    /// Resolved geometry shared by the optional background image below.
    #[serde(default)]
    pub background_layer: BackgroundLayerFixture,
    /// Ordered CSS background layers, with the first entry painted nearest
    /// the user. This is mutually exclusive with the legacy single-image
    /// fields below.
    #[serde(default)]
    pub background_layers: Vec<BackgroundPaintLayerFixture>,
    /// Optional resolved linear-gradient background image. The fixture DSL
    /// supplies the remaining `BackgroundLayer` fields as their CSS initial
    /// values so every Host receives the same protocol operation.
    #[serde(default)]
    pub linear_gradient: Option<LinearGradientFixture>,
    /// Optional explicit, non-repeating radial-gradient background image.
    #[serde(default)]
    pub radial_gradient: Option<RadialGradientFixture>,
    /// Optional explicit, non-repeating conic-gradient background image.
    #[serde(default)]
    pub conic_gradient: Option<ConicGradientFixture>,
}

/// Native text content used by a retained scene fixture.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextFixture {
    /// UTF-8 text value.
    pub value: String,
    /// Ordered font-family fallback list. The reserved `system` spelling maps
    /// to each Host's platform UI font; every other value is a named family.
    #[serde(default = "default_font_families")]
    pub font_families: Vec<String>,
    /// Logical-pixel font size.
    pub font_size: f32,
    /// CSS numeric font weight.
    #[serde(default = "default_font_weight")]
    pub font_weight: u16,
    /// Resolved font posture.
    #[serde(default)]
    pub font_style: FontStyleFixture,
    /// Explicit logical-pixel line height. Omission means `normal`.
    #[serde(default)]
    pub line_height: Option<f32>,
    /// Additional logical pixels between glyph advances.
    #[serde(default)]
    pub letter_spacing: f32,
    /// Ordered OpenType feature settings.
    #[serde(default)]
    pub font_features: Vec<FontFeatureFixture>,
    /// Ordered variable-font axis settings.
    #[serde(default)]
    pub font_variations: Vec<FontVariationFixture>,
    /// Lynx optical-sizing behavior.
    #[serde(default)]
    pub font_optical_sizing: FontOpticalSizingFixture,
    /// Foreground glyph color.
    pub color: ColorFixture,
    /// Base shaping direction.
    #[serde(default)]
    pub direction: TextDirectionFixture,
    /// Inline-axis alignment within the text element's layout box.
    #[serde(default)]
    pub alignment: TextAlignmentFixture,
    /// First-line indentation as a resolved length-plus-percentage pair.
    #[serde(default)]
    pub indent: TextIndentFixture,
    /// Lynx `white-space` value.
    #[serde(default)]
    pub white_space: WhiteSpaceFixture,
    /// Lynx `word-break` value.
    #[serde(default)]
    pub word_break: WordBreakFixture,
    /// Maximum visible line count; zero means unlimited.
    #[serde(default)]
    pub max_lines: u32,
    /// Lynx `text-overflow` value.
    #[serde(default)]
    pub overflow: TextOverflowFixture,
    /// Optional single Lynx text decoration.
    #[serde(default)]
    pub decoration: Option<TextDecorationFixture>,
    /// Optional single Lynx-compatible shadow.
    #[serde(default)]
    pub shadow: Option<TextShadowFixture>,
}

/// Font posture used by a retained text fixture.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FontStyleFixture {
    /// Upright glyphs.
    #[default]
    Normal,
    /// Use the font's italic face when available.
    Italic,
    /// Use an oblique face or synthetic slant.
    Oblique,
}

/// One OpenType feature selector in a shared fixture.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FontFeatureFixture {
    /// Exactly four printable ASCII characters.
    pub tag: String,
    /// Non-negative OpenType feature value.
    pub value: u32,
}

/// One variable-font axis selector in a shared fixture.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FontVariationFixture {
    /// Exactly four printable ASCII characters.
    pub tag: String,
    /// Finite axis value.
    pub value: f32,
}

/// Lynx-supported `font-optical-sizing` values.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FontOpticalSizingFixture {
    /// Derive `opsz` from the computed font size.
    Auto,
    /// Do not synthesize optical sizing. Lynx initial value.
    #[default]
    None,
}

/// Resolved `text-indent` components; percentage is relative to text width.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextIndentFixture {
    /// Fixed logical-pixel component.
    pub logical_pixels: f32,
    /// Percentage number before the `%` suffix.
    pub percentage: f32,
}

/// Lynx-supported `white-space` values.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WhiteSpaceFixture {
    /// Collapse whitespace and allow wrapping.
    #[default]
    Normal,
    /// Collapse whitespace and suppress wrapping.
    NoWrap,
}

/// Width availability supplied to intrinsic text measurement.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AvailableWidthFixture {
    /// A finite constraint using `available_width`.
    #[default]
    Definite,
    /// The smallest width allowed by text wrapping opportunities.
    MinContent,
    /// The unwrapped intrinsic width.
    MaxContent,
}

/// Lynx-supported `word-break` values.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WordBreakFixture {
    /// Use ordinary Unicode break opportunities.
    #[default]
    Normal,
    /// Permit a break between any characters.
    BreakAll,
    /// Suppress ordinary breaks inside CJK text.
    KeepAll,
}

/// Lynx-supported `text-overflow` values.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextOverflowFixture {
    /// Clip overflowing glyphs.
    #[default]
    Clip,
    /// Shape an ellipsis on the final visible line.
    Ellipsis,
}

/// Base direction used by Host text shaping.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextDirectionFixture {
    /// Resolve direction from Unicode content and the Host locale.
    #[default]
    Auto,
    /// Force left-to-right shaping.
    LeftToRight,
    /// Force right-to-left shaping.
    RightToLeft,
}

/// Lynx-supported `text-align` values.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignmentFixture {
    /// Align to the logical inline start.
    #[default]
    Start,
    /// Align to the logical inline end.
    End,
    /// Align to the physical left edge.
    Left,
    /// Align to the physical right edge.
    Right,
    /// Center each line.
    Center,
}

/// One resolved Lynx text decoration.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextDecorationFixture {
    /// Single supported line kind.
    pub line: TextDecorationLineFixture,
    /// Stroke style.
    pub style: TextDecorationStyleFixture,
    /// Decoration color.
    pub color: ColorFixture,
}

/// Lynx's single decoration line.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextDecorationLineFixture {
    /// A line below the glyphs.
    Underline,
    /// A line through the glyphs.
    LineThrough,
}

/// Lynx decoration stroke styles.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextDecorationStyleFixture {
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

/// One resolved native text shadow.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextShadowFixture {
    /// Horizontal and vertical logical-pixel offset.
    pub offset: [f32; 2],
    /// Non-negative logical-pixel blur radius.
    pub blur_radius: f32,
    /// Shadow color.
    pub color: ColorFixture,
}

const fn default_font_weight() -> u16 {
    400
}

fn default_font_families() -> Vec<String> {
    vec!["system".to_owned()]
}

impl SceneNodeFixture {
    /// Returns the explicit content box or the no-border/no-padding default.
    pub fn resolved_content_box(&self) -> [f32; 4] {
        self.content_box
            .unwrap_or([0.0, 0.0, self.rect[2], self.rect[3]])
    }
}

/// One resolved linear-gradient image used by a retained scene node.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LinearGradientFixture {
    /// Direction in clockwise degrees from the positive vertical axis.
    pub angle_degrees: f32,
    /// Whether the gradient repeats beyond its final stop.
    #[serde(default)]
    pub repeating: bool,
    /// Ordered, explicitly resolved color stops.
    pub stops: Vec<GradientStopFixture>,
}

/// One resolved stop on a fixture gradient line.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GradientStopFixture {
    /// Stop color.
    pub color: ColorFixture,
    /// Position as a fraction of the gradient line.
    pub position: f32,
}

/// One resolved explicit elliptical radial-gradient image.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RadialGradientFixture {
    /// Center x/y in logical pixels relative to the positioning box.
    pub center: [f32; 2],
    /// Horizontal and vertical radii in logical pixels.
    pub radii: [f32; 2],
    /// Ordered, explicitly resolved color stops.
    pub stops: Vec<GradientStopFixture>,
}

/// One resolved non-repeating conic-gradient image.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConicGradientFixture {
    /// Starting angle in clockwise degrees from the positive vertical axis.
    pub from_degrees: f32,
    /// Center x/y in logical pixels relative to the positioning box.
    pub center: [f32; 2],
    /// Ordered, explicitly resolved color stops expressed as turns.
    pub stops: Vec<GradientStopFixture>,
}

/// One image and its independently resolved CSS background geometry.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BackgroundPaintLayerFixture {
    /// Position, size, repetition, origin, and clip for this layer.
    #[serde(default)]
    pub geometry: BackgroundLayerFixture,
    /// Resolved image painted by this layer.
    pub image: BackgroundImageFixture,
}

/// Image vocabulary used by shared background-layer fixtures.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundImageFixture {
    /// A raster already registered with the mock Host resource store.
    Resource(u64),
    /// Resolved linear gradient.
    LinearGradient(LinearGradientFixture),
    /// Resolved radial gradient.
    RadialGradient(RadialGradientFixture),
    /// Resolved conic gradient.
    ConicGradient(ConicGradientFixture),
}

/// Resolved geometry for the one background image supported by the fixture DSL.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BackgroundLayerFixture {
    /// X/y position as a length plus a fraction of the positioning area.
    #[serde(default)]
    pub position: [LengthPercentageFixture; 2],
    /// Intrinsic, cover/contain, or explicit per-axis sizing.
    #[serde(default)]
    pub size: BackgroundSizeFixture,
    /// Horizontal image repetition.
    #[serde(default)]
    pub repeat_x: ImageRepeatFixture,
    /// Vertical image repetition.
    #[serde(default)]
    pub repeat_y: ImageRepeatFixture,
    /// Background positioning box.
    #[serde(default = "default_background_origin")]
    pub origin: BackgroundBoxFixture,
    /// Background painting clip box.
    #[serde(default = "default_background_clip")]
    pub clip: BackgroundBoxFixture,
}

impl Default for BackgroundLayerFixture {
    fn default() -> Self {
        Self {
            position: Default::default(),
            size: BackgroundSizeFixture::default(),
            repeat_x: ImageRepeatFixture::Repeat,
            repeat_y: ImageRepeatFixture::Repeat,
            origin: default_background_origin(),
            clip: default_background_clip(),
        }
    }
}

/// Background image sizing accepted by the shared fixture DSL.
///
/// The pair variant preserves the original fixture spelling. The axes variant
/// represents one-axis `auto` with `null` on that axis.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum BackgroundSizeFixture {
    /// Legacy explicit width and height pair.
    ExplicitPair([LengthPercentageFixture; 2]),
    /// Explicit axes where either width or height may retain intrinsic sizing.
    ExplicitAxes {
        /// Explicit width or `auto` when absent/null.
        width: Option<LengthPercentageFixture>,
        /// Explicit height or `auto` when absent/null.
        height: Option<LengthPercentageFixture>,
    },
    /// One CSS sizing keyword.
    Keyword(BackgroundSizeKeywordFixture),
}

impl Default for BackgroundSizeFixture {
    fn default() -> Self {
        Self::Keyword(BackgroundSizeKeywordFixture::Auto)
    }
}

/// Keyword forms of `background-size`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundSizeKeywordFixture {
    /// Preserve intrinsic dimensions.
    Auto,
    /// Cover the positioning area while preserving aspect ratio.
    Cover,
    /// Fit within the positioning area while preserving aspect ratio.
    Contain,
}

/// One resolved CSS length-percentage pair.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LengthPercentageFixture {
    /// Absolute logical-pixel component.
    pub length: f32,
    /// Fractional component where `1` is 100 percent.
    #[serde(default)]
    pub fraction: f32,
}

/// Background tiling rule on one axis.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageRepeatFixture {
    /// Tile and crop the final tile.
    #[default]
    Repeat,
    /// Paint one image only.
    NoRepeat,
    /// Distribute whole tiles with spacing.
    Space,
    /// Resize tiles so a whole number fits.
    Round,
}

/// CSS boxes currently meaningful for background geometry fixtures.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundBoxFixture {
    /// Border box.
    Border,
    /// Padding box.
    Padding,
    /// Content box.
    Content,
    /// Area painted by the element's border.
    BorderArea,
}

const fn default_background_origin() -> BackgroundBoxFixture {
    BackgroundBoxFixture::Padding
}

const fn default_background_clip() -> BackgroundBoxFixture {
    BackgroundBoxFixture::Border
}

/// Paint visibility delivered by `SetVisibility`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisibilityFixture {
    /// Paint the node and its descendants.
    Visible,
    /// Suppress the node subtree while preserving layout.
    Hidden,
}

/// Independent horizontal and vertical descendant clipping.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BoxClipFixture {
    /// Horizontal overflow behavior.
    #[serde(default)]
    pub horizontal: OverflowClipFixture,
    /// Vertical overflow behavior.
    #[serde(default)]
    pub vertical: OverflowClipFixture,
}

/// One Host-protocol overflow axis.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverflowClipFixture {
    /// Descendants remain visible outside this axis.
    #[default]
    Visible,
    /// Descendants are clipped to this axis.
    Hidden,
}

/// One circular or elliptical CSS border radius.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum CornerRadiusFixture {
    /// A single value applies to both axes.
    Circular(f32),
    /// Independent horizontal and vertical values.
    Elliptical([f32; 2]),
}

impl CornerRadiusFixture {
    /// Horizontal radius in logical pixels.
    pub fn horizontal(self) -> f32 {
        match self {
            Self::Circular(value) | Self::Elliptical([value, _]) => value,
        }
    }

    /// Vertical radius in logical pixels.
    pub fn vertical(self) -> f32 {
        match self {
            Self::Circular(value) | Self::Elliptical([_, value]) => value,
        }
    }

    fn is_valid(self) -> bool {
        [self.horizontal(), self.vertical()]
            .into_iter()
            .all(|value| value.is_finite() && value >= 0.0)
    }
}

/// Complete CSS border line-style vocabulary.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyleFixture {
    /// No border.
    None,
    /// Hidden conflict-resolution border.
    Hidden,
    /// Solid line.
    Solid,
    /// Dashed line.
    Dashed,
    /// Dotted line.
    Dotted,
    /// Double line.
    Double,
    /// Grooved line.
    Groove,
    /// Ridged line.
    Ridge,
    /// Inset line.
    Inset,
    /// Outset line.
    Outset,
}

mod validation;

pub use validation::{FixtureError, LoadedCase, load_all, load_required};

#[cfg(test)]
#[path = "lib/tests.rs"]
mod tests;
