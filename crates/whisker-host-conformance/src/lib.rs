//! Shared, test-only model for Host conformance scenarios.
//!
//! JSON under `tests/host-conformance` is the language-neutral source of
//! truth. This crate is only the Rust decoder used by Desktop and Web; Kotlin
//! and Swift decode the same versioned schema in their native test targets.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

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
        /// Font size in logical pixels.
        font_size: f32,
        /// Line height in logical pixels.
        line_height: f32,
        /// Definite available width.
        available_width: f32,
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
        /// Whether reusable prepared content is required.
        prepared_content: bool,
    },
    /// Emits one normalized pointer event into the mock runtime sink.
    EmitPointer {
        /// Pointer event phase.
        event: PointerEventFixture,
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
    /// Logical-pixel font size.
    pub font_size: f32,
    /// CSS numeric font weight.
    #[serde(default = "default_font_weight")]
    pub font_weight: u16,
    /// Foreground glyph color.
    pub color: ColorFixture,
    /// Optional single Lynx-compatible shadow.
    #[serde(default)]
    pub shadow: Option<TextShadowFixture>,
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

/// A loaded and cross-validated manifest case.
#[derive(Clone, Debug)]
pub struct LoadedCase {
    /// Manifest metadata.
    pub manifest: ManifestCase,
    /// Decoded scenario.
    pub scenario: Scenario,
}

/// Fixture loading or consistency failure.
#[derive(Debug)]
pub struct FixtureError(String);

impl fmt::Display for FixtureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FixtureError {}

/// Loads every declared case, enforcing manifest/scenario identity and WPT
/// provenance before a platform runner receives any commands.
pub fn load_all(root: &Path) -> Result<(Manifest, Vec<LoadedCase>), FixtureError> {
    let manifest_path = root.join("manifest.json");
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| FixtureError(format!("read {}: {error}", manifest_path.display())))?;
    let manifest: Manifest = serde_json::from_str(&manifest_text)
        .map_err(|error| FixtureError(format!("decode {}: {error}", manifest_path.display())))?;
    validate_manifest(&manifest)?;

    let mut loaded = Vec::new();
    for entry in &manifest.cases {
        let path = root.join(&entry.fixture);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| FixtureError(format!("read {}: {error}", path.display())))?;
        let scenario: Scenario = serde_json::from_str(&text)
            .map_err(|error| FixtureError(format!("decode {}: {error}", path.display())))?;
        validate_scenario(&manifest, entry, &scenario)?;
        loaded.push(LoadedCase {
            manifest: entry.clone(),
            scenario,
        });
    }
    Ok((manifest, loaded))
}

/// Loads all cases required by `host` after validating the complete suite.
///
/// Validating first ensures a malformed fixture cannot remain hidden merely
/// because its first Host runner has not been made required yet.
pub fn load_required(root: &Path, host: Host) -> Result<(Manifest, Vec<LoadedCase>), FixtureError> {
    let (manifest, mut loaded) = load_all(root)?;
    loaded.retain(|case| case.manifest.requires(host));
    Ok((manifest, loaded))
}

