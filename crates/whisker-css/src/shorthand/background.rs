//! `background` shorthand — multi-layer plus a trailing color.

use core::fmt;

use crate::css::Css;
use crate::data_type::Color;
use crate::data_type_ext::Position;
use crate::keyword::{
    BackgroundAttachment, BackgroundClip, BackgroundOrigin, BackgroundRepeat, BackgroundSize,
};
use crate::to_css::ToCss;
use crate::value::ImageRef;

/// One background layer. Only `image` is required; other fields
/// default to omitted in the serialized form.
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundLayer {
    /// Image (`url(...)` / `<gradient>` / `none`).
    pub image: ImageRef,
    /// Position.
    pub position: Option<Position>,
    /// Size — emitted as `/ <size>` after `position`.
    pub size: Option<BackgroundSize>,
    /// Repeat.
    pub repeat: Option<BackgroundRepeat>,
    /// Attachment.
    pub attachment: Option<BackgroundAttachment>,
    /// Origin.
    pub origin: Option<BackgroundOrigin>,
    /// Clip.
    pub clip: Option<BackgroundClip>,
}

impl BackgroundLayer {
    /// Start with an image (or [`ImageRef::None`]).
    pub fn new(image: impl Into<ImageRef>) -> Self {
        Self {
            image: image.into(),
            position: None,
            size: None,
            repeat: None,
            attachment: None,
            origin: None,
            clip: None,
        }
    }

    /// Set the layer position.
    pub fn position(mut self, p: Position) -> Self {
        self.position = Some(p);
        self
    }

    /// Set the layer size.
    pub fn size(mut self, sz: BackgroundSize) -> Self {
        self.size = Some(sz);
        self
    }

    /// Set the repeat behavior.
    pub fn repeat(mut self, r: BackgroundRepeat) -> Self {
        self.repeat = Some(r);
        self
    }

    /// Set the attachment.
    pub fn attachment(mut self, a: BackgroundAttachment) -> Self {
        self.attachment = Some(a);
        self
    }

    /// Set the origin.
    pub fn origin(mut self, o: BackgroundOrigin) -> Self {
        self.origin = Some(o);
        self
    }

    /// Set the clip.
    pub fn clip(mut self, c: BackgroundClip) -> Self {
        self.clip = Some(c);
        self
    }
}

impl ToCss for BackgroundLayer {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        self.image.to_css(dest)?;
        if let Some(p) = &self.position {
            dest.write_char(' ')?;
            p.to_css(dest)?;
            if let Some(sz) = &self.size {
                dest.write_str(" / ")?;
                sz.to_css(dest)?;
            }
        } else if let Some(sz) = &self.size {
            // `background-size` without an explicit `<position>` —
            // CSS requires at least an implicit `0% 0%`; we emit
            // `0 0` to keep the shorthand parseable.
            dest.write_str(" 0 0 / ")?;
            sz.to_css(dest)?;
        }
        if let Some(r) = &self.repeat {
            dest.write_char(' ')?;
            r.to_css(dest)?;
        }
        if let Some(a) = &self.attachment {
            dest.write_char(' ')?;
            a.to_css(dest)?;
        }
        if let Some(o) = &self.origin {
            dest.write_char(' ')?;
            o.to_css(dest)?;
        }
        if let Some(c) = &self.clip {
            dest.write_char(' ')?;
            c.to_css(dest)?;
        }
        Ok(())
    }
}

/// `background` shorthand value — N image layers + one trailing
/// `background-color`.
///
/// Lynx requires the color, if present, to come last (after all
/// image layers). [`Background`] enforces that by storing it
/// separately from the layer list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Background {
    /// Image layers, listed first-to-last.
    pub layers: Vec<BackgroundLayer>,
    /// Trailing color.
    pub color: Option<Color>,
}

impl Background {
    /// An empty background.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one layer.
    pub fn layer(mut self, l: BackgroundLayer) -> Self {
        self.layers.push(l);
        self
    }

    /// Set the trailing color.
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }
}

impl ToCss for Background {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        let mut wrote = false;
        for layer in &self.layers {
            if wrote {
                dest.write_str(", ")?;
            }
            layer.to_css(dest)?;
            wrote = true;
        }
        if let Some(c) = &self.color {
            if wrote {
                dest.write_char(' ')?;
            }
            c.to_css(dest)?;
        }
        Ok(())
    }
}

impl Css {
    /// Sets the `background` shorthand.
    /// <https://lynxjs.org/api/css/properties/background>
    pub fn background(self, b: Background) -> Self {
        self.push("background", b)
    }
}

#[cfg(test)]
mod tests {
    use crate::data_type::{Color, ColorStop, CssString, Gradient, NamedColor};
    use crate::data_type_ext::{Position, PositionKeyword};
    use crate::keyword::*;
    use crate::value::ImageRef;
    use crate::Css;

    use super::*;

    #[test]
    fn background_color_only() {
        let s = Css::new().background(Background::new().color(Color::Named(NamedColor::Red)));
        assert_eq!(s.to_string(), "background: red;");
    }

    #[test]
    fn background_image_url_with_repeat() {
        let layer = BackgroundLayer::new(ImageRef::Url(CssString::new("a.png")))
            .repeat(BackgroundRepeat::NoRepeat);
        let s = Css::new().background(Background::new().layer(layer));
        assert_eq!(s.to_string(), "background: url(\"a.png\") no-repeat;");
    }

    #[test]
    fn background_gradient_with_color_trailing() {
        let layer = BackgroundLayer::new(Gradient::linear_to_bottom([
            ColorStop::new(NamedColor::Red.into()),
            ColorStop::new(NamedColor::Blue.into()),
        ]));
        let s = Css::new().background(
            Background::new()
                .layer(layer)
                .color(Color::Named(NamedColor::White)),
        );
        assert_eq!(
            s.to_string(),
            "background: linear-gradient(to bottom, red, blue) white;"
        );
    }

    #[test]
    fn background_multiple_layers() {
        let l1 = BackgroundLayer::new(ImageRef::Url(CssString::new("top.png")))
            .repeat(BackgroundRepeat::NoRepeat);
        let l2 = BackgroundLayer::new(ImageRef::Url(CssString::new("base.png")));
        let s = Css::new().background(Background::new().layer(l1).layer(l2));
        assert_eq!(
            s.to_string(),
            "background: url(\"top.png\") no-repeat, url(\"base.png\");"
        );
    }

    #[test]
    fn background_layer_size_with_position() {
        let layer = BackgroundLayer::new(ImageRef::Url(CssString::new("a.png")))
            .position(Position::Keyword(PositionKeyword::Center))
            .size(BackgroundSize::Cover);
        let s = Css::new().background(Background::new().layer(layer));
        assert_eq!(s.to_string(), "background: url(\"a.png\") center / cover;");
    }

    #[test]
    fn background_layer_size_without_position_inserts_zero() {
        let layer = BackgroundLayer::new(ImageRef::Url(CssString::new("a.png")))
            .size(BackgroundSize::Cover);
        let s = Css::new().background(Background::new().layer(layer));
        assert_eq!(s.to_string(), "background: url(\"a.png\") 0 0 / cover;");
    }

    #[test]
    fn background_layer_origin_clip_attachment() {
        let layer = BackgroundLayer::new(ImageRef::None)
            .attachment(BackgroundAttachment::Fixed)
            .origin(BackgroundOrigin::ContentBox)
            .clip(BackgroundClip::PaddingBox);
        let s = Css::new().background(Background::new().layer(layer));
        assert_eq!(
            s.to_string(),
            "background: none fixed content-box padding-box;"
        );
    }
}
