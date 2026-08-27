//! Property-input composite value types.
//!
//! Some CSS properties accept a value Lynx does not document as a
//! standalone data type — e.g. `width` accepts a `<length-percentage>`,
//! `auto`, `max-content`, or a `fit-content()` function. Modeling
//! that mixture cleanly requires a Rust enum that gathers the
//! allowed forms in one place. Those enums live here so each
//! property method on [`Css`](crate::Css) can declare a precise
//! argument type.

use core::fmt;

use crate::data_type::{
    CssString, FitContent, Length, LengthPercentage, MaxContent, Number, Percentage,
};
use crate::to_css::{ToCss, write_number};

// ---------- BackdropFilter ----------

/// Supported value of `backdrop-filter`.
///
/// Whisker deliberately exposes only the app-oriented blur subset rather than
/// the complete CSS filter-function list.
#[derive(Clone, Debug, PartialEq)]
pub enum BackdropFilter {
    /// `none` — do not alter pixels behind the element.
    None,
    /// `blur(<length>)` — blur pixels already painted behind the element.
    Blur(crate::ValueOrVariable<Length>),
}

impl BackdropFilter {
    /// Creates `blur(<radius>)`.
    pub fn blur(radius: impl Into<crate::ValueOrVariable<Length>>) -> Self {
        Self::Blur(radius.into())
    }
}

impl ToCss for BackdropFilter {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::None => dest.write_str("none"),
            Self::Blur(radius) => {
                dest.write_str("blur(")?;
                radius.to_css(dest)?;
                dest.write_char(')')
            }
        }
    }
}

// ---------- Motion path ----------

/// One absolute point in an `offset-path: path()` value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionPathPoint {
    /// Horizontal logical-pixel coordinate.
    pub x: f32,
    /// Vertical logical-pixel coordinate.
    pub y: f32,
}

impl MotionPathPoint {
    /// Creates an absolute motion-path point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// One command in an absolute SVG `offset-path: path()` value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MotionPathCommand {
    /// Start a new subpath.
    MoveTo(MotionPathPoint),
    /// Add a straight segment.
    LineTo(MotionPathPoint),
    /// Add a quadratic Bezier segment.
    QuadraticTo {
        /// Curve control point.
        control: MotionPathPoint,
        /// Segment endpoint.
        to: MotionPathPoint,
    },
    /// Add a cubic Bezier segment.
    CubicTo {
        /// First curve control point.
        control1: MotionPathPoint,
        /// Second curve control point.
        control2: MotionPathPoint,
        /// Segment endpoint.
        to: MotionPathPoint,
    },
    /// Add an absolute SVG elliptical arc segment.
    ArcTo {
        /// Horizontal ellipse radius.
        radius_x: f32,
        /// Vertical ellipse radius.
        radius_y: f32,
        /// Clockwise rotation of the ellipse x axis, in degrees.
        x_axis_rotation: f32,
        /// Select the arc spanning at least 180 degrees.
        large_arc: bool,
        /// Sweep through increasing angles.
        sweep: bool,
        /// Segment endpoint.
        to: MotionPathPoint,
    },
    /// Close the current subpath.
    Close,
}

/// A typed `inset()` motion path.
#[derive(Clone, Debug, PartialEq)]
pub struct InsetPath {
    /// Top, right, bottom, and left offsets from the border box.
    pub offsets: [LengthPercentage; 4],
    /// Optional per-corner radii in CSS border-radius order.
    pub radii: Option<BorderRadius>,
}

/// Supported `offset-path` value.
#[derive(Clone, Debug, PartialEq)]
pub enum OffsetPath {
    /// Disable motion-path positioning.
    None,
    /// Follow an absolute SVG path.
    Path(Vec<MotionPathCommand>),
    /// Follow a circle resolved against the node border box.
    Circle {
        /// Radius.
        radius: LengthPercentage,
        /// Horizontal center position.
        center_x: LengthPercentage,
        /// Vertical center position.
        center_y: LengthPercentage,
    },
    /// Follow an ellipse resolved against the node border box.
    Ellipse {
        /// Horizontal radius.
        radius_x: LengthPercentage,
        /// Vertical radius.
        radius_y: LengthPercentage,
        /// Horizontal center position.
        center_x: LengthPercentage,
        /// Vertical center position.
        center_y: LengthPercentage,
    },
    /// Follow a possibly-rounded rectangle inset from the node border box.
    Inset(Box<InsetPath>),
}

impl OffsetPath {
    /// Creates a `path()` from absolute SVG commands.
    pub fn path(commands: impl Into<Vec<MotionPathCommand>>) -> Self {
        Self::Path(commands.into())
    }

    /// Creates a centered `circle()` motion path.
    pub fn circle(radius: impl Into<LengthPercentage>) -> Self {
        Self::circle_at(radius, Percentage::new(50.0), Percentage::new(50.0))
    }

