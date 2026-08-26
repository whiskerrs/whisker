//! Conversion from the compatibility authoring types to semantic style values.

use whisker_style::{
    AlignContentValue, AlignItemsValue, AlignSelfValue, BackdropFilterValue, BorderRadiusValue,
    BorderStyleValue, BoxSizingValue, CalcExpression, ClearValue, ColorValue, DirectionValue,
    DisplayValue, FlexBasisValue, FlexDirectionValue, FlexWrapValue, FloatValue, FontStyleValue,
    FontWeightValue, GridAutoFlowValue, GridMaxTrackSizingValue, GridMinTrackSizingValue,
    GridPlacementValue, GridRepetitionCountValue, GridTemplateAreaValue, GridTemplateAreasValue,
    GridTemplateComponentValue, GridTemplateRepetitionValue, GridTemplateValue,
    GridTrackSizingValue, ImageRenderingValue, InsetPathValue, JustifyContentValue,
    LengthPercentageAutoValue, LengthPercentageValue, LengthUnit, LengthValue, LineHeightValue,
    MotionPathCommandValue, MotionPathPointValue, OffsetPathValue, OffsetRotateValue,
    PositionValue, SizeValue, StyleNumber, StyleValue, TransformFunctionValue,
    TransformOriginValue, TransformValue,
};

use crate::data_type_ext::PositionKeyword;
use crate::{
    AlignContent, AlignItems, AlignSelf, Angle, BackdropFilter, BoxSizing, CalcExpr, Clear, Color,
    CssString, Direction, Display, FlexBasis, FlexDirection, FlexWrap, Float, FontStyle,
    FontWeight, GridAutoFlow, GridLine, GridRepeatCount, GridTemplate, GridTemplateAreas,
    GridTemplateComponent, GridTrack, GridTrackMax, GridTrackMin, ImageRendering, Integer,
    JustifyContent, Length, LengthPercentage, LineHeight, MarginValue, MotionPathCommand, Number,
    OffsetDistance, OffsetPath, OffsetRotate, Overflow, Percentage, Position, PositionKind, Size,
    Transform, TransformFn, Visibility,
};
use whisker_style::{OverflowValue, VisibilityValue};

pub(crate) trait ToStyleValue {
    fn to_style_value(&self) -> StyleValue;
}

impl ToStyleValue for Length {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Length(to_length(*self))
    }
}

impl ToStyleValue for BackdropFilter {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::BackdropFilter(match self {
            Self::None => BackdropFilterValue::None,
            Self::Blur(radius) => BackdropFilterValue::Blur(to_length(*radius)),
        })
    }
}

impl ToStyleValue for ImageRendering {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::ImageRendering(match self {
            Self::Auto => ImageRenderingValue::Auto,
            Self::Pixelated => ImageRenderingValue::Pixelated,
            Self::CrispEdges => ImageRenderingValue::CrispEdges,
        })
    }
}

impl ToStyleValue for Transform {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Transform(TransformValue(
            self.0.iter().map(to_transform_function).collect(),
        ))
    }
}

