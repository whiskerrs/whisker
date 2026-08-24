//! Lowering from computed style into the Host-independent paint protocol.

use whisker_protocol::{
    BorderLineStyle, BoxClip, BoxPaint, OverflowClip, PaintColor, PaintCornerRadius, PaintCorners,
    PaintEdges, PaintLengthPercentage, Visibility,
};
use whisker_style::{
    BorderStyleValue, ColorValue, ComputedLayoutStyle, ComputedLengthPercentage,
    ComputedPaintStyle, OverflowValue, VisibilityValue,
};

/// Complete common presentation values derived from one computed node style.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredPaint {
    /// Background and border paint.
    pub box_paint: BoxPaint,
    /// Descendant overflow clip.
    pub clip: BoxClip,
    /// Group opacity.
    pub opacity: f32,
    /// Paint visibility.
    pub visibility: Visibility,
    /// Sibling stacking key.
    pub z_order: i32,
}

/// Lowers renderer-independent computed style into protocol-owned values.
pub fn lower_paint(style: &ComputedPaintStyle, layout: &ComputedLayoutStyle) -> LoweredPaint {
    LoweredPaint {
        box_paint: BoxPaint {
            background_color: lower_color(&style.background_color),
            border_widths: edges(&layout.border, length),
            border_colors: PaintEdges {
                top: effective_border_color(&style.border_colors.top, style.border_styles.top),
                right: effective_border_color(
                    &style.border_colors.right,
                    style.border_styles.right,
                ),
                bottom: effective_border_color(
                    &style.border_colors.bottom,
                    style.border_styles.bottom,
                ),
                left: effective_border_color(&style.border_colors.left, style.border_styles.left),
            },
            border_styles: edges(&style.border_styles, lower_border_style),
            border_radii: PaintCorners {
                top_left: corner_radius(&style.border_radii.top_left),
                top_right: corner_radius(&style.border_radii.top_right),
                bottom_right: corner_radius(&style.border_radii.bottom_right),
                bottom_left: corner_radius(&style.border_radii.bottom_left),
            },
        },
        clip: BoxClip {
            horizontal: lower_overflow(style.overflow_x),
            vertical: lower_overflow(style.overflow_y),
        },
        opacity: style.opacity.get(),
        visibility: match style.visibility {
            VisibilityValue::Visible => Visibility::Visible,
            VisibilityValue::Hidden => Visibility::Hidden,
        },
        z_order: style.z_index,
    }
}

fn edges<T, U>(input: &whisker_style::Edges<T>, map: impl Fn(&T) -> U) -> PaintEdges<U> {
    PaintEdges {
        top: map(&input.top),
        right: map(&input.right),
        bottom: map(&input.bottom),
        left: map(&input.left),
    }
}

fn length(value: &ComputedLengthPercentage) -> PaintLengthPercentage {
    PaintLengthPercentage {
        length: value.length(),
        fraction: value.fraction(),
    }
}

fn corner_radius(value: &whisker_style::ComputedCornerRadius) -> PaintCornerRadius {
    PaintCornerRadius {
        horizontal: length(&value.horizontal),
        vertical: length(&value.vertical),
    }
}

fn lower_color(color: &ColorValue) -> PaintColor {
    match color {
        ColorValue::Named(name) => PaintColor::Named(name.clone()),
        ColorValue::Rgba {
            red,
            green,
            blue,
            alpha,
        } => PaintColor::Srgba {
            red: *red,
            green: *green,
            blue: *blue,
            alpha: alpha.get(),
        },
        ColorValue::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => PaintColor::Hsla {
            hue_degrees: hue_degrees.get(),
            saturation: saturation.get(),
            lightness: lightness.get(),
            alpha: alpha.get(),
        },
    }
}

fn lower_border_style(value: &BorderStyleValue) -> BorderLineStyle {
    match value {
        BorderStyleValue::None => BorderLineStyle::None,
        BorderStyleValue::Hidden => BorderLineStyle::Hidden,
        BorderStyleValue::Solid => BorderLineStyle::Solid,
        BorderStyleValue::Dashed => BorderLineStyle::Dashed,
        BorderStyleValue::Dotted => BorderLineStyle::Dotted,
        BorderStyleValue::Double => BorderLineStyle::Double,
        BorderStyleValue::Groove => BorderLineStyle::Groove,
        BorderStyleValue::Ridge => BorderLineStyle::Ridge,
        BorderStyleValue::Inset => BorderLineStyle::Inset,
        BorderStyleValue::Outset => BorderLineStyle::Outset,
    }
}

fn effective_border_color(color: &ColorValue, style: BorderStyleValue) -> PaintColor {
    if matches!(style, BorderStyleValue::None | BorderStyleValue::Hidden) {
        PaintColor::Srgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0.0,
        }
    } else {
        lower_color(color)
    }
}

