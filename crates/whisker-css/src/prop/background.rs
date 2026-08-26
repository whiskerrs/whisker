//! Background longhand properties.

use crate::ToCss;
use crate::css::Css;
use crate::data_type::{
    Color, Gradient, LengthPercentage, LinearDirection, RadialShape, StopPosition,
};
use crate::data_type_ext::Position;
use crate::keyword::{
    BackgroundAttachment, BackgroundClip, BackgroundOrigin, BackgroundRepeat, BackgroundSize,
    BackgroundSizeAxis,
};
use crate::style_value::ToStyleValue;
use crate::value::ImageRef;

impl Css {
    /// Sets `background-color`. Lynx default: `transparent`.
    /// <https://lynxjs.org/api/css/properties/background-color>
    pub fn background_color(self, v: Color) -> Self {
        self.push_typed(crate::StyleProperty::BackgroundColor, v)
    }

    /// Sets `background-image`. Accepts `url(...)` and `<gradient>`.
    /// `none` clears any existing image.
    /// <https://lynxjs.org/api/css/properties/background-image>
    pub fn background_image(self, v: impl Into<ImageRef>) -> Self {
        let image = v.into();
        let lynx_value = image.to_css_string();
        self.push_semantic(
            crate::StyleProperty::BackgroundImage,
            whisker_style::StyleValue::BackgroundImages(vec![background_image_value(&image)]),
            lynx_value,
        )
    }

    /// Sets `background-repeat`.
    /// <https://lynxjs.org/api/css/properties/background-repeat>
    pub fn background_repeat(self, v: BackgroundRepeat) -> Self {
        self.push_semantic(
            crate::StyleProperty::BackgroundRepeat,
            whisker_style::StyleValue::BackgroundRepeat(background_repeat_value(v)),
            v.to_css_string(),
        )
    }

    /// Sets `background-position`.
    /// <https://lynxjs.org/api/css/properties/background-position>
    pub fn background_position(self, v: Position) -> Self {
        let lynx_value = v.to_css_string();
        let Some(value) = background_position_value(v) else {
            return self.push_raw(crate::StyleProperty::BackgroundPosition, lynx_value);
        };
        self.push_semantic(
            crate::StyleProperty::BackgroundPosition,
            whisker_style::StyleValue::BackgroundPosition(value),
            lynx_value,
        )
    }

    /// Sets `background-position-x` — horizontal component only.
    /// <https://lynxjs.org/api/css/properties/background-position-x>
    pub fn background_position_x(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BackgroundPositionX, v.into())
    }

    /// Sets `background-position-y` — vertical component only.
    /// <https://lynxjs.org/api/css/properties/background-position-y>
    pub fn background_position_y(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::BackgroundPositionY, v.into())
    }

    /// Sets `background-size`.
    /// <https://lynxjs.org/api/css/properties/background-size>
    pub fn background_size(self, v: BackgroundSize) -> Self {
        let lynx_value = v.to_css_string();
        self.push_semantic(
            crate::StyleProperty::BackgroundSize,
            whisker_style::StyleValue::BackgroundSize(background_size_value(v)),
            lynx_value,
        )
    }

    /// Sets `background-origin`. Lynx default: `padding-box`.
    /// <https://lynxjs.org/api/css/properties/background-origin>
    pub fn background_origin(self, v: BackgroundOrigin) -> Self {
        let value = match v {
            BackgroundOrigin::BorderBox => whisker_style::BackgroundBoxValue::Border,
            BackgroundOrigin::PaddingBox => whisker_style::BackgroundBoxValue::Padding,
            BackgroundOrigin::ContentBox => whisker_style::BackgroundBoxValue::Content,
        };
        self.push_semantic(
            crate::StyleProperty::BackgroundOrigin,
            whisker_style::StyleValue::BackgroundBox(value),
            v.to_css_string(),
        )
    }

    /// Sets `background-clip`. Lynx default: `border-box`.
    /// <https://lynxjs.org/api/css/properties/background-clip>
    pub fn background_clip(self, v: BackgroundClip) -> Self {
        let lynx_value = v.to_css_string();
        let value = match v {
            BackgroundClip::BorderBox => whisker_style::BackgroundBoxValue::Border,
            BackgroundClip::PaddingBox => whisker_style::BackgroundBoxValue::Padding,
            BackgroundClip::ContentBox => whisker_style::BackgroundBoxValue::Content,
            BackgroundClip::BorderArea => whisker_style::BackgroundBoxValue::BorderArea,
        };
        self.push_semantic(
            crate::StyleProperty::BackgroundClip,
            whisker_style::StyleValue::BackgroundBox(value),
            lynx_value,
        )
    }

    /// Sets `background-attachment`. Lynx default: `scroll`.
    /// <https://lynxjs.org/api/css/properties/background-attachment>
    pub fn background_attachment(self, v: BackgroundAttachment) -> Self {
        let lynx_value = v.to_css_string();
        match v {
            BackgroundAttachment::Scroll => self.push_semantic(
                crate::StyleProperty::BackgroundAttachment,
                whisker_style::StyleValue::BackgroundAttachment(
                    whisker_style::BackgroundAttachmentValue::Scroll,
                ),
                lynx_value,
            ),
            BackgroundAttachment::Fixed | BackgroundAttachment::Local => {
                self.push_raw(crate::StyleProperty::BackgroundAttachment, lynx_value)
            }
        }
    }

    /// Sets `color` — the foreground color used by text and SVG strokes.
    /// <https://lynxjs.org/api/css/properties/color>
    pub fn color(self, v: Color) -> Self {
        self.push_typed(crate::StyleProperty::Color, v)
    }
}