    /// Creates a positioned `circle()` motion path.
    pub fn circle_at(
        radius: impl Into<LengthPercentage>,
        center_x: impl Into<LengthPercentage>,
        center_y: impl Into<LengthPercentage>,
    ) -> Self {
        Self::Circle {
            radius: radius.into(),
            center_x: center_x.into(),
            center_y: center_y.into(),
        }
    }

    /// Creates a centered `ellipse()` motion path.
    pub fn ellipse(
        radius_x: impl Into<LengthPercentage>,
        radius_y: impl Into<LengthPercentage>,
    ) -> Self {
        Self::ellipse_at(
            radius_x,
            radius_y,
            Percentage::new(50.0),
            Percentage::new(50.0),
        )
    }

    /// Creates a positioned `ellipse()` motion path.
    pub fn ellipse_at(
        radius_x: impl Into<LengthPercentage>,
        radius_y: impl Into<LengthPercentage>,
        center_x: impl Into<LengthPercentage>,
        center_y: impl Into<LengthPercentage>,
    ) -> Self {
        Self::Ellipse {
            radius_x: radius_x.into(),
            radius_y: radius_y.into(),
            center_x: center_x.into(),
            center_y: center_y.into(),
        }
    }

    /// Creates a rectangular `inset()` motion path.
    pub fn inset(
        top: impl Into<LengthPercentage>,
        right: impl Into<LengthPercentage>,
        bottom: impl Into<LengthPercentage>,
        left: impl Into<LengthPercentage>,
    ) -> Self {
        Self::Inset(Box::new(InsetPath {
            offsets: [top.into(), right.into(), bottom.into(), left.into()],
            radii: None,
        }))
    }

    /// Creates a rounded `inset()` motion path.
    pub fn inset_round(
        top: impl Into<LengthPercentage>,
        right: impl Into<LengthPercentage>,
        bottom: impl Into<LengthPercentage>,
        left: impl Into<LengthPercentage>,
        radii: BorderRadius,
    ) -> Self {
        Self::Inset(Box::new(InsetPath {
            offsets: [top.into(), right.into(), bottom.into(), left.into()],
            radii: Some(radii),
        }))
    }
}

impl ToCss for OffsetPath {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::None => dest.write_str("none"),
            Self::Path(commands) => {
                dest.write_str("path(\"")?;
                for (index, command) in commands.iter().enumerate() {
                    if index > 0 {
                        dest.write_char(' ')?;
                    }
                    match command {
                        MotionPathCommand::MoveTo(point) => {
                            dest.write_str("M ")?;
                            write_number(dest, point.x)?;
                            dest.write_char(' ')?;
                            write_number(dest, point.y)?;
                        }
                        MotionPathCommand::LineTo(point) => {
                            dest.write_str("L ")?;
                            write_number(dest, point.x)?;
                            dest.write_char(' ')?;
                            write_number(dest, point.y)?;
                        }
                        MotionPathCommand::QuadraticTo { control, to } => {
                            dest.write_str("Q ")?;
                            write_number(dest, control.x)?;
                            dest.write_char(' ')?;
                            write_number(dest, control.y)?;
                            dest.write_char(' ')?;
                            write_number(dest, to.x)?;
                            dest.write_char(' ')?;
                            write_number(dest, to.y)?;
                        }
                        MotionPathCommand::CubicTo {
                            control1,
                            control2,
                            to,
                        } => {
                            dest.write_str("C ")?;
                            write_number(dest, control1.x)?;
                            dest.write_char(' ')?;
                            write_number(dest, control1.y)?;
                            dest.write_char(' ')?;
                            write_number(dest, control2.x)?;
                            dest.write_char(' ')?;
                            write_number(dest, control2.y)?;
                            dest.write_char(' ')?;
                            write_number(dest, to.x)?;
                            dest.write_char(' ')?;
                            write_number(dest, to.y)?;
                        }
                        MotionPathCommand::ArcTo {
                            radius_x,
                            radius_y,
                            x_axis_rotation,
                            large_arc,
                            sweep,
                            to,
                        } => {
                            dest.write_str("A ")?;
                            write_number(dest, *radius_x)?;
                            dest.write_char(' ')?;
                            write_number(dest, *radius_y)?;
                            dest.write_char(' ')?;
                            write_number(dest, *x_axis_rotation)?;
                            dest.write_char(' ')?;
                            dest.write_char(if *large_arc { '1' } else { '0' })?;
                            dest.write_char(' ')?;
                            dest.write_char(if *sweep { '1' } else { '0' })?;
                            dest.write_char(' ')?;
                            write_number(dest, to.x)?;
                            dest.write_char(' ')?;
                            write_number(dest, to.y)?;
                        }
                        MotionPathCommand::Close => dest.write_char('Z')?,
                    }
                }
                dest.write_str("\")")
            }
            Self::Circle {
                radius,
                center_x,
                center_y,
            } => {
                dest.write_str("circle(")?;
                radius.to_css(dest)?;
                dest.write_str(" at ")?;
                center_x.to_css(dest)?;
                dest.write_char(' ')?;
                center_y.to_css(dest)?;
                dest.write_char(')')
            }
            Self::Ellipse {
                radius_x,
                radius_y,
                center_x,
                center_y,
            } => {
                dest.write_str("ellipse(")?;
                radius_x.to_css(dest)?;
                dest.write_char(' ')?;
                radius_y.to_css(dest)?;
                dest.write_str(" at ")?;
                center_x.to_css(dest)?;
                dest.write_char(' ')?;
                center_y.to_css(dest)?;
                dest.write_char(')')
            }
            Self::Inset(value) => {
                dest.write_str("inset(")?;
                write_four(dest, &value.offsets)?;
                if let Some(radii) = &value.radii {
                    dest.write_str(" round ")?;
                    radii.to_css(dest)?;
                }
                dest.write_char(')')
            }
        }
    }
}