impl ToStyleValue for OffsetPath {
    fn to_style_value(&self) -> StyleValue {
        let point = |point: &crate::MotionPathPoint| MotionPathPointValue {
            x: StyleNumber::new(point.x),
            y: StyleNumber::new(point.y),
        };
        StyleValue::OffsetPath(match self {
            Self::None => OffsetPathValue::None,
            Self::Path(commands) => OffsetPathValue::Path(
                commands
                    .iter()
                    .map(|command| match command {
                        MotionPathCommand::MoveTo(value) => {
                            MotionPathCommandValue::MoveTo(point(value))
                        }
                        MotionPathCommand::LineTo(value) => {
                            MotionPathCommandValue::LineTo(point(value))
                        }
                        MotionPathCommand::QuadraticTo { control, to } => {
                            MotionPathCommandValue::QuadraticTo {
                                control: point(control),
                                to: point(to),
                            }
                        }
                        MotionPathCommand::CubicTo {
                            control1,
                            control2,
                            to,
                        } => MotionPathCommandValue::CubicTo {
                            control1: point(control1),
                            control2: point(control2),
                            to: point(to),
                        },
                        MotionPathCommand::ArcTo {
                            radius_x,
                            radius_y,
                            x_axis_rotation,
                            large_arc,
                            sweep,
                            to,
                        } => MotionPathCommandValue::ArcTo {
                            radius_x: StyleNumber::new(*radius_x),
                            radius_y: StyleNumber::new(*radius_y),
                            x_axis_rotation: StyleNumber::new(*x_axis_rotation),
                            large_arc: *large_arc,
                            sweep: *sweep,
                            to: point(to),
                        },
                        MotionPathCommand::Close => MotionPathCommandValue::Close,
                    })
                    .collect(),
            ),
            Self::Circle {
                radius,
                center_x,
                center_y,
            } => OffsetPathValue::Circle {
                radius: to_length_percentage(radius),
                center_x: to_length_percentage(center_x),
                center_y: to_length_percentage(center_y),
            },
            Self::Ellipse {
                radius_x,
                radius_y,
                center_x,
                center_y,
            } => OffsetPathValue::Ellipse {
                radius_x: to_length_percentage(radius_x),
                radius_y: to_length_percentage(radius_y),
                center_x: to_length_percentage(center_x),
                center_y: to_length_percentage(center_y),
            },
            Self::Inset(value) => {
                let radii = value.radii.as_ref().map(|radii| {
                    let vertical = radii.vertical.as_ref().unwrap_or(&radii.horizontal);
                    std::array::from_fn(|index| BorderRadiusValue {
                        horizontal: to_length_percentage(&radii.horizontal[index]),
                        vertical: to_length_percentage(&vertical[index]),
                    })
                });
                OffsetPathValue::Inset(Box::new(InsetPathValue {
                    offsets: std::array::from_fn(|index| {
                        to_length_percentage(&value.offsets[index])
                    }),
                    radii,
                }))
            }
        })
    }
}

impl ToStyleValue for OffsetDistance {
    fn to_style_value(&self) -> StyleValue {
        match self {
            Self::Number(value) => value.to_style_value(),
            Self::Percentage(value) => value.to_style_value(),
        }
    }
}

impl ToStyleValue for OffsetRotate {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::OffsetRotate(match self {
            Self::Auto => OffsetRotateValue::Auto,
            Self::Angle(angle) => OffsetRotateValue::Angle(StyleNumber::new(angle_degrees(*angle))),
        })
    }
}

impl ToStyleValue for Position {
    fn to_style_value(&self) -> StyleValue {
        let (horizontal, vertical) = transform_origin(self);
        StyleValue::TransformOrigin(TransformOriginValue {
            horizontal,
            vertical,
        })
    }
}

impl ToStyleValue for Percentage {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::LengthPercentage(LengthPercentageValue::Percentage(StyleNumber::new(self.0)))
    }
}

impl ToStyleValue for LengthPercentage {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::LengthPercentage(to_length_percentage(self))
    }
}

pub(crate) fn to_border_radius(
    horizontal: &LengthPercentage,
    vertical: &LengthPercentage,
) -> StyleValue {
    StyleValue::BorderRadius(BorderRadiusValue {
        horizontal: to_length_percentage(horizontal),
        vertical: to_length_percentage(vertical),
    })
}

impl ToStyleValue for Number {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Number(StyleNumber::new(self.0))
    }
}

impl ToStyleValue for Integer {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Integer(i64::from(self.0))
    }
}

impl ToStyleValue for CssString {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Text(self.0.clone())
    }
}

impl ToStyleValue for FontStyle {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::FontStyle(match self {
            Self::Normal => FontStyleValue::Normal,
            Self::Italic => FontStyleValue::Italic,
            Self::Oblique => FontStyleValue::Oblique,
        })
    }
}

impl ToStyleValue for FontWeight {
    fn to_style_value(&self) -> StyleValue {
        let value = match self {
            Self::Normal => FontWeightValue::NORMAL,
            Self::Bold => FontWeightValue::BOLD,
            Self::Numeric(value) => FontWeightValue::from_raw(*value),
        };
        StyleValue::FontWeight(value)
    }
}

