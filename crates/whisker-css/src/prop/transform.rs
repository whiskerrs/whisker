//! Transform-related properties.

use crate::css::Css;
use crate::data_type::Length;
use crate::data_type_ext::Position;
use crate::keyword::{BackfaceVisibility, TransformBox, TransformStyle};
use crate::{OffsetDistance, OffsetPath, OffsetRotate};

impl Css {
    /// Sets `transform-origin`.
    /// <https://lynxjs.org/api/css/properties/transform-origin>
    pub fn transform_origin(self, v: Position) -> Self {
        self.push_typed(crate::StyleProperty::TransformOrigin, v)
    }

    /// Sets `transform-box`.
    /// <https://lynxjs.org/api/css/properties/transform-box>
    pub fn transform_box(self, v: TransformBox) -> Self {
        self.push(crate::StyleProperty::TransformBox, v)
    }

    /// Sets `transform-style`.
    /// <https://lynxjs.org/api/css/properties/transform-style>
    pub fn transform_style(self, v: TransformStyle) -> Self {
        self.push(crate::StyleProperty::TransformStyle, v)
    }

    /// Sets `backface-visibility`.
    /// <https://lynxjs.org/api/css/properties/backface-visibility>
    pub fn backface_visibility(self, v: BackfaceVisibility) -> Self {
        self.push(crate::StyleProperty::BackfaceVisibility, v)
    }

    /// Sets `perspective` — distance from the viewer to the z=0 plane.
    /// <https://lynxjs.org/api/css/properties/perspective>
    pub fn perspective(self, v: Length) -> Self {
        self.push_typed(crate::StyleProperty::Perspective, v)
    }

    /// Sets `perspective-origin`.
    /// <https://lynxjs.org/api/css/properties/perspective-origin>
    pub fn perspective_origin(self, v: Position) -> Self {
        self.push(crate::StyleProperty::PerspectiveOrigin, v)
    }

    /// Sets `offset-path` to a typed polyline path or `none`.
    /// <https://lynxjs.org/api/css/properties/offset-path>
    pub fn offset_path(self, value: OffsetPath) -> Self {
        self.push_typed(crate::StyleProperty::OffsetPath, value)
    }

    /// Sets normalized progress along `offset-path`.
    /// <https://lynxjs.org/api/css/properties/offset-distance>
    pub fn offset_distance(self, value: impl Into<OffsetDistance>) -> Self {
        self.push_typed(crate::StyleProperty::OffsetDistance, value.into())
    }

    /// Sets tangent-following or fixed motion-path rotation.
    /// <https://lynxjs.org/api/css/properties/offset-rotate>
    pub fn offset_rotate(self, value: OffsetRotate) -> Self {
        self.push_typed(crate::StyleProperty::OffsetRotate, value)
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type_ext::{Position, PositionKeyword};
    use crate::ext::*;
    use crate::keyword::*;
    use crate::{MotionPathCommand, MotionPathPoint, OffsetPath, OffsetRotate};

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

    #[test]
    fn motion_path_props_keep_css_and_semantic_values_together() {
        let style = Css::new()
            .offset_path(OffsetPath::path(vec![
                MotionPathCommand::MoveTo(MotionPathPoint::new(0.0, 0.0)),
                MotionPathCommand::LineTo(MotionPathPoint::new(40.0, 0.0)),
                MotionPathCommand::QuadraticTo {
                    control: MotionPathPoint::new(50.0, 10.0),
                    to: MotionPathPoint::new(60.0, 0.0),
                },
                MotionPathCommand::CubicTo {
                    control1: MotionPathPoint::new(70.0, -10.0),
                    control2: MotionPathPoint::new(80.0, 10.0),
                    to: MotionPathPoint::new(90.0, 0.0),
                },
                MotionPathCommand::Close,
            ]))
            .offset_distance(75.0.percent())
            .offset_rotate(OffsetRotate::Auto);
        assert_eq!(
            style.to_string(),
            "offset-path: path(\"M 0 0 L 40 0 Q 50 10 60 0 C 70 -10 80 10 90 0 Z\"); offset-distance: 75%; offset-rotate: auto;"
        );
        assert!(style.to_specified_style().is_ok());

        let fixed = Css::new()
            .offset_path(OffsetPath::None)
            .offset_distance(crate::Number::new(0.5))
            .offset_rotate(OffsetRotate::Angle(45.0.deg()));
        assert_eq!(
            fixed.to_string(),
            "offset-path: none; offset-distance: 0.5; offset-rotate: 45deg;"
        );
        assert!(fixed.to_specified_style().is_ok());

        let circle = Css::new().offset_path(OffsetPath::circle_at(
            25.0.percent(),
            10.0.px(),
            75.0.percent(),
        ));
        assert_eq!(circle.to_string(), "offset-path: circle(25% at 10px 75%);");
        assert!(circle.to_specified_style().is_ok());

        let ellipse = Css::new().offset_path(OffsetPath::ellipse(10.0.px(), 25.0.percent()));
        assert_eq!(
            ellipse.to_string(),
            "offset-path: ellipse(10px 25% at 50% 50%);"
        );
        assert!(ellipse.to_specified_style().is_ok());
    }
}
