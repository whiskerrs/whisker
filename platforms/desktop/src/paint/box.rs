use whisker_protocol::{
    BorderLineStyle, BoxPaint, LayoutRect, PaintBox, PaintColor, PaintCornerRadius, PaintCorners,
    PaintLengthPercentage,
};

use super::color::gpu_color;
use crate::scene::is_transparent;

/// One box fill or complete border ring ready for GPU vertex expansion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoxPrimitive {
    pub(crate) outer_rect: LayoutRect,
    pub(crate) outer_radii_x: [f32; 4],
    pub(crate) outer_radii_y: [f32; 4],
    pub(crate) inner_rect: LayoutRect,
    pub(crate) inner_radii_x: [f32; 4],
    pub(crate) inner_radii_y: [f32; 4],
    pub(crate) border_widths: [f32; 4],
    pub(crate) color: [f32; 4],
    pub(crate) border_colors: [[f32; 4]; 4],
    pub(crate) border_styles: [f32; 4],
    pub(crate) kind: BoxPrimitiveKind,
}

/// Selects the fill or border-ring branch in the shared box shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoxPrimitiveKind {
    Fill,
    LinearGradient,
    Border,
}

impl BoxPrimitiveKind {
    pub(crate) const fn shader_mode(self) -> f32 {
        match self {
            Self::Fill => -1.0,
            Self::LinearGradient => -2.0,
            Self::Border => 1.0,
        }
    }
}

/// Builds the quad used by a background image layer. The shader evaluates the
/// gradient against its independently resolved positioning box.
pub(crate) fn background_gradient_primitive(
    rect: LayoutRect,
    paint: &BoxPaint,
    clip: PaintBox,
) -> BoxPrimitive {
    let geometry = resolve_box_geometry(rect, paint);
    let geometry = match clip {
        PaintBox::Border => geometry,
        PaintBox::Padding => BoxGeometry {
            outer_rect: geometry.inner_rect,
            outer_radii: geometry.inner_radii,
            inner_rect: geometry.inner_rect,
            inner_radii: geometry.inner_radii,
            border_widths: [0.0; 4],
        },
        _ => geometry,
    };
    geometry.primitive(
        [0.0; 4],
        [[0.0; 4]; 4],
        [0.0; 4],
        BoxPrimitiveKind::LinearGradient,
    )
}

/// Lowers one semantic box without allocating or dynamically dispatching.
pub(crate) fn lower_box(
    rect: LayoutRect,
    paint: &BoxPaint,
    opacity: f32,
    mut emit: impl FnMut(BoxPrimitive),
) {
    let geometry = resolve_box_geometry(rect, paint);
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    if !is_transparent(&paint.background_color) {
        let color = gpu_color(&paint.background_color, opacity);
        if color[3] > 0.0 {
            emit(geometry.primitive(color, [[0.0; 4]; 4], [0.0; 4], BoxPrimitiveKind::Fill));
        }
    }
    let [top, right, bottom, left] = geometry.border_widths;
    let border_colors = [
        border_color(
            paint.border_styles.top,
            top,
            &paint.border_colors.top,
            opacity,
        ),
        border_color(
            paint.border_styles.right,
            right,
            &paint.border_colors.right,
            opacity,
        ),
        border_color(
            paint.border_styles.bottom,
            bottom,
            &paint.border_colors.bottom,
            opacity,
        ),
        border_color(
            paint.border_styles.left,
            left,
            &paint.border_colors.left,
            opacity,
        ),
    ];
    if border_colors.iter().any(|color| color[3] > 0.0) {
        emit(geometry.primitive(
            [0.0; 4],
            border_colors,
            [
                border_style(paint.border_styles.top),
                border_style(paint.border_styles.right),
                border_style(paint.border_styles.bottom),
                border_style(paint.border_styles.left),
            ],
            BoxPrimitiveKind::Border,
        ));
    }
}

fn border_style(style: BorderLineStyle) -> f32 {
    match style {
        BorderLineStyle::None | BorderLineStyle::Hidden => 0.0,
        BorderLineStyle::Solid => 1.0,
        BorderLineStyle::Dashed => 2.0,
        BorderLineStyle::Dotted => 3.0,
        BorderLineStyle::Double => 4.0,
        BorderLineStyle::Groove => 5.0,
        BorderLineStyle::Ridge => 6.0,
        BorderLineStyle::Inset => 7.0,
        BorderLineStyle::Outset => 8.0,
    }
}

fn paints_line(style: BorderLineStyle) -> bool {
    !matches!(style, BorderLineStyle::None | BorderLineStyle::Hidden)
}

fn border_color(style: BorderLineStyle, width: f32, color: &PaintColor, opacity: f32) -> [f32; 4] {
    if paints_line(style) && width > 0.0 {
        gpu_color(color, opacity)
    } else {
        [0.0; 4]
    }
}