impl ToStyleValue for LineHeight {
    fn to_style_value(&self) -> StyleValue {
        let value = match self {
            Self::Normal => LineHeightValue::Normal,
            Self::Number(value) => LineHeightValue::Number(StyleNumber::new(*value)),
            Self::LengthPercentage(value) => {
                LineHeightValue::LengthPercentage(to_length_percentage(value))
            }
        };
        StyleValue::LineHeight(value)
    }
}

impl ToStyleValue for Color {
    fn to_style_value(&self) -> StyleValue {
        let value = match self {
            Self::Named(value) => ColorValue::Named(value.name().into()),
            Self::Transparent => ColorValue::Rgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: StyleNumber::new(0.0),
            },
            Self::Rgba(red, green, blue, alpha) => ColorValue::Rgba {
                red: *red,
                green: *green,
                blue: *blue,
                alpha: StyleNumber::new(*alpha),
            },
            Self::Hsla { h, s, l, a } => ColorValue::Hsla {
                hue_degrees: StyleNumber::new(angle_degrees(*h)),
                saturation: StyleNumber::new(*s),
                lightness: StyleNumber::new(*l),
                alpha: StyleNumber::new(*a),
            },
        };
        StyleValue::Color(value)
    }
}

impl ToStyleValue for crate::BorderStyle {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::BorderStyle(match self {
            Self::None => BorderStyleValue::None,
            Self::Hidden => BorderStyleValue::Hidden,
            Self::Solid => BorderStyleValue::Solid,
            Self::Dashed => BorderStyleValue::Dashed,
            Self::Dotted => BorderStyleValue::Dotted,
            Self::Double => BorderStyleValue::Double,
            Self::Groove => BorderStyleValue::Groove,
            Self::Ridge => BorderStyleValue::Ridge,
            Self::Inset => BorderStyleValue::Inset,
            Self::Outset => BorderStyleValue::Outset,
        })
    }
}

impl ToStyleValue for Overflow {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Overflow(match self {
            Self::Visible => OverflowValue::Visible,
            Self::Hidden => OverflowValue::Hidden,
        })
    }
}

impl ToStyleValue for Visibility {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Visibility(match self {
            Self::Visible => VisibilityValue::Visible,
            Self::Hidden => VisibilityValue::Hidden,
        })
    }
}

impl ToStyleValue for Display {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Display(match self {
            Self::None => DisplayValue::None,
            Self::Flex => DisplayValue::Flex,
            Self::Grid => DisplayValue::Grid,
            Self::Block => DisplayValue::Block,
            Self::FlowRoot => DisplayValue::FlowRoot,
            Self::Linear => DisplayValue::Linear,
            Self::Relative => DisplayValue::Relative,
        })
    }
}

impl ToStyleValue for Float {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Float(match self {
            Self::None => FloatValue::None,
            Self::Left => FloatValue::Left,
            Self::Right => FloatValue::Right,
        })
    }
}

impl ToStyleValue for Clear {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Clear(match self {
            Self::None => ClearValue::None,
            Self::Left => ClearValue::Left,
            Self::Right => ClearValue::Right,
            Self::Both => ClearValue::Both,
        })
    }
}

impl ToStyleValue for PositionKind {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Position(match self {
            Self::Relative => PositionValue::Relative,
            Self::Absolute => PositionValue::Absolute,
            Self::Fixed => PositionValue::Fixed,
            Self::Sticky => PositionValue::Sticky,
        })
    }
}

impl ToStyleValue for BoxSizing {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::BoxSizing(match self {
            Self::ContentBox => BoxSizingValue::ContentBox,
            Self::BorderBox => BoxSizingValue::BorderBox,
        })
    }
}

impl ToStyleValue for Direction {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Direction(match self {
            Self::Ltr => DirectionValue::Ltr,
            Self::Rtl => DirectionValue::Rtl,
        })
    }
}