pub(crate) fn background_image_value(value: &ImageRef) -> whisker_style::BackgroundImageValue {
    match value {
        ImageRef::None => whisker_style::BackgroundImageValue::None,
        ImageRef::Url(value) => whisker_style::BackgroundImageValue::Url(value.0.clone()),
        ImageRef::Gradient(value) => {
            whisker_style::BackgroundImageValue::Gradient(gradient_value(value))
        }
    }
}

fn gradient_value(value: &Gradient) -> whisker_style::GradientValue {
    use whisker_style::{
        BackgroundPositionValue, GradientStopValue, GradientValue, RadialGradientValue,
        StyleNumber, StyleValue,
    };

    let stops = |values: &[crate::ColorStop]| {
        values
            .iter()
            .map(|stop| {
                let StyleValue::Color(color) = stop.color.to_style_value() else {
                    unreachable!("Color always has a semantic style value")
                };
                GradientStopValue {
                    color,
                    position: stop.position.as_ref().map(|position| match position {
                        StopPosition::LengthPercentage(value) => length_percentage_value(value),
                        StopPosition::Number(value) => {
                            whisker_style::LengthPercentageValue::Percentage(StyleNumber::new(
                                *value * 100.0,
                            ))
                        }
                    }),
                }
            })
            .collect()
    };
    match value {
        Gradient::Linear {
            direction,
            stops: values,
        } => GradientValue::Linear {
            angle_degrees: StyleNumber::new(match direction {
                LinearDirection::ToTop => 0.0,
                LinearDirection::ToTopRight => 45.0,
                LinearDirection::ToRight => 90.0,
                LinearDirection::ToBottomRight => 135.0,
                LinearDirection::ToBottom => 180.0,
                LinearDirection::ToBottomLeft => 225.0,
                LinearDirection::ToLeft => 270.0,
                LinearDirection::ToTopLeft => 315.0,
                LinearDirection::Angle(value) => angle_degrees(*value),
            }),
            stops: stops(values),
        },
        Gradient::Radial {
            shape,
            stops: values,
        } => GradientValue::Radial {
            shape: match shape {
                RadialShape::Circle => RadialGradientValue::Circle,
                RadialShape::Ellipse => RadialGradientValue::Ellipse,
                RadialShape::CircleSized(radius) => {
                    RadialGradientValue::CircleSized(length_percentage_value(radius))
                }
                RadialShape::EllipseSized(x, y) => RadialGradientValue::EllipseSized(
                    length_percentage_value(x),
                    length_percentage_value(y),
                ),
            },
            stops: stops(values),
        },
        Gradient::Conic {
            from,
            at,
            stops: values,
        } => GradientValue::Conic {
            from_degrees: StyleNumber::new(from.map_or(0.0, angle_degrees)),
            center: at.as_ref().map_or_else(
                || BackgroundPositionValue {
                    horizontal: percentage(50.0),
                    vertical: percentage(50.0),
                },
                |(x, y)| BackgroundPositionValue {
                    horizontal: length_percentage_value(x),
                    vertical: length_percentage_value(y),
                },
            ),
            stops: stops(values),
        },
    }
}

fn angle_degrees(value: crate::Angle) -> f32 {
    match value {
        crate::Angle::Deg(value) => value,
        crate::Angle::Rad(value) => value.to_degrees(),
        crate::Angle::Turn(value) => value * 360.0,
    }
}

pub(crate) fn background_repeat_value(
    value: BackgroundRepeat,
) -> whisker_style::BackgroundRepeatValue {
    use whisker_style::{BackgroundRepeatModeValue as Mode, BackgroundRepeatValue};

    let (horizontal, vertical) = match value {
        BackgroundRepeat::Repeat => (Mode::Repeat, Mode::Repeat),
        BackgroundRepeat::NoRepeat => (Mode::NoRepeat, Mode::NoRepeat),
        BackgroundRepeat::RepeatX => (Mode::Repeat, Mode::NoRepeat),
        BackgroundRepeat::RepeatY => (Mode::NoRepeat, Mode::Repeat),
        BackgroundRepeat::Space => (Mode::Space, Mode::Space),
        BackgroundRepeat::Round => (Mode::Round, Mode::Round),
    };
    BackgroundRepeatValue {
        horizontal,
        vertical,
    }
}

