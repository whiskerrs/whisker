//! Background-related keyword enums.
//!
//! References:
//! - <https://lynxjs.org/api/css/properties/background-repeat>
//! - <https://lynxjs.org/api/css/properties/background-clip>
//! - <https://lynxjs.org/api/css/properties/background-origin>
//! - <https://lynxjs.org/api/css/properties/background-size>
//! - <https://lynxjs.org/api/css/properties/background-attachment>

use core::fmt;

use crate::data_type::{Length, LengthPercentage, Percentage};
use crate::to_css::ToCss;

/// The `background-repeat` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundRepeat {
    /// `repeat` — tile both axes. Default.
    Repeat,
    /// `no-repeat` — no tiling.
    NoRepeat,
    /// `repeat-x` — tile horizontally only.
    RepeatX,
    /// `repeat-y` — tile vertically only.
    RepeatY,
    /// `space` — tile with extra space between tiles to fill the box.
    Space,
    /// `round` — tile, scaling each tile so an integer number fit.
    Round,
}

impl ToCss for BackgroundRepeat {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            BackgroundRepeat::Repeat => "repeat",
            BackgroundRepeat::NoRepeat => "no-repeat",
            BackgroundRepeat::RepeatX => "repeat-x",
            BackgroundRepeat::RepeatY => "repeat-y",
            BackgroundRepeat::Space => "space",
            BackgroundRepeat::Round => "round",
        })
    }
}

/// The `background-clip` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundClip {
    /// `border-box` — clip to the border box. Default.
    BorderBox,
    /// `padding-box` — clip to the padding box.
    PaddingBox,
    /// `content-box` — clip to the content box.
    ContentBox,
    /// `border-area` — clip to the area painted by the border.
    BorderArea,
}

impl ToCss for BackgroundClip {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            BackgroundClip::BorderBox => "border-box",
            BackgroundClip::PaddingBox => "padding-box",
            BackgroundClip::ContentBox => "content-box",
            BackgroundClip::BorderArea => "border-area",
        })
    }
}

/// The `background-origin` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundOrigin {
    /// `border-box` — origin at the border box.
    BorderBox,
    /// `padding-box` — origin at the padding box. Default.
    PaddingBox,
    /// `content-box` — origin at the content box.
    ContentBox,
}

impl ToCss for BackgroundOrigin {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            BackgroundOrigin::BorderBox => "border-box",
            BackgroundOrigin::PaddingBox => "padding-box",
            BackgroundOrigin::ContentBox => "content-box",
        })
    }
}

/// The `background-attachment` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundAttachment {
    /// `scroll` — background scrolls with the element. Default.
    Scroll,
    /// `fixed` — background is fixed relative to the viewport.
    Fixed,
    /// `local` — background scrolls with the element's content.
    Local,
}

impl ToCss for BackgroundAttachment {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            BackgroundAttachment::Scroll => "scroll",
            BackgroundAttachment::Fixed => "fixed",
            BackgroundAttachment::Local => "local",
        })
    }
}

/// The `background-size` value: a keyword or an explicit pair of
/// length-percentages.
#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundSize {
    /// `auto` — use the intrinsic size.
    Auto,
    /// `cover` — scale to cover the entire box, possibly cropping.
    Cover,
    /// `contain` — scale to fit within the box without cropping.
    Contain,
    /// Explicit width × height; either axis may retain intrinsic sizing.
    Explicit(BackgroundSizeAxis, BackgroundSizeAxis),
}

/// One axis of an explicit `background-size` pair.
#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundSizeAxis {
    /// Retain the intrinsic size on this axis.
    Auto,
    /// Resolve one length-percentage against the positioning area.
    Value(LengthPercentage),
}

impl From<LengthPercentage> for BackgroundSizeAxis {
    fn from(value: LengthPercentage) -> Self {
        Self::Value(value)
    }
}

impl From<Length> for BackgroundSizeAxis {
    fn from(value: Length) -> Self {
        Self::Value(value.into())
    }
}

impl From<Percentage> for BackgroundSizeAxis {
    fn from(value: Percentage) -> Self {
        Self::Value(value.into())
    }
}

impl ToCss for BackgroundSizeAxis {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::Auto => dest.write_str("auto"),
            Self::Value(value) => value.to_css(dest),
        }
    }
}

impl ToCss for BackgroundSize {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            BackgroundSize::Auto => dest.write_str("auto"),
            BackgroundSize::Cover => dest.write_str("cover"),
            BackgroundSize::Contain => dest.write_str("contain"),
            BackgroundSize::Explicit(w, h) => {
                w.to_css(dest)?;
                dest.write_char(' ')?;
                h.to_css(dest)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_type::{Length, Percentage};

    #[test]
    fn background_repeat_all() {
        let cases = [
            (BackgroundRepeat::Repeat, "repeat"),
            (BackgroundRepeat::NoRepeat, "no-repeat"),
            (BackgroundRepeat::RepeatX, "repeat-x"),
            (BackgroundRepeat::RepeatY, "repeat-y"),
            (BackgroundRepeat::Space, "space"),
            (BackgroundRepeat::Round, "round"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn background_clip_all() {
        let cases = [
            (BackgroundClip::BorderBox, "border-box"),
            (BackgroundClip::PaddingBox, "padding-box"),
            (BackgroundClip::ContentBox, "content-box"),
            (BackgroundClip::BorderArea, "border-area"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn background_origin_all() {
        let cases = [
            (BackgroundOrigin::BorderBox, "border-box"),
            (BackgroundOrigin::PaddingBox, "padding-box"),
            (BackgroundOrigin::ContentBox, "content-box"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn background_attachment_all() {
        let cases = [
            (BackgroundAttachment::Scroll, "scroll"),
            (BackgroundAttachment::Fixed, "fixed"),
            (BackgroundAttachment::Local, "local"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn background_size_keywords_and_explicit() {
        assert_eq!(BackgroundSize::Auto.to_css_string(), "auto");
        assert_eq!(BackgroundSize::Cover.to_css_string(), "cover");
        assert_eq!(BackgroundSize::Contain.to_css_string(), "contain");
        let explicit = BackgroundSize::Explicit(Length::Px(100.0).into(), Percentage(50.0).into());
        assert_eq!(explicit.to_css_string(), "100px 50%");
        assert_eq!(
            BackgroundSize::Explicit(BackgroundSizeAxis::Auto, Length::Px(40.0).into())
                .to_css_string(),
            "auto 40px"
        );
    }
}