impl ToStyleValue for Size {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Size(match self {
            Self::Auto => SizeValue::Auto,
            Self::LengthPercentage(value) => {
                SizeValue::LengthPercentage(to_length_percentage(value))
            }
            Self::MaxContent => SizeValue::MaxContent,
            Self::MinContent => SizeValue::MinContent,
            Self::FitContent(value) => {
                SizeValue::FitContent(value.0.as_ref().map(to_length_percentage))
            }
            Self::None => SizeValue::None,
        })
    }
}

impl ToStyleValue for MarginValue {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::LengthPercentageAuto(match self {
            Self::Auto => LengthPercentageAutoValue::Auto,
            Self::LengthPercentage(value) => {
                LengthPercentageAutoValue::LengthPercentage(to_length_percentage(value))
            }
        })
    }
}

impl ToStyleValue for FlexDirection {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::FlexDirection(match self {
            Self::Row => FlexDirectionValue::Row,
            Self::RowReverse => FlexDirectionValue::RowReverse,
            Self::Column => FlexDirectionValue::Column,
            Self::ColumnReverse => FlexDirectionValue::ColumnReverse,
        })
    }
}

impl ToStyleValue for FlexWrap {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::FlexWrap(match self {
            Self::Nowrap => FlexWrapValue::NoWrap,
            Self::Wrap => FlexWrapValue::Wrap,
            Self::WrapReverse => FlexWrapValue::WrapReverse,
        })
    }
}

impl ToStyleValue for FlexBasis {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::FlexBasis(match self {
            Self::Auto => FlexBasisValue::Auto,
            Self::Content => FlexBasisValue::Content,
            Self::LengthPercentage(value) => {
                FlexBasisValue::LengthPercentage(to_length_percentage(value))
            }
        })
    }
}

impl ToStyleValue for GridLine {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::GridPlacement(match self {
            Self::Auto => GridPlacementValue::Auto,
            Self::Number(value) => GridPlacementValue::Line(*value),
            Self::Span(value) => GridPlacementValue::Span(*value),
            Self::Named(name, occurrence) => {
                GridPlacementValue::NamedLine(name.clone(), *occurrence)
            }
            Self::NamedSpan(name, occurrence) => {
                GridPlacementValue::NamedSpan(name.clone(), *occurrence)
            }
        })
    }
}

impl ToStyleValue for GridAutoFlow {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::GridAutoFlow(match self {
            Self::Row => GridAutoFlowValue::Row,
            Self::Column => GridAutoFlowValue::Column,
            Self::RowDense => GridAutoFlowValue::RowDense,
            Self::ColumnDense => GridAutoFlowValue::ColumnDense,
        })
    }
}

impl ToStyleValue for GridTemplate {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::GridTemplate(GridTemplateValue {
            components: self
                .components
                .iter()
                .map(to_grid_template_component)
                .collect(),
            line_names: self.line_names.clone(),
        })
    }
}

impl ToStyleValue for GridTemplateAreas {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::GridTemplateAreas(GridTemplateAreasValue {
            areas: self
                .areas
                .iter()
                .map(|area| GridTemplateAreaValue {
                    name: area.name.clone(),
                    row_start: area.row_start,
                    row_end: area.row_end,
                    column_start: area.column_start,
                    column_end: area.column_end,
                })
                .collect(),
            row_count: self.row_count,
            column_count: self.column_count,
        })
    }
}

pub(crate) fn grid_auto_tracks(value: &GridTemplate) -> StyleValue {
    StyleValue::GridTracks(
        value
            .components
            .iter()
            .filter_map(|component| match component {
                GridTemplateComponent::Track(track) => Some(to_grid_track(track)),
                GridTemplateComponent::Repeat { .. } => None,
            })
            .collect(),
    )
}