/// Supported `offset-distance` value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OffsetDistance {
    /// Unitless normalized progress in the `0..=1` range.
    Number(Number),
    /// Percentage progress in the `0%..=100%` range.
    Percentage(Percentage),
}

impl From<Number> for OffsetDistance {
    fn from(value: Number) -> Self {
        Self::Number(value)
    }
}

impl From<Percentage> for OffsetDistance {
    fn from(value: Percentage) -> Self {
        Self::Percentage(value)
    }
}

impl ToCss for OffsetDistance {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::Number(value) => value.to_css(dest),
            Self::Percentage(value) => value.to_css(dest),
        }
    }
}

/// Supported `offset-rotate` value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OffsetRotate {
    /// Follow the path tangent.
    Auto,
    /// Use a fixed clockwise angle.
    Angle(crate::Angle),
}

impl ToCss for OffsetRotate {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::Auto => dest.write_str("auto"),
            Self::Angle(angle) => angle.to_css(dest),
        }
    }
}

// ---------- Size (width / height / min-/max-) ----------

/// Value for `width`, `height`, `min-width`, `min-height`,
/// `max-width`, `max-height`.
#[derive(Clone, Debug, PartialEq)]
pub enum Size {
    /// `auto` — let the layout algorithm choose.
    Auto,
    /// An explicit length or percentage.
    LengthPercentage(LengthPercentage),
    /// `max-content` — the maximum intrinsic content size.
    MaxContent,
    /// `min-content` — the minimum intrinsic content size.
    MinContent,
    /// `fit-content` (or `fit-content(<limit>)`).
    FitContent(FitContent),
    /// `none` — only valid for `max-*` properties.
    None,
}

impl ToCss for Size {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Size::Auto => dest.write_str("auto"),
            Size::LengthPercentage(lp) => lp.to_css(dest),
            Size::MaxContent => dest.write_str("max-content"),
            Size::MinContent => dest.write_str("min-content"),
            Size::FitContent(fc) => fc.to_css(dest),
            Size::None => dest.write_str("none"),
        }
    }
}

impl From<Length> for Size {
    fn from(l: Length) -> Self {
        Self::LengthPercentage(l.into())
    }
}

impl From<Percentage> for Size {
    fn from(p: Percentage) -> Self {
        Self::LengthPercentage(p.into())
    }
}

impl From<LengthPercentage> for Size {
    fn from(lp: LengthPercentage) -> Self {
        Self::LengthPercentage(lp)
    }
}

impl From<MaxContent> for Size {
    fn from(_: MaxContent) -> Self {
        Self::MaxContent
    }
}

impl From<FitContent> for Size {
    fn from(fc: FitContent) -> Self {
        Self::FitContent(fc)
    }
}

// ---------- FlexBasis ----------

/// Value for `flex-basis`.
#[derive(Clone, Debug, PartialEq)]
pub enum FlexBasis {
    /// `auto` — basis comes from the item's `width`/`height`.
    Auto,
    /// `content` — basis is the content size.
    Content,
    /// An explicit length or percentage.
    LengthPercentage(LengthPercentage),
}

impl ToCss for FlexBasis {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            FlexBasis::Auto => dest.write_str("auto"),
            FlexBasis::Content => dest.write_str("content"),
            FlexBasis::LengthPercentage(lp) => lp.to_css(dest),
        }
    }
}

impl From<Length> for FlexBasis {
    fn from(l: Length) -> Self {
        Self::LengthPercentage(l.into())
    }
}

impl From<Percentage> for FlexBasis {
    fn from(p: Percentage) -> Self {
        Self::LengthPercentage(p.into())
    }
}

