//! Transform-related properties.

use crate::css::Css;
use crate::data_type::Length;
use crate::data_type_ext::Position;
use crate::keyword::{BackfaceVisibility, TransformBox, TransformStyle};

impl Css {
    /// Sets `transform-origin`.
    /// <https://lynxjs.org/api/css/properties/transform-origin>
    pub fn transform_origin(self, v: Position) -> Self {
        self.push("transform-origin", v)
    }

    /// Sets `transform-box`.
    /// <https://lynxjs.org/api/css/properties/transform-box>
    pub fn transform_box(self, v: TransformBox) -> Self {
        self.push("transform-box", v)
    }

    /// Sets `transform-style`.
    /// <https://lynxjs.org/api/css/properties/transform-style>
    pub fn transform_style(self, v: TransformStyle) -> Self {
        self.push("transform-style", v)
    }

    /// Sets `backface-visibility`.
    /// <https://lynxjs.org/api/css/properties/backface-visibility>
    pub fn backface_visibility(self, v: BackfaceVisibility) -> Self {
        self.push("backface-visibility", v)
    }

    /// Sets `perspective` — distance from the viewer to the z=0 plane.
    /// <https://lynxjs.org/api/css/properties/perspective>
    pub fn perspective(self, v: Length) -> Self {
        self.push("perspective", v)
    }

    /// Sets `perspective-origin`.
    /// <https://lynxjs.org/api/css/properties/perspective-origin>
    pub fn perspective_origin(self, v: Position) -> Self {
        self.push("perspective-origin", v)
    }
}

#[cfg(test)]
mod tests {
    use crate::data_type_ext::{Position, PositionKeyword};
    use crate::ext::*;
    use crate::keyword::*;
    use crate::Css;

    #[test]
    fn transform_origin_keywords() {
        let s = Css::new().transform_origin(Position::Keyword(PositionKeyword::Center));
        assert_eq!(s.to_string(), "transform-origin: center;");
    }

    #[test]
    fn transform_box_styles() {
        let s = Css::new()
            .transform_box(TransformBox::BorderBox)
            .transform_style(TransformStyle::Preserve3d)
            .backface_visibility(BackfaceVisibility::Hidden);
        assert_eq!(
            s.to_string(),
            "transform-box: border-box; transform-style: preserve-3d; backface-visibility: hidden;"
        );
    }

    #[test]
    fn perspective_props() {
        let s = Css::new()
            .perspective(500.px())
            .perspective_origin(Position::Keyword(PositionKeyword::Center));
        assert_eq!(
            s.to_string(),
            "perspective: 500px; perspective-origin: center;"
        );
    }
}