fn to_grid_template_component(value: &GridTemplateComponent) -> GridTemplateComponentValue {
    match value {
        GridTemplateComponent::Track(track) => {
            GridTemplateComponentValue::Track(to_grid_track(track))
        }
        GridTemplateComponent::Repeat {
            count,
            tracks,
            line_names,
        } => GridTemplateComponentValue::Repeat(GridTemplateRepetitionValue {
            count: match count {
                GridRepeatCount::Count(value) => GridRepetitionCountValue::Count(*value),
                GridRepeatCount::AutoFill => GridRepetitionCountValue::AutoFill,
                GridRepeatCount::AutoFit => GridRepetitionCountValue::AutoFit,
            },
            tracks: tracks.iter().map(to_grid_track).collect(),
            line_names: line_names.clone(),
        }),
    }
}

fn to_grid_track(value: &GridTrack) -> GridTrackSizingValue {
    GridTrackSizingValue {
        min: match &value.min {
            GridTrackMin::Fixed(value) => {
                GridMinTrackSizingValue::Fixed(to_length_percentage(value))
            }
            GridTrackMin::MinContent => GridMinTrackSizingValue::MinContent,
            GridTrackMin::MaxContent => GridMinTrackSizingValue::MaxContent,
            GridTrackMin::Auto => GridMinTrackSizingValue::Auto,
        },
        max: match &value.max {
            GridTrackMax::Fixed(value) => {
                GridMaxTrackSizingValue::Fixed(to_length_percentage(value))
            }
            GridTrackMax::MinContent => GridMaxTrackSizingValue::MinContent,
            GridTrackMax::MaxContent => GridMaxTrackSizingValue::MaxContent,
            GridTrackMax::FitContent(value) => {
                GridMaxTrackSizingValue::FitContent(to_length_percentage(value))
            }
            GridTrackMax::Auto => GridMaxTrackSizingValue::Auto,
            GridTrackMax::Fraction(value) => {
                GridMaxTrackSizingValue::Fraction(StyleNumber::new(*value))
            }
        },
    }
}

impl ToStyleValue for JustifyContent {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::JustifyContent(match self {
            Self::Stretch => JustifyContentValue::Stretch,
            Self::FlexStart => JustifyContentValue::FlexStart,
            Self::FlexEnd => JustifyContentValue::FlexEnd,
            Self::Center => JustifyContentValue::Center,
            Self::SpaceBetween => JustifyContentValue::SpaceBetween,
            Self::SpaceAround => JustifyContentValue::SpaceAround,
            Self::SpaceEvenly => JustifyContentValue::SpaceEvenly,
            Self::Start => JustifyContentValue::Start,
            Self::End => JustifyContentValue::End,
        })
    }
}

impl ToStyleValue for AlignItems {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::AlignItems(match self {
            Self::Stretch => AlignItemsValue::Stretch,
            Self::FlexStart => AlignItemsValue::FlexStart,
            Self::FlexEnd => AlignItemsValue::FlexEnd,
            Self::Center => AlignItemsValue::Center,
            Self::Baseline => AlignItemsValue::Baseline,
            Self::Start => AlignItemsValue::Start,
            Self::End => AlignItemsValue::End,
        })
    }
}

impl ToStyleValue for AlignSelf {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::AlignSelf(match self {
            Self::Auto => AlignSelfValue::Auto,
            Self::Stretch => AlignSelfValue::Stretch,
            Self::FlexStart => AlignSelfValue::FlexStart,
            Self::FlexEnd => AlignSelfValue::FlexEnd,
            Self::Center => AlignSelfValue::Center,
            Self::Baseline => AlignSelfValue::Baseline,
            Self::Start => AlignSelfValue::Start,
            Self::End => AlignSelfValue::End,
        })
    }
}

impl ToStyleValue for AlignContent {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::AlignContent(match self {
            Self::Stretch => AlignContentValue::Stretch,
            Self::FlexStart => AlignContentValue::FlexStart,
            Self::FlexEnd => AlignContentValue::FlexEnd,
            Self::Center => AlignContentValue::Center,
            Self::SpaceBetween => AlignContentValue::SpaceBetween,
            Self::SpaceAround => AlignContentValue::SpaceAround,
            Self::SpaceEvenly => AlignContentValue::SpaceEvenly,
        })
    }
}

