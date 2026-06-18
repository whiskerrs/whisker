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

use crate::data_type::{CssString, FitContent, Length, LengthPercentage, MaxContent, Percentage};
use crate::to_css::{ToCss, write_number};

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

// ---------- GridLine, GridTemplate ----------

/// Value for `grid-row-start`, `grid-row-end`, `grid-column-start`,
/// `grid-column-end`. Lynx accepts numeric line references and
/// `span <integer>`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum GridLine {
    /// `auto` — let the layout algorithm decide.
    Auto,
    /// Numeric line reference; negative values count from the end.
    Number(i32),
    /// `span <integer>` — span N tracks from the opposite edge.
    Span(u32),
}

impl ToCss for GridLine {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            GridLine::Auto => dest.write_str("auto"),
            GridLine::Number(n) => write!(dest, "{n}"),
            GridLine::Span(n) => write!(dest, "span {n}"),
        }
    }
}

/// Value for `grid-template-rows` / `grid-template-columns`. Lynx
/// accepts a sequence of track-sizing values; this struct stores
/// them as already-serialized track strings since the grammar is
/// rich enough that a typed model is impractical.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GridTemplate(pub String);

impl GridTemplate {
    /// Build from a list of track-sizing tokens. Each token is
    /// joined with a space.
    pub fn tracks(tracks: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut out = String::new();
        for (i, t) in tracks.into_iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&t.into());
        }
        Self(out)
    }
}

impl ToCss for GridTemplate {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(&self.0)
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
        let g = Gradient::linear_to_bottom([ColorStop::new(NamedColor::Red.into())]);
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
    }

    #[test]
    fn grid_template_joins_tracks() {
        let t = GridTemplate::tracks(["1fr", "auto", "2fr"]);
        assert_eq!(t.to_css_string(), "1fr auto 2fr");
    }

    #[test]
    fn repeated_serializes_with_commas() {
        let r = Repeated::new([Length::Px(8.0), Length::Px(16.0)]);
        assert_eq!(r.to_css_string(), "8px, 16px");
    }
}