pub(crate) fn background_size_value(value: BackgroundSize) -> whisker_style::BackgroundSizeValue {
    match value {
        BackgroundSize::Auto => whisker_style::BackgroundSizeValue::Auto,
        BackgroundSize::Explicit(width, height) => whisker_style::BackgroundSizeValue::Explicit {
            width: background_size_axis(width),
            height: background_size_axis(height),
        },
        BackgroundSize::Cover => whisker_style::BackgroundSizeValue::Cover,
        BackgroundSize::Contain => whisker_style::BackgroundSizeValue::Contain,
    }
}

fn background_size_axis(value: BackgroundSizeAxis) -> Option<whisker_style::LengthPercentageValue> {
    match value {
        BackgroundSizeAxis::Auto => None,
        BackgroundSizeAxis::Value(value) => Some(length_percentage_value(&value)),
    }
}

fn length_percentage_value(value: &LengthPercentage) -> whisker_style::LengthPercentageValue {
    let whisker_style::StyleValue::LengthPercentage(value) = value.to_style_value() else {
        unreachable!("LengthPercentage always has a semantic style value")
    };
    value
}

fn percentage(value: f32) -> whisker_style::LengthPercentageValue {
    whisker_style::LengthPercentageValue::Percentage(whisker_style::StyleNumber::new(value))
}

fn keyword_value(
    keyword: crate::data_type_ext::PositionKeyword,
) -> whisker_style::LengthPercentageValue {
    use crate::data_type_ext::PositionKeyword;

    percentage(match keyword {
        PositionKeyword::Left | PositionKeyword::Top => 0.0,
        PositionKeyword::Center => 50.0,
        PositionKeyword::Right | PositionKeyword::Bottom => 100.0,
    })
}