fn angle_degrees(value: Angle) -> f32 {
    match value {
        Angle::Deg(value) => value,
        Angle::Rad(value) => value.to_degrees(),
        Angle::Turn(value) => value * 360.0,
    }
}

fn to_transform_function(value: &TransformFn) -> TransformFunctionValue {
    match value {
        TransformFn::Translate(x, y) => {
            TransformFunctionValue::Translate(to_length_percentage(x), to_length_percentage(y))
        }
        TransformFn::TranslateX(x) => TransformFunctionValue::TranslateX(to_length_percentage(x)),
        TransformFn::TranslateY(y) => TransformFunctionValue::TranslateY(to_length_percentage(y)),
        TransformFn::TranslateZ(z) => TransformFunctionValue::TranslateZ(to_length(*z)),
        TransformFn::Translate3d(x, y, z) => TransformFunctionValue::Translate3d(
            to_length_percentage(x),
            to_length_percentage(y),
            to_length(*z),
        ),
        TransformFn::Rotate(angle) => {
            TransformFunctionValue::Rotate(StyleNumber::new(angle_degrees(*angle)))
        }
        TransformFn::RotateX(angle) => {
            TransformFunctionValue::RotateX(StyleNumber::new(angle_degrees(*angle)))
        }
        TransformFn::RotateY(angle) => {
            TransformFunctionValue::RotateY(StyleNumber::new(angle_degrees(*angle)))
        }
        TransformFn::RotateZ(angle) => {
            TransformFunctionValue::RotateZ(StyleNumber::new(angle_degrees(*angle)))
        }
        TransformFn::Scale(x, y) => {
            TransformFunctionValue::Scale(StyleNumber::new(*x), StyleNumber::new(*y))
        }
        TransformFn::ScaleX(x) => TransformFunctionValue::ScaleX(StyleNumber::new(*x)),
        TransformFn::ScaleY(y) => TransformFunctionValue::ScaleY(StyleNumber::new(*y)),
        TransformFn::Skew(x, y) => TransformFunctionValue::Skew(
            StyleNumber::new(angle_degrees(*x)),
            StyleNumber::new(angle_degrees(*y)),
        ),
        TransformFn::SkewX(angle) => {
            TransformFunctionValue::SkewX(StyleNumber::new(angle_degrees(*angle)))
        }
        TransformFn::SkewY(angle) => {
            TransformFunctionValue::SkewY(StyleNumber::new(angle_degrees(*angle)))
        }
        TransformFn::Matrix(matrix) => TransformFunctionValue::Matrix(matrix.map(StyleNumber::new)),
        TransformFn::Matrix3d(matrix) => {
            TransformFunctionValue::Matrix3d(matrix.map(StyleNumber::new))
        }
    }
}

fn transform_origin(value: &Position) -> (LengthPercentageValue, LengthPercentageValue) {
    let center = || LengthPercentageValue::Percentage(StyleNumber::new(50.0));
    let keyword = |value, horizontal: bool| {
        let percentage = match (value, horizontal) {
            (PositionKeyword::Left, true) | (PositionKeyword::Top, false) => 0.0,
            (PositionKeyword::Right, true) | (PositionKeyword::Bottom, false) => 100.0,
            (PositionKeyword::Center, _) => 50.0,
            _ => 50.0,
        };
        LengthPercentageValue::Percentage(StyleNumber::new(percentage))
    };
    match value {
        Position::Keyword(PositionKeyword::Left | PositionKeyword::Right) => {
            (keyword(position_keyword(value), true), center())
        }
        Position::Keyword(PositionKeyword::Top | PositionKeyword::Bottom) => {
            (center(), keyword(position_keyword(value), false))
        }
        Position::Keyword(PositionKeyword::Center) => (center(), center()),
        Position::Keywords(first, second) => match first {
            PositionKeyword::Top | PositionKeyword::Bottom => {
                (keyword(*second, true), keyword(*first, false))
            }
            PositionKeyword::Left | PositionKeyword::Right => {
                (keyword(*first, true), keyword(*second, false))
            }
            PositionKeyword::Center => match second {
                PositionKeyword::Top | PositionKeyword::Bottom => {
                    (center(), keyword(*second, false))
                }
                PositionKeyword::Left | PositionKeyword::Right => {
                    (keyword(*second, true), center())
                }
                PositionKeyword::Center => (center(), center()),
            },
        },
        Position::Coords(horizontal, vertical) => (
            to_length_percentage(horizontal),
            to_length_percentage(vertical),
        ),
        Position::Mixed(axis, offset) => match axis {
            PositionKeyword::Top | PositionKeyword::Bottom => {
                (to_length_percentage(offset), keyword(*axis, false))
            }
            PositionKeyword::Left | PositionKeyword::Right => {
                (keyword(*axis, true), to_length_percentage(offset))
            }
            PositionKeyword::Center => (center(), to_length_percentage(offset)),
        },
    }
}