impl From<LengthPercentage> for FlexBasis {
    fn from(lp: LengthPercentage) -> Self {
        Self::LengthPercentage(lp)
    }
}

// ---------- LineHeight ----------

/// Value for `line-height`.
#[derive(Clone, Debug, PartialEq)]
pub enum LineHeight {
    /// `normal` — engine-chosen line height.
    Normal,
    /// Unit-less multiplier of the element's `font-size`.
    Number(f32),
    /// Explicit length or percentage.
    LengthPercentage(LengthPercentage),
}

impl ToCss for LineHeight {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            LineHeight::Normal => dest.write_str("normal"),
            LineHeight::Number(n) => write_number(dest, *n),
            LineHeight::LengthPercentage(lp) => lp.to_css(dest),
        }
    }
}

impl From<Length> for LineHeight {
    fn from(l: Length) -> Self {
        Self::LengthPercentage(l.into())
    }
}

impl From<Percentage> for LineHeight {
    fn from(p: Percentage) -> Self {
        Self::LengthPercentage(p.into())
    }
}

impl From<LengthPercentage> for LineHeight {
    fn from(lp: LengthPercentage) -> Self {
        Self::LengthPercentage(lp)
    }
}

impl From<f32> for LineHeight {
    fn from(v: f32) -> Self {
        Self::Number(v)
    }
}

// ---------- ImageRef (background-image, etc.) ----------

/// A reference to an image resource. Lynx accepts `url("...")`,
/// `linear-gradient(...)`, and `radial-gradient(...)`. `conic-gradient`
/// is supported on background-image but represented via [`crate::Gradient`].
#[derive(Clone, Debug, PartialEq)]
pub enum ImageRef {
    /// `none` — no image.
    None,
    /// `url("<path>")`.
    Url(CssString),
    /// One of the `<gradient>` functions.
    Gradient(crate::Gradient),
}

impl ToCss for ImageRef {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            ImageRef::None => dest.write_str("none"),
            ImageRef::Url(s) => {
                dest.write_str("url(")?;
                s.to_css(dest)?;
                dest.write_char(')')
            }
            ImageRef::Gradient(g) => g.to_css(dest),
        }
    }
}

impl From<crate::Gradient> for ImageRef {
    fn from(g: crate::Gradient) -> Self {
        Self::Gradient(g)
    }
}

// ---------- BorderRadius (4 corners + optional elliptical y) ----------

/// Value for the `border-radius` shorthand. Stores per-corner
/// radii, optionally with an elliptical second axis.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderRadius {
    /// Horizontal radii: top-left, top-right, bottom-right, bottom-left.
    pub horizontal: [LengthPercentage; 4],
    /// Optional vertical radii for an elliptical corner.
    pub vertical: Option<[LengthPercentage; 4]>,
}

impl BorderRadius {
    /// All four corners share the same radius.
    pub fn all(v: impl Into<LengthPercentage>) -> Self {
        let v = v.into();
        Self {
            horizontal: [v.clone(), v.clone(), v.clone(), v],
            vertical: None,
        }
    }

    /// Specify each corner explicitly (top-left, top-right, bottom-right, bottom-left).
    pub fn corners(
        tl: impl Into<LengthPercentage>,
        tr: impl Into<LengthPercentage>,
        br: impl Into<LengthPercentage>,
        bl: impl Into<LengthPercentage>,
    ) -> Self {
        Self {
            horizontal: [tl.into(), tr.into(), br.into(), bl.into()],
            vertical: None,
        }
    }

    /// Elliptical radius: horizontal and vertical components.
    pub fn elliptical(horizontal: [LengthPercentage; 4], vertical: [LengthPercentage; 4]) -> Self {
        Self {
            horizontal,
            vertical: Some(vertical),
        }
    }
}

impl ToCss for BorderRadius {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        write_four(dest, &self.horizontal)?;
        if let Some(v) = &self.vertical {
            dest.write_str(" / ")?;
            write_four(dest, v)?;
        }
        Ok(())
    }
}

fn write_four(dest: &mut dyn fmt::Write, v: &[LengthPercentage; 4]) -> fmt::Result {
    for (i, item) in v.iter().enumerate() {
        if i > 0 {
            dest.write_char(' ')?;
        }
        item.to_css(dest)?;
    }
    Ok(())
}

// ---------- CSS Grid ----------

/// Value for `grid-row-start`, `grid-row-end`, `grid-column-start`,
/// `grid-column-end`. Lynx accepts numeric line references and
/// `span <integer>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GridLine {
    /// `auto` — let the layout algorithm decide.
    Auto,
    /// Numeric line reference; negative values count from the end.
    Number(i16),
    /// `span <integer>` — span N tracks from the opposite edge.
    Span(u16),
    /// A named line, optionally selecting the nth occurrence.
    Named(String, i16),
    /// Span to a named line.
    NamedSpan(String, u16),
}

