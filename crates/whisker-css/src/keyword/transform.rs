//! Transform-related keyword enums.
//!
//! References:
//! - <https://lynxjs.org/api/css/properties/transform-box>
//! - <https://lynxjs.org/api/css/properties/transform-style>
//! - <https://lynxjs.org/api/css/properties/backface-visibility>

use core::fmt;

use crate::to_css::ToCss;

/// The `transform-box` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransformBox {
    /// `content-box` — reference box is the content box.
    ContentBox,
    /// `border-box` — reference box is the border box.
    BorderBox,
    /// `fill-box` — reference box is the object's bounding box.
    FillBox,
    /// `stroke-box` — reference box is the stroke bounding box.
    StrokeBox,
    /// `view-box` — reference box is the viewport.
    ViewBox,
}

impl ToCss for TransformBox {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            TransformBox::ContentBox => "content-box",
            TransformBox::BorderBox => "border-box",
            TransformBox::FillBox => "fill-box",
            TransformBox::StrokeBox => "stroke-box",
            TransformBox::ViewBox => "view-box",
        })
    }
}

/// The `transform-style` keyword. Determines whether children of a
/// 3-D-transformed element live in the same 3-D rendering context.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransformStyle {
    /// `flat` — children are flattened into the element's plane. Default.
    Flat,
    /// `preserve-3d` — children live in 3-D space.
    Preserve3d,
}

impl ToCss for TransformStyle {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            TransformStyle::Flat => "flat",
            TransformStyle::Preserve3d => "preserve-3d",
        })
    }
}

/// The `backface-visibility` keyword. Controls whether the back face
/// of a 3-D-transformed element is visible when facing the viewer.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackfaceVisibility {
    /// `visible` — back face is rendered. Default.
    Visible,
    /// `hidden` — back face is not rendered.
    Hidden,
}

impl ToCss for BackfaceVisibility {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            BackfaceVisibility::Visible => "visible",
            BackfaceVisibility::Hidden => "hidden",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_box_all() {
        let cases = [
            (TransformBox::ContentBox, "content-box"),
            (TransformBox::BorderBox, "border-box"),
            (TransformBox::FillBox, "fill-box"),
            (TransformBox::StrokeBox, "stroke-box"),
            (TransformBox::ViewBox, "view-box"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn transform_style_all() {
        assert_eq!(TransformStyle::Flat.to_css_string(), "flat");
        assert_eq!(TransformStyle::Preserve3d.to_css_string(), "preserve-3d");
    }

    #[test]
    fn backface_visibility_all() {
        assert_eq!(BackfaceVisibility::Visible.to_css_string(), "visible");
        assert_eq!(BackfaceVisibility::Hidden.to_css_string(), "hidden");
    }
}