fn position_keyword(value: &Position) -> PositionKeyword {
    match value {
        Position::Keyword(value) => *value,
        _ => unreachable!("called only for Position::Keyword"),
    }
}

fn to_length(value: Length) -> LengthValue {
    let (value, unit) = match value {
        Length::Zero => return LengthValue::Zero,
        Length::Px(value) => (value, LengthUnit::Px),
        Length::Rpx(value) => (value, LengthUnit::Rpx),
        Length::Ppx(value) => (value, LengthUnit::Ppx),
        Length::Em(value) => (value, LengthUnit::Em),
        Length::Rem(value) => (value, LengthUnit::Rem),
        Length::Vh(value) => (value, LengthUnit::Vh),
        Length::Vw(value) => (value, LengthUnit::Vw),
    };
    LengthValue::Dimension {
        value: StyleNumber::new(value),
        unit,
    }
}

fn to_length_percentage(value: &LengthPercentage) -> LengthPercentageValue {
    match value {
        LengthPercentage::Length(value) => LengthPercentageValue::Length(to_length(*value)),
        LengthPercentage::Percentage(value) => {
            LengthPercentageValue::Percentage(StyleNumber::new(value.0))
        }
        LengthPercentage::Calc(value) => {
            LengthPercentageValue::Calc(Box::new(to_calc_expression(value)))
        }
    }
}

