//! Transform-related properties.

use crate::css::Css;
use crate::data_type::Length;
use crate::data_type_ext::Position;
use crate::{OffsetDistance, OffsetPath, OffsetRotate};

impl Css {
    /// Sets `transform-origin`.
    /// <https://lynxjs.org/api/css/properties/transform-origin>
    pub fn transform_origin(self, v: Position) -> Self {
        self.push_typed(crate::StyleProperty::TransformOrigin, v)
    }

    /// Sets `perspective` — distance from the viewer to the z=0 plane.
    /// <https://lynxjs.org/api/css/properties/perspective>
    pub fn perspective(self, v: Length) -> Self {
        self.push_typed(crate::StyleProperty::Perspective, v)
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
    use crate::{MotionPathCommand, MotionPathPoint, OffsetPath, OffsetRotate};

    #[test]
    fn transform_origin_keywords() {
        let s = Css::new().transform_origin(Position::Keyword(PositionKeyword::Center));
        assert_eq!(s.to_string(), "transform-origin: center;");
    }

    #[test]
    fn perspective_props() {
        let s = Css::new().perspective(500.px());
        assert_eq!(s.to_string(), "perspective: 500px;");
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
                MotionPathCommand::ArcTo {
                    radius_x: 25.0,
                    radius_y: 10.0,
                    x_axis_rotation: 30.0,
                    large_arc: true,
                    sweep: false,
                    to: MotionPathPoint::new(100.0, 20.0),
                },
                MotionPathCommand::Close,
            ]))
            .offset_distance(75.0.percent())
            .offset_rotate(OffsetRotate::Auto);
        assert_eq!(
            style.to_string(),
            "offset-path: path(\"M 0 0 L 40 0 Q 50 10 60 0 C 70 -10 80 10 90 0 A 25 10 30 1 0 100 20 Z\"); offset-distance: 75%; offset-rotate: auto;"
        );
        let _ = style.to_specified_style();

        let fixed = Css::new()
            .offset_path(OffsetPath::None)
            .offset_distance(crate::Number::new(0.5))
            .offset_rotate(OffsetRotate::Angle(45.0.deg()));
        assert_eq!(
            fixed.to_string(),
            "offset-path: none; offset-distance: 0.5; offset-rotate: 45deg;"
        );
        let _ = fixed.to_specified_style();

        let circle = Css::new().offset_path(OffsetPath::circle_at(
            25.0.percent(),
            10.0.px(),
            75.0.percent(),
        ));
        assert_eq!(circle.to_string(), "offset-path: circle(25% at 10px 75%);");
        let _ = circle.to_specified_style();

        let ellipse = Css::new().offset_path(OffsetPath::ellipse(10.0.px(), 25.0.percent()));
        assert_eq!(
            ellipse.to_string(),
            "offset-path: ellipse(10px 25% at 50% 50%);"
        );
        let _ = ellipse.to_specified_style();

        let inset = Css::new().offset_path(OffsetPath::inset_round(
            10.0.px(),
            20.0.percent(),
            5.0.px(),
            15.0.percent(),
            crate::BorderRadius::elliptical(
                [
                    2.0.px().into(),
                    4.0.px().into(),
                    6.0.px().into(),
                    8.0.px().into(),
                ],
                [
                    1.0.px().into(),
                    3.0.px().into(),
                    5.0.px().into(),
                    7.0.px().into(),
                ],
            ),
        ));
        assert_eq!(
            inset.to_string(),
            "offset-path: inset(10px 20% 5px 15% round 2px 4px 6px 8px / 1px 3px 5px 7px);"
        );
        let _ = inset.to_specified_style();
    }
}