fn validate_manifest(manifest: &Manifest) -> Result<(), FixtureError> {
    if manifest.schema != SCHEMA_VERSION {
        return Err(FixtureError(format!(
            "manifest schema {} is not supported",
            manifest.schema
        )));
    }
    if manifest.wpt_revision.trim().is_empty() {
        return Err(FixtureError("manifest WPT revision is empty".into()));
    }
    let mut ids = BTreeSet::new();
    let mut fixtures = BTreeSet::new();
    for case in &manifest.cases {
        if !ids.insert(&case.id) {
            return Err(FixtureError(format!("duplicate case id {}", case.id)));
        }
        if !fixtures.insert(&case.fixture) {
            return Err(FixtureError(format!(
                "duplicate fixture {}",
                case.fixture.display()
            )));
        }
        if case.fixture.is_absolute()
            || case
                .fixture
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(FixtureError(format!(
                "fixture path escapes root: {}",
                case.fixture.display()
            )));
        }
        if case.feature.trim().is_empty()
            || case.required_hosts.is_empty()
            || case.checkpoints.is_empty()
        {
            return Err(FixtureError(format!(
                "case {} has incomplete metadata",
                case.id
            )));
        }
        let mut hosts = BTreeSet::new();
        for host in &case.required_hosts {
            if !matches!(host.as_str(), "desktop" | "web" | "android" | "ios") {
                return Err(FixtureError(format!(
                    "case {} names unknown Host {host}",
                    case.id
                )));
            }
            if !hosts.insert(host) {
                return Err(FixtureError(format!(
                    "case {} repeats required Host {host}",
                    case.id
                )));
            }
        }
        let mut checkpoints = BTreeSet::new();
        for checkpoint in &case.checkpoints {
            if !matches!(
                checkpoint.as_str(),
                "rust-layout-protocol"
                    | "semantic-projection"
                    | "pixel"
                    | "pixel-samples"
                    | "pixel-relations"
                    | "resource-registration"
                    | "resource-lifecycle"
                    | "measurement"
                    | "input"
            ) {
                return Err(FixtureError(format!(
                    "case {} names unknown checkpoint {checkpoint}",
                    case.id
                )));
            }
            if !checkpoints.insert(checkpoint) {
                return Err(FixtureError(format!(
                    "case {} repeats checkpoint {checkpoint}",
                    case.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_scenario(
    manifest: &Manifest,
    entry: &ManifestCase,
    scenario: &Scenario,
) -> Result<(), FixtureError> {
    if scenario.schema != SCHEMA_VERSION {
        return Err(FixtureError(format!(
            "scenario {} schema {} is not supported",
            scenario.id, scenario.schema
        )));
    }
    if scenario.id != entry.id {
        return Err(FixtureError(format!(
            "fixture {} declares id {}, expected {}",
            entry.fixture.display(),
            scenario.id,
            entry.id
        )));
    }
    if scenario.test.commands.is_empty() {
        return Err(FixtureError(format!(
            "scenario {} has no test commands",
            scenario.id
        )));
    }
    validate_side(&scenario.id, "test", &scenario.test)?;
    if entry
        .checkpoints
        .iter()
        .any(|checkpoint| checkpoint == "pixel-samples")
        && !scenario.test.commands.iter().any(|command| {
            matches!(
                command,
                Command::Checkpoint { samples, .. } if !samples.is_empty()
            )
        })
    {
        return Err(FixtureError(format!(
            "scenario {} declares pixel-samples without sample assertions",
            scenario.id
        )));
    }
    if entry
        .checkpoints
        .iter()
        .any(|checkpoint| checkpoint == "resource-lifecycle")
        && !scenario
            .test
            .commands
            .iter()
            .any(|command| matches!(command, Command::CheckpointResource { .. }))
    {
        return Err(FixtureError(format!(
            "scenario {} declares resource-lifecycle without a resource checkpoint",
            scenario.id
        )));
    }
    if entry
        .checkpoints
        .iter()
        .any(|checkpoint| checkpoint == "pixel-relations")
        && !scenario.test.commands.iter().any(|command| {
            matches!(
                command,
                Command::Checkpoint { relations, .. } if !relations.is_empty()
            )
        })
    {
        return Err(FixtureError(format!(
            "scenario {} declares pixel-relations without relation assertions",
            scenario.id
        )));
    }
    if let Some(reference) = &scenario.reference {
        validate_side(&scenario.id, "reference", reference)?;
    }
    if scenario.id.starts_with("wpt.") {
        let upstream = scenario.upstream.as_ref().ok_or_else(|| {
            FixtureError(format!("WPT scenario {} has no provenance", scenario.id))
        })?;
        if upstream.repository != "https://github.com/web-platform-tests/wpt"
            || upstream.revision != manifest.wpt_revision
            || upstream.license != "BSD-3-Clause"
            || upstream.path.trim().is_empty()
            || upstream.assertion.trim().is_empty()
            || upstream.adaptation.trim().is_empty()
        {
            return Err(FixtureError(format!(
                "WPT scenario {} has invalid provenance",
                scenario.id
            )));
        }
    } else if scenario.upstream.is_some() {
        return Err(FixtureError(format!(
            "core scenario {} unexpectedly has WPT provenance",
            scenario.id
        )));
    }
    Ok(())
}

fn validate_side(id: &str, label: &str, side: &ScenarioSide) -> Result<(), FixtureError> {
    if side.commands.is_empty() {
        return Err(FixtureError(format!(
            "scenario {id} has no {label} commands"
        )));
    }
    for command in &side.commands {
        match command {
            Command::AttachSurface {
                width,
                height,
                scale,
            } if finite_positive(*width) && finite_positive(*height) && finite_positive(*scale) => {
            }
            Command::RegisterRasterResource {
                id,
                width,
                height,
                pixels,
            } if *id > 0
                && *width > 0
                && *height > 0
                && width
                    .checked_mul(*height)
                    .is_some_and(|count| count as usize == pixels.len())
                && pixels.iter().all(valid_color) => {}
            Command::LoadRasterResource {
                id,
                generation,
                source,
            } if *id > 0 && *generation > 0 && valid_resource_source(source) => {}
            Command::ReleaseRasterResource { id, generation } if *id > 0 && *generation > 0 => {}
            Command::CheckpointResource {
                id,
                generation,
                state,
                width,
                height,
            } if *id > 0
                && *generation > 0
                && match state {
                    ResourceStateFixture::Ready => {
                        width.is_some_and(|value| value > 0)
                            && height.is_some_and(|value| value > 0)
                    }
                    ResourceStateFixture::Failed | ResourceStateFixture::Released => {
                        width.is_none() && height.is_none()
                    }
                } => {}
            Command::PresentBox {
                revision,
                rect,
                background,
                border,
            } if *revision > 0
                && rect.iter().all(|value| value.is_finite())
                && rect[2] >= 0.0
                && rect[3] >= 0.0
                && valid_color(background)
                && border.as_ref().is_none_or(valid_border) => {}
            Command::PresentScene { revision, nodes }
                if *revision > 0 && valid_scene_nodes(nodes) => {}
            Command::Checkpoint {
                name,
                samples,
                relations,
            } if !name.trim().is_empty()
                && samples.iter().all(|sample| {
                    sample
                        .point
                        .iter()
                        .all(|value| value.is_finite() && *value >= 0.0)
                        && valid_color(&sample.color)
                })
                && relations.iter().all(|relation| {
                    relation
                        .first
                        .iter()
                        .chain(relation.second.iter())
                        .all(|value| value.is_finite() && *value >= 0.0)
                }) => {}
            Command::MeasureText {
                key,
                font_size,
                line_height,
                available_width,
                ..
            } if *key > 0
                && finite_positive(*font_size)
                && finite_positive(*line_height)
                && available_width.is_finite()
                && *available_width >= 0.0 => {}
            Command::CheckpointMeasurement {
                key,
                min_width,
                max_width,
                min_height,
                max_height,
                ..
            } if *key > 0
                && [*min_width, *max_width, *min_height, *max_height]
                    .iter()
                    .all(|value| value.is_finite() && *value >= 0.0)
                && min_width <= max_width
                && min_height <= max_height => {}
            Command::EmitPointer {
                timestamp_ms, x, y, ..
            } if timestamp_ms.is_finite()
                && *timestamp_ms >= 0.0
                && x.is_finite()
                && y.is_finite() => {}
            Command::CheckpointInput { x, y, .. } if x.is_finite() && y.is_finite() => {}
            _ => {
                return Err(FixtureError(format!(
                    "scenario {id} has an invalid {label} command"
                )));
            }
        }
    }
    Ok(())
}

fn valid_resource_source(source: &ResourceSourceFixture) -> bool {
    match source {
        ResourceSourceFixture::Url { value } => !value.trim().is_empty(),
        ResourceSourceFixture::Bytes { media_type, base64 } => {
            !media_type.trim().is_empty()
                && !base64.trim().is_empty()
                && base64
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
        }
    }
}

fn finite_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn valid_color(color: &ColorFixture) -> bool {
    match color {
        ColorFixture::Named { value } => !value.trim().is_empty(),
        ColorFixture::Srgba { alpha, .. } => alpha.is_finite() && (0.0..=1.0).contains(alpha),
    }
}

fn valid_border(border: &BorderFixture) -> bool {
    border
        .widths
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
        && border.radii.iter().all(|radius| radius.is_valid())
        && border.colors.iter().all(valid_color)
}

fn valid_background_geometry(geometry: &BackgroundLayerFixture) -> bool {
    geometry
        .position
        .iter()
        .all(LengthPercentageFixture::is_finite)
        && match geometry.size {
            BackgroundSizeFixture::ExplicitPair(size) => {
                size.iter().all(LengthPercentageFixture::is_finite)
            }
            BackgroundSizeFixture::ExplicitAxes { width, height } => {
                (width.is_some() || height.is_some())
                    && width.is_none_or(|value| value.is_finite())
                    && height.is_none_or(|value| value.is_finite())
            }
            BackgroundSizeFixture::Keyword(_) => true,
        }
}

fn valid_linear_gradient(gradient: &LinearGradientFixture) -> bool {
    gradient.angle_degrees.is_finite()
        && gradient.stops.len() >= 2
        && gradient
            .stops
            .iter()
            .all(|stop| valid_color(&stop.color) && stop.position.is_finite())
}

fn valid_radial_gradient(gradient: &RadialGradientFixture) -> bool {
    gradient.center.iter().all(|value| value.is_finite())
        && gradient
            .radii
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
        && gradient.stops.len() >= 2
        && gradient
            .stops
            .iter()
            .all(|stop| valid_color(&stop.color) && stop.position.is_finite())
}

fn valid_conic_gradient(gradient: &ConicGradientFixture) -> bool {
    gradient.from_degrees.is_finite()
        && gradient.center.iter().all(|value| value.is_finite())
        && gradient.stops.len() >= 2
        && gradient
            .stops
            .iter()
            .all(|stop| valid_color(&stop.color) && stop.position.is_finite())
}

fn valid_background_image(image: &BackgroundImageFixture) -> bool {
    match image {
        BackgroundImageFixture::Resource(resource) => *resource != 0,
        BackgroundImageFixture::LinearGradient(gradient) => valid_linear_gradient(gradient),
        BackgroundImageFixture::RadialGradient(gradient) => valid_radial_gradient(gradient),
        BackgroundImageFixture::ConicGradient(gradient) => valid_conic_gradient(gradient),
    }
}

fn valid_scene_nodes(nodes: &[SceneNodeFixture]) -> bool {
    if nodes.is_empty() {
        return false;
    }
    let ids = nodes
        .iter()
        .map(|node| node.id)
        .collect::<std::collections::BTreeSet<_>>();
    ids.len() == nodes.len()
        && !ids.contains(&0)
        && nodes.iter().all(|node| {
            node.parent
                .is_none_or(|parent| parent != node.id && ids.contains(&parent))
                && node.rect.iter().all(|value| value.is_finite())
                && node.rect[2] >= 0.0
                && node.rect[3] >= 0.0
                && node.content_box.is_none_or(|rect| {
                    rect.into_iter().all(f32::is_finite) && rect[2] >= 0.0 && rect[3] >= 0.0
                })
                && valid_color(&node.background)
                && node.text.as_ref().is_none_or(|text| {
                    text.font_size.is_finite()
                        && text.font_size > 0.0
                        && (1..=1000).contains(&text.font_weight)
                        && valid_color(&text.color)
                        && text.shadow.as_ref().is_none_or(|shadow| {
                            shadow.offset.into_iter().all(f32::is_finite)
                                && shadow.blur_radius.is_finite()
                                && shadow.blur_radius >= 0.0
                                && valid_color(&shadow.color)
                        })
                })
                && node.border.as_ref().is_none_or(valid_border)
                && node.box_shadows.iter().all(|shadow| {
                    shadow.offset.into_iter().all(f32::is_finite)
                        && shadow.blur_radius.is_finite()
                        && shadow.blur_radius >= 0.0
                        && shadow.spread_radius.is_finite()
                        && valid_color(&shadow.color)
                })
                && node
                    .backdrop_blur
                    .is_none_or(|radius| radius.is_finite() && radius >= 0.0)
                && node
                    .clip_path
                    .as_ref()
                    .is_none_or(|clip| match &clip.shape {
                        ClipShapeFixture::Inset { edges, radii } => {
                            edges.iter().all(LengthPercentageFixture::is_finite)
                                && radii.iter().copied().all(CornerRadiusFixture::is_valid)
                        }
                        ClipShapeFixture::Circle { radius, center } => {
                            radius.is_non_negative()
                                && center.iter().all(LengthPercentageFixture::is_finite)
                        }
                        ClipShapeFixture::Ellipse { radii, center } => {
                            radii.iter().all(LengthPercentageFixture::is_non_negative)
                                && center.iter().all(LengthPercentageFixture::is_finite)
                        }
                        ClipShapeFixture::Path { commands, .. } => {
                            !commands.is_empty()
                                && commands.iter().all(PathCommandFixture::is_valid)
                        }
                    })
                && node
                    .transform
                    .is_none_or(|transform| transform.into_iter().all(f32::is_finite))
                && node
                    .opacity
                    .is_none_or(|opacity| opacity.is_finite() && (0.0..=1.0).contains(&opacity))
                && valid_background_geometry(&node.background_layer)
                && node.background_layers.iter().all(|layer| {
                    valid_background_geometry(&layer.geometry)
                        && valid_background_image(&layer.image)
                })
                && node
                    .linear_gradient
                    .as_ref()
                    .is_none_or(valid_linear_gradient)
                && node
                    .radial_gradient
                    .as_ref()
                    .is_none_or(valid_radial_gradient)
                && node
                    .conic_gradient
                    .as_ref()
                    .is_none_or(valid_conic_gradient)
                && [
                    node.linear_gradient.is_some(),
                    node.radial_gradient.is_some(),
                    node.conic_gradient.is_some(),
                ]
                .into_iter()
                .filter(|present| *present)
                .count()
                    <= 1
                && (node.background_layers.is_empty()
                    || (node.linear_gradient.is_none()
                        && node.radial_gradient.is_none()
                        && node.conic_gradient.is_none()))
        })
        && nodes.iter().all(|node| {
            let mut seen = std::collections::BTreeSet::new();
            let mut current = Some(node.id);
            while let Some(id) = current {
                if !seen.insert(id) {
                    return false;
                }
                current = nodes
                    .iter()
                    .find(|candidate| candidate.id == id)
                    .and_then(|candidate| candidate.parent);
            }
            true
        })
}

impl LengthPercentageFixture {
    fn is_finite(&self) -> bool {
        self.length.is_finite() && self.fraction.is_finite()
    }

    fn is_non_negative(&self) -> bool {
        self.is_finite() && self.length >= 0.0 && self.fraction >= 0.0
    }
}

impl PathCommandFixture {
    fn is_valid(&self) -> bool {
        let point = |value: &[LengthPercentageFixture; 2]| {
            value.iter().all(LengthPercentageFixture::is_finite)
        };
        match self {
            Self::MoveTo { point: value } | Self::LineTo { point: value } => point(value),
            Self::QuadraticTo { control, end } => point(control) && point(end),
            Self::CubicTo {
                control_1,
                control_2,
                end,
            } => point(control_1) && point(control_2) && point(end),
            Self::Close => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn repository_suite_is_well_formed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/host-conformance");
        let (manifest, all) = load_all(&root).unwrap();
        let (_, desktop) = load_required(&root, Host::Desktop).unwrap();
        assert_eq!(all.len(), manifest.cases.len());
        assert_eq!(
            desktop.len(),
            manifest
                .cases
                .iter()
                .filter(|case| case.requires(Host::Desktop))
                .count()
        );
        assert!(desktop.iter().any(|case| case.scenario.reference.is_some()));
        assert!(desktop.iter().any(|case| case.scenario.upstream.is_none()));
    }

    #[test]
    fn capability_target_is_complete_and_disjoint() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/host-conformance");
        let source = std::fs::read_to_string(root.join("capabilities.json")).unwrap();
        let document: serde_json::Value = serde_json::from_str(&source).unwrap();
        assert_eq!(document["schema"], 2);

        let target = &document["target"];
        let properties = document["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|capability| capability["properties"].as_array().into_iter().flatten())
            .map(|property| property.as_str().unwrap())
            .collect::<Vec<_>>();
        let unique = properties.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            properties.len(),
            target["property_count"].as_u64().unwrap() as usize
        );
        assert_eq!(unique.len(), properties.len());

        let excluded = document["excluded_registered_properties"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["property"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            excluded.len(),
            target["excluded_registered_property_count"]
                .as_u64()
                .unwrap() as usize
        );
        assert!(unique.is_disjoint(&excluded));

        let pending = target["pending_registry_properties"].as_array().unwrap();
        assert_eq!(
            unique.len() - pending.len(),
            target["target_registered_property_count"].as_u64().unwrap() as usize
        );
        assert_eq!(
            target["target_registered_property_count"].as_u64().unwrap()
                + target["excluded_registered_property_count"]
                    .as_u64()
                    .unwrap(),
            target["registered_property_count"].as_u64().unwrap()
        );
        assert_eq!(
            target["property_count"].as_u64().unwrap()
                + target["non_property_features"].as_array().unwrap().len() as u64,
            target["feature_count"].as_u64().unwrap()
        );
    }
}