fn lower_overflow(value: OverflowValue) -> OverflowClip {
    match value {
        OverflowValue::Visible => OverflowClip::Visible,
        OverflowValue::Hidden => OverflowClip::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_style::{ComputedCornerRadius, Corners, Edges, StyleNumber};

    fn color(name: &str) -> ColorValue {
        ColorValue::Named(name.into())
    }

    fn paint_style() -> ComputedPaintStyle {
        ComputedPaintStyle {
            background_color: color("background"),
            background_images: Vec::new(),
            border_colors: Edges {
                top: color("top"),
                right: color("right"),
                bottom: color("bottom"),
                left: color("left"),
            },
            border_styles: Edges {
                top: BorderStyleValue::Solid,
                right: BorderStyleValue::Dashed,
                bottom: BorderStyleValue::Dotted,
                left: BorderStyleValue::Double,
            },
            border_radii: Corners {
                top_left: radius(1.0, 0.1, 11.0, 0.11),
                top_right: radius(2.0, 0.2, 12.0, 0.12),
                bottom_right: radius(3.0, 0.3, 13.0, 0.13),
                bottom_left: radius(4.0, 0.4, 14.0, 0.14),
            },
            opacity: StyleNumber::new(0.5),
            visibility: VisibilityValue::Hidden,
            overflow_x: OverflowValue::Visible,
            overflow_y: OverflowValue::Hidden,
            z_index: -3,
        }
    }

    fn radius(
        horizontal_length: f32,
        horizontal_fraction: f32,
        vertical_length: f32,
        vertical_fraction: f32,
    ) -> ComputedCornerRadius {
        ComputedCornerRadius {
            horizontal: ComputedLengthPercentage::new(horizontal_length, horizontal_fraction),
            vertical: ComputedLengthPercentage::new(vertical_length, vertical_fraction),
        }
    }

    #[test]
    fn lowers_complete_box_paint_clip_and_compositing_state() {
        let layout = ComputedLayoutStyle {
            border: Edges {
                top: ComputedLengthPercentage::new(1.0, 0.0),
                right: ComputedLengthPercentage::new(2.0, 0.1),
                bottom: ComputedLengthPercentage::new(3.0, 0.2),
                left: ComputedLengthPercentage::new(4.0, 0.3),
            },
            ..ComputedLayoutStyle::default()
        };
        let lowered = lower_paint(&paint_style(), &layout);

        assert_eq!(
            lowered.box_paint.background_color,
            PaintColor::Named("background".into())
        );
        assert_eq!(lowered.box_paint.border_widths.left.length, 4.0);
        assert_eq!(lowered.box_paint.border_widths.left.fraction, 0.3);
        assert_eq!(
            lowered.box_paint.border_radii.bottom_left.horizontal.length,
            4.0
        );
        assert_eq!(
            lowered.box_paint.border_radii.bottom_left.vertical.length,
            14.0
        );
        assert_eq!(
            lowered.box_paint.border_colors.top,
            PaintColor::Named("top".into())
        );
        assert_eq!(
            lowered.box_paint.border_styles.right,
            BorderLineStyle::Dashed
        );
        assert_eq!(lowered.clip.horizontal, OverflowClip::Visible);
        assert_eq!(lowered.clip.vertical, OverflowClip::Hidden);
        assert_eq!(lowered.opacity, 0.5);
        assert_eq!(lowered.visibility, Visibility::Hidden);
        assert_eq!(lowered.z_order, -3);
    }

    #[test]
    fn lowers_every_color_border_visibility_and_overflow_variant() {
        assert_eq!(
            lower_color(&ColorValue::Rgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: StyleNumber::new(0.4),
            }),
            PaintColor::Srgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: 0.4,
            }
        );
        assert_eq!(
            lower_color(&ColorValue::Hsla {
                hue_degrees: StyleNumber::new(10.0),
                saturation: StyleNumber::new(20.0),
                lightness: StyleNumber::new(30.0),
                alpha: StyleNumber::new(0.5),
            }),
            PaintColor::Hsla {
                hue_degrees: 10.0,
                saturation: 20.0,
                lightness: 30.0,
                alpha: 0.5,
            }
        );

        for (source, expected) in [
            (BorderStyleValue::None, BorderLineStyle::None),
            (BorderStyleValue::Hidden, BorderLineStyle::Hidden),
            (BorderStyleValue::Solid, BorderLineStyle::Solid),
            (BorderStyleValue::Dashed, BorderLineStyle::Dashed),
            (BorderStyleValue::Dotted, BorderLineStyle::Dotted),
            (BorderStyleValue::Double, BorderLineStyle::Double),
            (BorderStyleValue::Groove, BorderLineStyle::Groove),
            (BorderStyleValue::Ridge, BorderLineStyle::Ridge),
            (BorderStyleValue::Inset, BorderLineStyle::Inset),
            (BorderStyleValue::Outset, BorderLineStyle::Outset),
        ] {
            assert_eq!(lower_border_style(&source), expected);
        }

        let transparent = PaintColor::Srgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0.0,
        };
        assert_eq!(
            effective_border_color(&color("ignored"), BorderStyleValue::None),
            transparent
        );
        assert_eq!(
            effective_border_color(&color("ignored"), BorderStyleValue::Hidden),
            transparent
        );
        assert_eq!(
            effective_border_color(&color("kept"), BorderStyleValue::Solid),
            PaintColor::Named("kept".into())
        );
        assert_eq!(
            lower_overflow(OverflowValue::Visible),
            OverflowClip::Visible
        );
        assert_eq!(lower_overflow(OverflowValue::Hidden), OverflowClip::Hidden);

        let mut visible = paint_style();
        visible.visibility = VisibilityValue::Visible;
        assert_eq!(
            lower_paint(&visible, &ComputedLayoutStyle::default()).visibility,
            Visibility::Visible
        );
    }
}