fn resolve_length(value: PaintLengthPercentage, axis: f32) -> f32 {
    value.length + value.fraction * axis
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedRadii {
    pub(crate) horizontal: [f32; 4],
    pub(crate) vertical: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoxGeometry {
    pub(crate) outer_rect: LayoutRect,
    pub(crate) outer_radii: ResolvedRadii,
    pub(crate) inner_rect: LayoutRect,
    pub(crate) inner_radii: ResolvedRadii,
    pub(crate) border_widths: [f32; 4],
}

impl BoxGeometry {
    fn primitive(
        self,
        color: [f32; 4],
        border_colors: [[f32; 4]; 4],
        border_styles: [f32; 4],
        kind: BoxPrimitiveKind,
    ) -> BoxPrimitive {
        BoxPrimitive {
            outer_rect: self.outer_rect,
            outer_radii_x: self.outer_radii.horizontal,
            outer_radii_y: self.outer_radii.vertical,
            inner_rect: self.inner_rect,
            inner_radii_x: self.inner_radii.horizontal,
            inner_radii_y: self.inner_radii.vertical,
            border_widths: self.border_widths,
            color,
            border_colors,
            border_styles,
            kind,
        }
    }
}

pub(crate) fn resolve_box_geometry(rect: LayoutRect, paint: &BoxPaint) -> BoxGeometry {
    let outer_radii = resolve_radii(&paint.border_radii, rect);
    let top = resolve_length(paint.border_widths.top, rect.height).min(rect.height);
    let right = resolve_length(paint.border_widths.right, rect.width).min(rect.width);
    let bottom = resolve_length(paint.border_widths.bottom, rect.height).min(rect.height);
    let left = resolve_length(paint.border_widths.left, rect.width).min(rect.width);
    let inner_rect = LayoutRect {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    };
    let inner_radii = ResolvedRadii {
        horizontal: [
            (outer_radii.horizontal[0] - left).max(0.0),
            (outer_radii.horizontal[1] - right).max(0.0),
            (outer_radii.horizontal[2] - right).max(0.0),
            (outer_radii.horizontal[3] - left).max(0.0),
        ],
        vertical: [
            (outer_radii.vertical[0] - top).max(0.0),
            (outer_radii.vertical[1] - top).max(0.0),
            (outer_radii.vertical[2] - bottom).max(0.0),
            (outer_radii.vertical[3] - bottom).max(0.0),
        ],
    };
    BoxGeometry {
        outer_rect: rect,
        outer_radii,
        inner_rect,
        inner_radii,
        border_widths: [top, right, bottom, left],
    }
}

pub(crate) fn resolve_radii(
    radii: &PaintCorners<PaintCornerRadius>,
    rect: LayoutRect,
) -> ResolvedRadii {
    let values = [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ];
    let mut horizontal = values.map(|radius| resolve_length(radius.horizontal, rect.width));
    let mut vertical = values.map(|radius| resolve_length(radius.vertical, rect.height));
    let ratios = [
        ratio(rect.width, horizontal[0] + horizontal[1]),
        ratio(rect.width, horizontal[3] + horizontal[2]),
        ratio(rect.height, vertical[0] + vertical[3]),
        ratio(rect.height, vertical[1] + vertical[2]),
    ];
    let scale = ratios.into_iter().fold(1.0_f32, f32::min);
    for radius in &mut horizontal {
        *radius *= scale;
    }
    for radius in &mut vertical {
        *radius *= scale;
    }
    ResolvedRadii {
        horizontal,
        vertical,
    }
}

fn ratio(available: f32, required: f32) -> f32 {
    if required > available && required > 0.0 {
        available / required
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_protocol::{PaintCorners, PaintEdges};

    fn radius(length: f32, fraction: f32) -> PaintCornerRadius {
        PaintCornerRadius::circular(PaintLengthPercentage { length, fraction })
    }

    fn elliptical(horizontal: f32, vertical: f32) -> PaintCornerRadius {
        PaintCornerRadius {
            horizontal: PaintLengthPercentage {
                length: horizontal,
                fraction: 0.0,
            },
            vertical: PaintLengthPercentage {
                length: vertical,
                fraction: 0.0,
            },
        }
    }

    fn paint(background_color: PaintColor) -> BoxPaint {
        let zero = PaintLengthPercentage::default();
        BoxPaint {
            background_color,
            border_widths: PaintEdges {
                top: PaintLengthPercentage {
                    length: 1.0,
                    fraction: 0.0,
                },
                right: zero,
                bottom: zero,
                left: zero,
            },
            border_colors: PaintEdges {
                top: PaintColor::Named("blue".into()),
                right: PaintColor::default(),
                bottom: PaintColor::default(),
                left: PaintColor::default(),
            },
            border_styles: PaintEdges {
                top: BorderLineStyle::Solid,
                right: BorderLineStyle::None,
                bottom: BorderLineStyle::Hidden,
                left: BorderLineStyle::None,
            },
            border_radii: PaintCorners {
                top_left: PaintCornerRadius::default(),
                top_right: PaintCornerRadius::default(),
                bottom_right: PaintCornerRadius::default(),
                bottom_left: PaintCornerRadius::default(),
            },
        }
    }

    fn lower(rect: LayoutRect, paint: &BoxPaint, opacity: f32) -> Vec<BoxPrimitive> {
        let mut primitives = Vec::new();
        lower_box(rect, paint, opacity, |primitive| primitives.push(primitive));
        primitives
    }

    #[test]
    fn box_lowering_emits_background_and_visible_border_ring() {
        let primitives = lower(
            LayoutRect {
                x: 2.0,
                y: 3.0,
                width: 20.0,
                height: 10.0,
            },
            &paint(PaintColor::Named("red".into())),
            0.5,
        );
        assert_eq!(primitives.len(), 2);
        assert_eq!(primitives[0].kind, BoxPrimitiveKind::Fill);
        assert_eq!(primitives[0].outer_rect.x, 2.0);
        assert!((primitives[0].color[3] - 0.5).abs() < f32::EPSILON);
        assert_eq!(primitives[1].kind, BoxPrimitiveKind::Border);
        assert_eq!(primitives[1].inner_rect.y, 4.0);
        assert_eq!(primitives[1].border_widths, [1.0, 0.0, 0.0, 0.0]);
        assert!(primitives[1].border_colors[0][2] > 0.99);
        assert_eq!(primitives[1].border_colors[1], [0.0; 4]);

        let mut transparent = paint(PaintColor::Named("transparent".into()));
        transparent.border_styles.top = BorderLineStyle::None;
        assert!(lower(LayoutRect::default(), &transparent, 1.0).is_empty());
    }

    #[test]
    fn rounded_radii_resolve_percentages_and_scale_overlaps() {
        let resolved = resolve_radii(
            &PaintCorners {
                top_left: radius(30.0, 0.0),
                top_right: radius(0.0, 0.5),
                bottom_right: radius(30.0, 0.0),
                bottom_left: radius(30.0, 0.0),
            },
            LayoutRect {
                x: 5.0,
                y: 6.0,
                width: 100.0,
                height: 40.0,
            },
        );
        assert!((resolved.horizontal[0] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.horizontal[1] - 100.0 / 3.0).abs() < 0.001);
        assert!((resolved.vertical[1] - 40.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn elliptical_radii_preserve_independent_axes() {
        let resolved = resolve_radii(
            &PaintCorners {
                top_left: elliptical(12.0, 4.0),
                top_right: elliptical(10.0, 6.0),
                bottom_right: elliptical(8.0, 3.0),
                bottom_left: elliptical(7.0, 2.0),
            },
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
            },
        );
        assert_eq!(resolved.horizontal, [12.0, 10.0, 8.0, 7.0]);
        assert_eq!(resolved.vertical, [4.0, 6.0, 3.0, 2.0]);
    }

    #[test]
    fn rounded_border_geometry_preserves_corner_arc_centers() {
        let three = PaintLengthPercentage {
            length: 3.0,
            fraction: 0.0,
        };
        let mut bordered = paint(PaintColor::Named("green".into()));
        bordered.border_widths = PaintEdges {
            top: three,
            right: three,
            bottom: three,
            left: three,
        };
        bordered.border_styles = PaintEdges {
            top: BorderLineStyle::Solid,
            right: BorderLineStyle::Solid,
            bottom: BorderLineStyle::Solid,
            left: BorderLineStyle::Solid,
        };
        bordered.border_colors = PaintEdges {
            top: PaintColor::Named("yellow".into()),
            right: PaintColor::Named("yellow".into()),
            bottom: PaintColor::Named("yellow".into()),
            left: PaintColor::Named("yellow".into()),
        };
        bordered.border_radii = PaintCorners {
            top_left: radius(40.0, 0.0),
            top_right: radius(8.0, 0.0),
            bottom_right: radius(40.0, 0.0),
            bottom_left: radius(8.0, 0.0),
        };
        let geometry = resolve_box_geometry(
            LayoutRect {
                x: 10.0,
                y: 20.0,
                width: 200.0,
                height: 88.0,
            },
            &bordered,
        );

        assert_eq!(geometry.inner_rect.x, 13.0);
        assert_eq!(geometry.inner_rect.y, 23.0);
        assert_eq!(geometry.inner_rect.width, 194.0);
        assert_eq!(geometry.inner_rect.height, 82.0);
        assert_eq!(geometry.outer_radii.horizontal, [40.0, 8.0, 40.0, 8.0]);
        assert_eq!(geometry.inner_radii.horizontal, [37.0, 5.0, 37.0, 5.0]);
        assert_eq!(
            geometry.outer_rect.x + geometry.outer_radii.horizontal[0],
            geometry.inner_rect.x + geometry.inner_radii.horizontal[0]
        );

        let primitives = lower(geometry.outer_rect, &bordered, 1.0);
        assert_eq!(primitives.len(), 2);
        assert_eq!(primitives[1].kind, BoxPrimitiveKind::Border);
        assert!(
            primitives[1]
                .border_colors
                .windows(2)
                .all(|colors| colors[0] == colors[1])
        );
    }
}