impl ToCss for GridLine {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            GridLine::Auto => dest.write_str("auto"),
            GridLine::Number(n) => write!(dest, "{n}"),
            GridLine::Span(n) => write!(dest, "span {n}"),
            GridLine::Named(name, occurrence) if *occurrence == 0 => dest.write_str(name),
            GridLine::Named(name, occurrence) => write!(dest, "{occurrence} {name}"),
            GridLine::NamedSpan(name, occurrence) if *occurrence == 0 => {
                write!(dest, "span {name}")
            }
            GridLine::NamedSpan(name, occurrence) => write!(dest, "span {occurrence} {name}"),
        }
    }
}

/// Minimum sizing function accepted by `minmax()`.
#[derive(Clone, Debug, PartialEq)]
pub enum GridTrackMin {
    /// A fixed length or percentage.
    Fixed(LengthPercentage),
    /// The minimum intrinsic contribution.
    MinContent,
    /// The maximum intrinsic contribution.
    MaxContent,
    /// Automatic minimum sizing.
    Auto,
}

/// Maximum sizing function accepted by `minmax()`.
#[derive(Clone, Debug, PartialEq)]
pub enum GridTrackMax {
    /// A fixed length or percentage.
    Fixed(LengthPercentage),
    /// The minimum intrinsic contribution.
    MinContent,
    /// The maximum intrinsic contribution.
    MaxContent,
    /// `fit-content(<limit>)`.
    FitContent(LengthPercentage),
    /// Automatic maximum sizing.
    Auto,
    /// A flexible share in `fr` units.
    Fraction(f32),
}

/// One CSS Grid track sizing function.
#[derive(Clone, Debug, PartialEq)]
pub struct GridTrack {
    pub(crate) min: GridTrackMin,
    pub(crate) max: GridTrackMax,
}

impl GridTrack {
    /// `auto`.
    pub const fn auto() -> Self {
        Self {
            min: GridTrackMin::Auto,
            max: GridTrackMax::Auto,
        }
    }

    /// `min-content`.
    pub const fn min_content() -> Self {
        Self {
            min: GridTrackMin::MinContent,
            max: GridTrackMax::MinContent,
        }
    }

    /// `max-content`.
    pub const fn max_content() -> Self {
        Self {
            min: GridTrackMin::MaxContent,
            max: GridTrackMax::MaxContent,
        }
    }

    /// A fixed length or percentage.
    pub fn fixed(value: impl Into<LengthPercentage>) -> Self {
        let value = value.into();
        Self {
            min: GridTrackMin::Fixed(value.clone()),
            max: GridTrackMax::Fixed(value),
        }
    }

    /// A flexible `fr` track.
    pub const fn fraction(value: f32) -> Self {
        Self {
            min: GridTrackMin::Auto,
            max: GridTrackMax::Fraction(value),
        }
    }

    /// `fit-content(<limit>)`.
    pub fn fit_content(limit: impl Into<LengthPercentage>) -> Self {
        Self {
            min: GridTrackMin::Auto,
            max: GridTrackMax::FitContent(limit.into()),
        }
    }

    /// `minmax(<min>, <max>)`.
    pub const fn minmax(min: GridTrackMin, max: GridTrackMax) -> Self {
        Self { min, max }
    }
}

impl From<Length> for GridTrack {
    fn from(value: Length) -> Self {
        Self::fixed(value)
    }
}

impl From<Percentage> for GridTrack {
    fn from(value: Percentage) -> Self {
        Self::fixed(value)
    }
}

impl From<LengthPercentage> for GridTrack {
    fn from(value: LengthPercentage) -> Self {
        Self::fixed(value)
    }
}

impl From<Length> for GridTrackMin {
    fn from(value: Length) -> Self {
        Self::Fixed(value.into())
    }
}

impl From<Percentage> for GridTrackMin {
    fn from(value: Percentage) -> Self {
        Self::Fixed(value.into())
    }
}

impl From<LengthPercentage> for GridTrackMin {
    fn from(value: LengthPercentage) -> Self {
        Self::Fixed(value)
    }
}

impl From<Length> for GridTrackMax {
    fn from(value: Length) -> Self {
        Self::Fixed(value.into())
    }
}

impl From<Percentage> for GridTrackMax {
    fn from(value: Percentage) -> Self {
        Self::Fixed(value.into())
    }
}

impl From<LengthPercentage> for GridTrackMax {
    fn from(value: LengthPercentage) -> Self {
        Self::Fixed(value)
    }
}