fn to_calc_expression(value: &CalcExpr) -> CalcExpression {
    match value {
        CalcExpr::Value(value) => CalcExpression::Value(Box::new(to_length_percentage(value))),
        CalcExpr::Number(value) => CalcExpression::Number(StyleNumber::new(*value)),
        CalcExpr::Add(left, right) => CalcExpression::Add(
            Box::new(to_calc_expression(left)),
            Box::new(to_calc_expression(right)),
        ),
        CalcExpr::Sub(left, right) => CalcExpression::Sub(
            Box::new(to_calc_expression(left)),
            Box::new(to_calc_expression(right)),
        ),
        CalcExpr::Mul(left, right) => CalcExpression::Mul(
            Box::new(to_calc_expression(left)),
            Box::new(to_calc_expression(right)),
        ),
        CalcExpr::Div(left, right) => CalcExpression::Div(
            Box::new(to_calc_expression(left)),
            Box::new(to_calc_expression(right)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_length_unit_converts_semantically() {
        let cases = [
            (Length::Zero, LengthValue::Zero),
            (Length::Px(1.0), dimension(1.0, LengthUnit::Px)),
            (Length::Rpx(2.0), dimension(2.0, LengthUnit::Rpx)),
            (Length::Ppx(3.0), dimension(3.0, LengthUnit::Ppx)),
            (Length::Em(4.0), dimension(4.0, LengthUnit::Em)),
            (Length::Rem(5.0), dimension(5.0, LengthUnit::Rem)),
            (Length::Vh(6.0), dimension(6.0, LengthUnit::Vh)),
            (Length::Vw(7.0), dimension(7.0, LengthUnit::Vw)),
        ];
        for (input, expected) in cases {
            assert_eq!(input.to_style_value(), StyleValue::Length(expected));
        }
    }

    #[test]
    fn scalar_authoring_types_keep_semantics() {
        assert_eq!(
            Number(1.5).to_style_value(),
            StyleValue::Number(StyleNumber::new(1.5))
        );
        assert_eq!(Integer(-2).to_style_value(), StyleValue::Integer(-2));
        assert_eq!(
            CssString::new("hello").to_style_value(),
            StyleValue::Text("hello".into())
        );
        assert_eq!(
            Percentage(25.0).to_style_value(),
            StyleValue::LengthPercentage(LengthPercentageValue::Percentage(StyleNumber::new(25.0)))
        );
    }

    #[test]
    fn every_calc_operator_converts_as_a_tree() {
        let leaf = || CalcExpr::value(Length::Px(1.0));
        for expression in [
            leaf().add(leaf()),
            leaf().sub(leaf()),
            leaf().mul(CalcExpr::number(2.0)),
            leaf().div(CalcExpr::number(2.0)),
        ] {
            let value = LengthPercentage::calc(expression).to_style_value();
            assert!(matches!(
                value,
                StyleValue::LengthPercentage(LengthPercentageValue::Calc(_))
            ));
        }
    }

    #[test]
    fn inherited_authoring_values_convert_without_css_parsing() {
        for (input, expected) in [
            (FontStyle::Normal, FontStyleValue::Normal),
            (FontStyle::Italic, FontStyleValue::Italic),
            (FontStyle::Oblique, FontStyleValue::Oblique),
        ] {
            assert_eq!(input.to_style_value(), StyleValue::FontStyle(expected));
        }
        for (input, expected) in [
            (FontWeight::Normal, FontWeightValue::NORMAL),
            (FontWeight::Bold, FontWeightValue::BOLD),
            (FontWeight::Numeric(650), FontWeightValue::from_raw(650)),
        ] {
            assert_eq!(input.to_style_value(), StyleValue::FontWeight(expected));
        }
        assert_eq!(
            LineHeight::Normal.to_style_value(),
            StyleValue::LineHeight(LineHeightValue::Normal)
        );
        assert_eq!(
            LineHeight::Number(1.5).to_style_value(),
            StyleValue::LineHeight(LineHeightValue::Number(StyleNumber::new(1.5)))
        );
        assert_eq!(
            LineHeight::LengthPercentage(Length::Px(20.0).into()).to_style_value(),
            StyleValue::LineHeight(LineHeightValue::LengthPercentage(
                LengthPercentageValue::Length(dimension(20.0, LengthUnit::Px))
            ))
        );
    }

    #[test]
    fn every_color_form_becomes_a_typed_color_value() {
        assert_eq!(
            Color::Named(crate::NamedColor::Red).to_style_value(),
            StyleValue::Color(ColorValue::Named("red".into()))
        );
        assert_eq!(
            Color::Transparent.to_style_value(),
            StyleValue::Color(ColorValue::Rgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: StyleNumber::new(0.0),
            })
        );
        assert_eq!(
            Color::rgba(1, 2, 3, 0.5).to_style_value(),
            StyleValue::Color(ColorValue::Rgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: StyleNumber::new(0.5),
            })
        );
        for (angle, degrees) in [
            (Angle::Deg(90.0), 90.0),
            (Angle::Rad(core::f32::consts::FRAC_PI_2), 90.0),
            (Angle::Turn(0.25), 90.0),
        ] {
            assert_eq!(
                Color::Hsla {
                    h: angle,
                    s: 50.0,
                    l: 25.0,
                    a: 0.75,
                }
                .to_style_value(),
                StyleValue::Color(ColorValue::Hsla {
                    hue_degrees: StyleNumber::new(degrees),
                    saturation: StyleNumber::new(50.0),
                    lightness: StyleNumber::new(25.0),
                    alpha: StyleNumber::new(0.75),
                })
            );
        }
    }

    fn dimension(value: f32, unit: LengthUnit) -> LengthValue {
        LengthValue::Dimension {
            value: StyleNumber::new(value),
            unit,
        }
    }
}
