//! Background longhand properties.

use crate::css::Css;
use crate::data_type::{Color, LengthPercentage};
use crate::data_type_ext::Position;
use crate::keyword::{
    BackgroundAttachment, BackgroundClip, BackgroundOrigin, BackgroundRepeat, BackgroundSize,
};
use crate::value::ImageRef;

impl Css {
    /// Sets `background-color`. Lynx default: `transparent`.
    /// <https://lynxjs.org/api/css/properties/background-color>
    pub fn background_color(self, v: Color) -> Self {
        self.push("background-color", v)
    }

    /// Sets `background-image`. Accepts `url(...)` and `<gradient>`.
    /// `none` clears any existing image.
    /// <https://lynxjs.org/api/css/properties/background-image>
    pub fn background_image(self, v: impl Into<ImageRef>) -> Self {
        self.push("background-image", v.into())
    }

    /// Sets `background-repeat`.
    /// <https://lynxjs.org/api/css/properties/background-repeat>
    pub fn background_repeat(self, v: BackgroundRepeat) -> Self {
        self.push("background-repeat", v)
    }

    /// Sets `background-position`.
    /// <https://lynxjs.org/api/css/properties/background-position>
    pub fn background_position(self, v: Position) -> Self {
        self.push("background-position", v)
    }

    /// Sets `background-position-x` — horizontal component only.
    /// <https://lynxjs.org/api/css/properties/background-position-x>
    pub fn background_position_x(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("background-position-x", v.into())
    }

    /// Sets `background-position-y` — vertical component only.
    /// <https://lynxjs.org/api/css/properties/background-position-y>
    pub fn background_position_y(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("background-position-y", v.into())
    }

    /// Sets `background-size`.
    /// <https://lynxjs.org/api/css/properties/background-size>
    pub fn background_size(self, v: BackgroundSize) -> Self {
        self.push("background-size", v)
    }

    /// Sets `background-origin`. Lynx default: `padding-box`.
    /// <https://lynxjs.org/api/css/properties/background-origin>
    pub fn background_origin(self, v: BackgroundOrigin) -> Self {
        self.push("background-origin", v)
    }

    /// Sets `background-clip`. Lynx default: `border-box`.
    /// <https://lynxjs.org/api/css/properties/background-clip>
    pub fn background_clip(self, v: BackgroundClip) -> Self {
        self.push("background-clip", v)
    }

    /// Sets `background-attachment`. Lynx default: `scroll`.
    /// <https://lynxjs.org/api/css/properties/background-attachment>
    pub fn background_attachment(self, v: BackgroundAttachment) -> Self {
        self.push("background-attachment", v)
    }

    /// Sets `color` — the foreground color used by text and SVG strokes.
    /// <https://lynxjs.org/api/css/properties/color>
    pub fn color(self, v: Color) -> Self {
        self.push("color", v)
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type::{Color, CssString, Gradient, NamedColor};
    use crate::data_type::{ColorStop, Percentage};
    use crate::data_type_ext::{Position, PositionKeyword};
    use crate::ext::*;
    use crate::keyword::*;
    use crate::value::ImageRef;

    #[test]
    fn background_color() {
        let s = Css::new().background_color(Color::Named(NamedColor::Black));
        assert_eq!(s.to_string(), "background-color: black;");
    }

    #[test]
    fn foreground_color() {
        let s = Css::new().color(Color::Named(NamedColor::White));
        assert_eq!(s.to_string(), "color: white;");
    }

    #[test]
    fn background_image_url() {
        let s = Css::new().background_image(ImageRef::Url(CssString::new("a.png")));
        assert_eq!(s.to_string(), "background-image: url(\"a.png\");");
    }

    #[test]
    fn background_image_gradient() {
        let g = Gradient::linear_to_bottom([
            ColorStop::new(NamedColor::Red.into()),
            ColorStop::new(NamedColor::Blue.into()),
        ]);
        let s = Css::new().background_image(g);
        assert_eq!(
            s.to_string(),
            "background-image: linear-gradient(to bottom, red, blue);"
        );
    }

    #[test]
    fn background_image_none() {
        let s = Css::new().background_image(ImageRef::None);
        assert_eq!(s.to_string(), "background-image: none;");
    }

    #[test]
    fn background_repeat_and_position() {
        let s = Css::new()
            .background_repeat(BackgroundRepeat::NoRepeat)
            .background_position(Position::Keywords(
                PositionKeyword::Center,
                PositionKeyword::Top,
            ));
        assert_eq!(
            s.to_string(),
            "background-repeat: no-repeat; background-position: center top;"
        );
    }

    #[test]
    fn background_position_axis() {
        let s = Css::new()
            .background_position_x(px(10))
            .background_position_y(Percentage(50.0));
        assert_eq!(
            s.to_string(),
            "background-position-x: 10px; background-position-y: 50%;"
        );
    }

    #[test]
    fn background_size_keywords() {
        let s = Css::new().background_size(BackgroundSize::Cover);
        assert_eq!(s.to_string(), "background-size: cover;");
        let s = Css::new().background_size(BackgroundSize::Contain);
        assert_eq!(s.to_string(), "background-size: contain;");
        let s = Css::new().background_size(BackgroundSize::Auto);
        assert_eq!(s.to_string(), "background-size: auto;");
    }

    #[test]
    fn background_origin_and_clip() {
        let s = Css::new()
            .background_origin(BackgroundOrigin::ContentBox)
            .background_clip(BackgroundClip::Text);
        assert_eq!(
            s.to_string(),
            "background-origin: content-box; background-clip: text;"
        );
    }

    #[test]
    fn background_attachment() {
        let s = Css::new().background_attachment(BackgroundAttachment::Fixed);
        assert_eq!(s.to_string(), "background-attachment: fixed;");
    }
}