pub(crate) fn background_position_value(
    position: Position,
) -> Option<whisker_style::BackgroundPositionValue> {
    use crate::data_type_ext::PositionKeyword;

    let center = || keyword_value(PositionKeyword::Center);
    let (horizontal, vertical) = match position {
        Position::Keyword(keyword @ (PositionKeyword::Left | PositionKeyword::Right)) => {
            (keyword_value(keyword), center())
        }
        Position::Keyword(keyword @ (PositionKeyword::Top | PositionKeyword::Bottom)) => {
            (center(), keyword_value(keyword))
        }
        Position::Keyword(PositionKeyword::Center) => (center(), center()),
        Position::Coords(horizontal, vertical) => (
            length_percentage_value(&horizontal),
            length_percentage_value(&vertical),
        ),
        Position::Mixed(keyword @ (PositionKeyword::Left | PositionKeyword::Right), offset) => {
            (keyword_value(keyword), length_percentage_value(&offset))
        }
        Position::Mixed(keyword @ (PositionKeyword::Top | PositionKeyword::Bottom), offset) => {
            (length_percentage_value(&offset), keyword_value(keyword))
        }
        Position::Mixed(PositionKeyword::Center, offset) => {
            (center(), length_percentage_value(&offset))
        }
        Position::Keywords(first, second) => {
            let horizontal =
                |keyword| matches!(keyword, PositionKeyword::Left | PositionKeyword::Right);
            let vertical =
                |keyword| matches!(keyword, PositionKeyword::Top | PositionKeyword::Bottom);
            match (first, second) {
                (PositionKeyword::Center, PositionKeyword::Center) => (center(), center()),
                (PositionKeyword::Center, value) if vertical(value) => {
                    (center(), keyword_value(value))
                }
                (PositionKeyword::Center, value) if horizontal(value) => {
                    (keyword_value(value), center())
                }
                (value, PositionKeyword::Center) if horizontal(value) => {
                    (keyword_value(value), center())
                }
                (value, PositionKeyword::Center) if vertical(value) => {
                    (center(), keyword_value(value))
                }
                (x, y) if horizontal(x) && vertical(y) => (keyword_value(x), keyword_value(y)),
                (y, x) if vertical(y) && horizontal(x) => (keyword_value(x), keyword_value(y)),
                _ => return None,
            }
        }
    };
    Some(whisker_style::BackgroundPositionValue {
        horizontal,
        vertical,
    })
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
        assert_eq!(
            s.to_specified_style().unwrap().resolved()[0].value(),
            &whisker_style::StyleValue::BackgroundImages(vec![
                whisker_style::BackgroundImageValue::Url("a.png".into()),
            ])
        );
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
        assert!(s.to_specified_style().is_ok());
    }

    #[test]
    fn background_radial_and_conic_gradients_are_typed() {
        let stops = || {
            vec![
                ColorStop::at(NamedColor::Red.into(), Percentage(0.0)),
                ColorStop::at(NamedColor::Blue.into(), Percentage(100.0)),
            ]
        };
        let radial = Gradient::Radial {
            shape: crate::RadialShape::EllipseSized(
                crate::Length::Px(40.0).into(),
                Percentage(25.0).into(),
            ),
            stops: stops(),
        };
        let conic = Gradient::Conic {
            from: Some(crate::Angle::Turn(0.25)),
            at: Some((Percentage(25.0).into(), Percentage(75.0).into())),
            stops: stops(),
        };
        assert!(
            Css::new()
                .background_image(radial)
                .to_specified_style()
                .is_ok()
        );
        assert!(
            Css::new()
                .background_image(conic)
                .to_specified_style()
                .is_ok()
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
        let specified = s.to_specified_style().unwrap();
        assert_eq!(
            specified.resolved()[0].value(),
            &whisker_style::StyleValue::BackgroundRepeat(whisker_style::BackgroundRepeatValue {
                horizontal: whisker_style::BackgroundRepeatModeValue::NoRepeat,
                vertical: whisker_style::BackgroundRepeatModeValue::NoRepeat,
            })
        );
        assert_eq!(
            specified.resolved()[1].value(),
            &whisker_style::StyleValue::BackgroundPosition(
                whisker_style::BackgroundPositionValue {
                    horizontal: whisker_style::LengthPercentageValue::Percentage(
                        whisker_style::StyleNumber::new(50.0),
                    ),
                    vertical: whisker_style::LengthPercentageValue::Percentage(
                        whisker_style::StyleNumber::new(0.0),
                    ),
                }
            )
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
        assert_eq!(
            s.to_specified_style().unwrap().resolved()[0].value(),
            &whisker_style::StyleValue::BackgroundSize(whisker_style::BackgroundSizeValue::Auto)
        );

        let explicit = Css::new().background_size(BackgroundSize::Explicit(
            px(20).into(),
            Percentage(50.0).into(),
        ));
        assert!(matches!(
            explicit.to_specified_style().unwrap().resolved()[0].value(),
            whisker_style::StyleValue::BackgroundSize(
                whisker_style::BackgroundSizeValue::Explicit { .. }
            )
        ));

        assert!(matches!(
            Css::new()
                .background_size(BackgroundSize::Cover)
                .to_specified_style()
                .unwrap()
                .resolved()[0]
                .value(),
            whisker_style::StyleValue::BackgroundSize(whisker_style::BackgroundSizeValue::Cover)
        ));
        assert_eq!(
            Css::new()
                .background_size(BackgroundSize::Explicit(
                    BackgroundSizeAxis::Auto,
                    px(30).into(),
                ))
                .to_string(),
            "background-size: auto 30px;"
        );
    }

    #[test]
    fn background_origin_and_clip() {
        let s = Css::new()
            .background_origin(BackgroundOrigin::ContentBox)
            .background_clip(BackgroundClip::BorderArea);
        assert_eq!(
            s.to_string(),
            "background-origin: content-box; background-clip: border-area;"
        );
        assert_eq!(
            s.to_specified_style().unwrap().resolved()[1].value(),
            &whisker_style::StyleValue::BackgroundBox(
                whisker_style::BackgroundBoxValue::BorderArea
            )
        );

        let supported = Css::new()
            .background_origin(BackgroundOrigin::ContentBox)
            .background_clip(BackgroundClip::PaddingBox)
            .to_specified_style()
            .unwrap();
        assert_eq!(
            supported.resolved()[0].value(),
            &whisker_style::StyleValue::BackgroundBox(whisker_style::BackgroundBoxValue::Content)
        );
        assert_eq!(
            supported.resolved()[1].value(),
            &whisker_style::StyleValue::BackgroundBox(whisker_style::BackgroundBoxValue::Padding)
        );
    }

    #[test]
    fn background_attachment() {
        let s = Css::new().background_attachment(BackgroundAttachment::Fixed);
        assert_eq!(s.to_string(), "background-attachment: fixed;");
        assert_eq!(
            s.to_specified_style().unwrap_err().property(),
            "background-attachment"
        );
        assert_eq!(
            Css::new()
                .background_attachment(BackgroundAttachment::Scroll)
                .to_specified_style()
                .unwrap()
                .resolved()[0]
                .value(),
            &whisker_style::StyleValue::BackgroundAttachment(
                whisker_style::BackgroundAttachmentValue::Scroll
            )
        );
    }
}