impl ToCss for GridTrack {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match (&self.min, &self.max) {
            (GridTrackMin::Auto, GridTrackMax::Auto) => dest.write_str("auto"),
            (GridTrackMin::MinContent, GridTrackMax::MinContent) => dest.write_str("min-content"),
            (GridTrackMin::MaxContent, GridTrackMax::MaxContent) => dest.write_str("max-content"),
            (GridTrackMin::Fixed(min), GridTrackMax::Fixed(max)) if min == max => min.to_css(dest),
            (GridTrackMin::Auto, GridTrackMax::Fraction(value)) => {
                write_number(dest, *value)?;
                dest.write_str("fr")
            }
            (GridTrackMin::Auto, GridTrackMax::FitContent(limit)) => {
                dest.write_str("fit-content(")?;
                limit.to_css(dest)?;
                dest.write_char(')')
            }
            (min, max) => {
                dest.write_str("minmax(")?;
                min.to_css(dest)?;
                dest.write_str(", ")?;
                max.to_css(dest)?;
                dest.write_char(')')
            }
        }
    }
}

impl ToCss for GridTrackMin {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::Fixed(value) => value.to_css(dest),
            Self::MinContent => dest.write_str("min-content"),
            Self::MaxContent => dest.write_str("max-content"),
            Self::Auto => dest.write_str("auto"),
        }
    }
}

impl ToCss for GridTrackMax {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::Fixed(value) => value.to_css(dest),
            Self::MinContent => dest.write_str("min-content"),
            Self::MaxContent => dest.write_str("max-content"),
            Self::FitContent(limit) => {
                dest.write_str("fit-content(")?;
                limit.to_css(dest)?;
                dest.write_char(')')
            }
            Self::Auto => dest.write_str("auto"),
            Self::Fraction(value) => {
                write_number(dest, *value)?;
                dest.write_str("fr")
            }
        }
    }
}

/// Count used by a Grid `repeat()` fragment.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GridRepeatCount {
    /// Repeat a fixed number of times.
    Count(u16),
    /// Fill the available axis while retaining empty repeated tracks.
    AutoFill,
    /// Fill the available axis and collapse empty repeated tracks.
    AutoFit,
}

/// One explicit track or repeated track fragment.
#[derive(Clone, Debug, PartialEq)]
pub enum GridTemplateComponent {
    /// One track.
    Track(GridTrack),
    /// A repeated fragment.
    Repeat {
        /// Fixed or automatic repetition count.
        count: GridRepeatCount,
        /// Tracks inside the repeated fragment.
        tracks: Vec<GridTrack>,
        /// Named lines before, between, and after repeated tracks.
        line_names: Vec<Vec<String>>,
    },
}

impl From<GridTrack> for GridTemplateComponent {
    fn from(value: GridTrack) -> Self {
        Self::Track(value)
    }
}

/// Value for `grid-template-rows` / `grid-template-columns`.
#[derive(Clone, Debug, PartialEq)]
pub struct GridTemplate {
    pub(crate) components: Vec<GridTemplateComponent>,
    pub(crate) line_names: Vec<Vec<String>>,
}

impl GridTemplate {
    /// Build from a list of track-sizing tokens. Each token is
    /// joined with a space.
    pub fn tracks(tracks: impl IntoIterator<Item = impl Into<GridTrack>>) -> Self {
        let components: Vec<_> = tracks
            .into_iter()
            .map(|track| track.into().into())
            .collect();
        let line_names = vec![Vec::new(); components.len() + 1];
        Self {
            components,
            line_names,
        }
    }

    /// Build a template from explicit track and `repeat()` components.
    pub fn components(components: impl IntoIterator<Item = GridTemplateComponent>) -> Self {
        let components: Vec<_> = components.into_iter().collect();
        let line_names = vec![Vec::new(); components.len() + 1];
        Self {
            components,
            line_names,
        }
    }

    /// Build a template containing one `repeat()` fragment.
    pub fn repeat(
        count: GridRepeatCount,
        tracks: impl IntoIterator<Item = impl Into<GridTrack>>,
    ) -> Self {
        let tracks: Vec<_> = tracks.into_iter().map(Into::into).collect();
        let line_names = vec![Vec::new(); tracks.len() + 1];
        Self::components([GridTemplateComponent::Repeat {
            count,
            tracks,
            line_names,
        }])
    }

    /// Attach names to the lines before, between, and after components.
    /// Invalid line-name counts are rejected during style resolution.
    pub fn line_names(
        mut self,
        line_names: impl IntoIterator<Item = impl IntoIterator<Item = impl Into<String>>>,
    ) -> Self {
        self.line_names = line_names
            .into_iter()
            .map(|names| names.into_iter().map(Into::into).collect())
            .collect();
        self
    }
}

impl ToCss for GridTemplate {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        for (index, component) in self.components.iter().enumerate() {
            if index > 0 {
                dest.write_char(' ')?;
            }
            write_grid_line_names(dest, self.line_names.get(index))?;
            if !self.line_names.get(index).is_none_or(Vec::is_empty) {
                dest.write_char(' ')?;
            }
            component.to_css(dest)?;
        }
        if !self
            .line_names
            .get(self.components.len())
            .is_none_or(Vec::is_empty)
        {
            if !self.components.is_empty() {
                dest.write_char(' ')?;
            }
            write_grid_line_names(dest, self.line_names.get(self.components.len()))?;
        }
        Ok(())
    }
}

impl ToCss for GridTemplateComponent {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::Track(track) => track.to_css(dest),
            Self::Repeat {
                count,
                tracks,
                line_names,
            } => {
                dest.write_str("repeat(")?;
                count.to_css(dest)?;
                dest.write_str(", ")?;
                for (index, track) in tracks.iter().enumerate() {
                    if index > 0 {
                        dest.write_char(' ')?;
                    }
                    write_grid_line_names(dest, line_names.get(index))?;
                    if !line_names.get(index).is_none_or(Vec::is_empty) {
                        dest.write_char(' ')?;
                    }
                    track.to_css(dest)?;
                }
                if !line_names.get(tracks.len()).is_none_or(Vec::is_empty) {
                    if !tracks.is_empty() {
                        dest.write_char(' ')?;
                    }
                    write_grid_line_names(dest, line_names.get(tracks.len()))?;
                }
                dest.write_char(')')
            }
        }
    }
}

impl ToCss for GridRepeatCount {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::Count(value) => write!(dest, "{value}"),
            Self::AutoFill => dest.write_str("auto-fill"),
            Self::AutoFit => dest.write_str("auto-fit"),
        }
    }
}

fn write_grid_line_names(dest: &mut dyn fmt::Write, names: Option<&Vec<String>>) -> fmt::Result {
    let Some(names) = names.filter(|names| !names.is_empty()) else {
        return Ok(());
    };
    dest.write_char('[')?;
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            dest.write_char(' ')?;
        }
        dest.write_str(name)?;
    }
    dest.write_char(']')
}

/// One rectangular named region in `grid-template-areas`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GridArea {
    pub(crate) name: String,
    pub(crate) row_start: u16,
    pub(crate) row_end: u16,
    pub(crate) column_start: u16,
    pub(crate) column_end: u16,
}

impl GridArea {
    /// Defines a zero-based, end-exclusive rectangular area.
    pub fn new(
        name: impl Into<String>,
        row_start: u16,
        row_end: u16,
        column_start: u16,
        column_end: u16,
    ) -> Self {
        Self {
            name: name.into(),
            row_start,
            row_end,
            column_start,
            column_end,
        }
    }
}

/// Rectangular named regions for `grid-template-areas`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GridTemplateAreas {
    pub(crate) row_count: u16,
    pub(crate) column_count: u16,
    pub(crate) areas: Vec<GridArea>,
}

impl GridTemplateAreas {
    /// Creates an empty named-area matrix with explicit dimensions.
    pub const fn new(row_count: u16, column_count: u16) -> Self {
        Self {
            row_count,
            column_count,
            areas: Vec::new(),
        }
    }

    /// Adds one named rectangular area.
    pub fn area(mut self, area: GridArea) -> Self {
        self.areas.push(area);
        self
    }
}

impl ToCss for GridTemplateAreas {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        for row in 0..self.row_count {
            if row > 0 {
                dest.write_char(' ')?;
            }
            dest.write_char('"')?;
            for column in 0..self.column_count {
                if column > 0 {
                    dest.write_char(' ')?;
                }
                let name = self
                    .areas
                    .iter()
                    .rev()
                    .find(|area| {
                        area.row_start <= row
                            && row < area.row_end
                            && area.column_start <= column
                            && column < area.column_end
                    })
                    .map_or(".", |area| area.name.as_str());
                dest.write_str(name)?;
            }
            dest.write_char('"')?;
        }
        Ok(())
    }
}

// ---------- Repeated (animation-name list etc.) ----------

/// Comma-separated list of values, used for properties like
/// `animation-name`, `transition-property`, `background-image`.
#[derive(Clone, Debug, PartialEq)]
pub struct Repeated<T>(pub Vec<T>);

impl<T> Repeated<T> {
    /// Wrap a `Vec<T>`.
    pub fn new(v: impl IntoIterator<Item = T>) -> Self {
        Self(v.into_iter().collect())
    }
}

impl<T: ToCss> ToCss for Repeated<T> {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        for (i, item) in self.0.iter().enumerate() {
            if i > 0 {
                dest.write_str(", ")?;
            }
            item.to_css(dest)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_type::{ColorStop, Gradient, Length, NamedColor};
    use crate::ext::*;

    #[test]
    fn size_keywords() {
        assert_eq!(Size::Auto.to_css_string(), "auto");
        assert_eq!(Size::MaxContent.to_css_string(), "max-content");
        assert_eq!(Size::MinContent.to_css_string(), "min-content");
        assert_eq!(Size::None.to_css_string(), "none");
    }

    #[test]
    fn size_from_lengths_and_percentages() {
        let from_len: Size = px(8).into();
        let from_pct: Size = 50.percent().into();
        let from_lp: Size = LengthPercentage::Length(Length::Px(4.0)).into();
        let from_mc: Size = MaxContent.into();
        let from_fc: Size = FitContent::keyword().into();
        assert_eq!(from_len.to_css_string(), "8px");
        assert_eq!(from_pct.to_css_string(), "50%");
        assert_eq!(from_lp.to_css_string(), "4px");
        assert_eq!(from_mc.to_css_string(), "max-content");
        assert_eq!(from_fc.to_css_string(), "fit-content");
    }

    #[test]
    fn size_fit_content_with_limit() {
        let s = Size::FitContent(FitContent::with_limit(px(200)));
        assert_eq!(s.to_css_string(), "fit-content(200px)");
    }

    #[test]
    fn flex_basis_variants() {
        assert_eq!(FlexBasis::Auto.to_css_string(), "auto");
        assert_eq!(FlexBasis::Content.to_css_string(), "content");
        let from_len: FlexBasis = px(120).into();
        let from_pct: FlexBasis = 25.percent().into();
        let from_lp: FlexBasis = LengthPercentage::Length(Length::Px(8.0)).into();
        assert_eq!(from_len.to_css_string(), "120px");
        assert_eq!(from_pct.to_css_string(), "25%");
        assert_eq!(from_lp.to_css_string(), "8px");
    }

    #[test]
    fn line_height_variants() {
        assert_eq!(LineHeight::Normal.to_css_string(), "normal");
        let n: LineHeight = 1.5_f32.into();
        let from_len: LineHeight = px(20).into();
        let from_pct: LineHeight = 150.percent().into();
        let from_lp: LineHeight = LengthPercentage::Length(Length::Px(10.0)).into();
        assert_eq!(n.to_css_string(), "1.5");
        assert_eq!(from_len.to_css_string(), "20px");
        assert_eq!(from_pct.to_css_string(), "150%");
        assert_eq!(from_lp.to_css_string(), "10px");
    }

    #[test]
    fn image_ref_variants() {
        assert_eq!(ImageRef::None.to_css_string(), "none");
        assert_eq!(
            ImageRef::Url(CssString::new("a.png")).to_css_string(),
            "url(\"a.png\")"
        );
        let g = Gradient::linear_to_bottom([ColorStop::new(crate::Color::Named(NamedColor::Red))]);
        let r: ImageRef = g.into();
        assert_eq!(r.to_css_string(), "linear-gradient(to bottom, red)");
    }

    #[test]
    fn border_radius_uniform() {
        let r = BorderRadius::all(px(8));
        assert_eq!(r.to_css_string(), "8px 8px 8px 8px");
    }

    #[test]
    fn border_radius_corners() {
        let r = BorderRadius::corners(px(2), px(4), px(6), px(8));
        assert_eq!(r.to_css_string(), "2px 4px 6px 8px");
    }

    #[test]
    fn border_radius_elliptical() {
        let h = [px(2).into(), px(4).into(), px(6).into(), px(8).into()];
        let v = [px(20).into(), px(40).into(), px(60).into(), px(80).into()];
        let r = BorderRadius::elliptical(h, v);
        assert_eq!(r.to_css_string(), "2px 4px 6px 8px / 20px 40px 60px 80px");
    }

    #[test]
    fn grid_line_variants() {
        assert_eq!(GridLine::Auto.to_css_string(), "auto");
        assert_eq!(GridLine::Number(1).to_css_string(), "1");
        assert_eq!(GridLine::Number(-1).to_css_string(), "-1");
        assert_eq!(GridLine::Span(2).to_css_string(), "span 2");
        assert_eq!(
            GridLine::Named("content".into(), 0).to_css_string(),
            "content"
        );
        assert_eq!(
            GridLine::NamedSpan("content".into(), 2).to_css_string(),
            "span 2 content"
        );
    }

    #[test]
    fn grid_template_joins_tracks() {
        let t = GridTemplate::tracks([
            GridTrack::fraction(1.0),
            GridTrack::auto(),
            GridTrack::fraction(2.0),
        ]);
        assert_eq!(t.to_css_string(), "1fr auto 2fr");
    }

    #[test]
    fn repeated_serializes_with_commas() {
        let r = Repeated::new([Length::Px(8.0), Length::Px(16.0)]);
        assert_eq!(r.to_css_string(), "8px, 16px");
    }
}
