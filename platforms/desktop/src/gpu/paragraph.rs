use parley::PositionedLayoutItem;
use whisker_protocol::{BoxPaint, LayoutRect, TextContent, TextDecorationThickness};

use super::geometry::text_decoration_rects;
use crate::paint::box_paint::{BoxPrimitive, lower_box, solid_rect_primitive};
use crate::paint::color::gpu_color;
use crate::text::paragraph::PreparedParagraph;

pub(super) fn paragraph_primitives(
    paragraph: &PreparedParagraph,
    content: &TextContent,
    origin: LayoutRect,
    opacity: f32,
    mut emit: impl FnMut(BoxPrimitive),
) {
    for (line, geometry) in paragraph.layout.lines().zip(&paragraph.lines) {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                continue;
            };
            let run = glyphs.run();
            let metrics = run.metrics();
            let style = glyphs
                .style()
                .brush
                .checked_sub(1)
                .and_then(|index| content.runs.get(index as usize));
            let baseline = origin.y + geometry.baseline
                - geometry.run_shift(
                    crate::text::paragraph::run_alignment(&content.payload, glyphs.style().brush),
                    metrics,
                );
            let fragment = LayoutRect {
                x: origin.x + glyphs.offset(),
                y: baseline - metrics.ascent,
                width: glyphs.advance(),
                height: metrics.ascent + metrics.descent,
            };
            if let Some(style) = style
                && let Some(background) = &style.background
            {
                let paint = BoxPaint {
                    background_color: background.clone(),
                    border_radii: style.background_radii.clone(),
                    ..BoxPaint::default()
                };
                lower_box(fragment, &paint, opacity, &mut emit);
            }
            let decoration =
                style.map_or(&content.paint.decoration, |style| &style.paint.decoration);
            let thickness = match decoration.thickness {
                TextDecorationThickness::Auto => (run.font_size() / 16.0).max(1.0),
                TextDecorationThickness::FromFont => metrics.underline_size.max(0.0),
                TextDecorationThickness::Length(value) => value,
            };
            if thickness <= 0.0 || fragment.width <= 0.0 {
                continue;
            }
            let color = gpu_color(&decoration.color, opacity);
            for (enabled, y) in [
                (
                    decoration.lines.underline,
                    baseline - metrics.underline_offset,
                ),
                (
                    decoration.lines.line_through,
                    baseline - metrics.strikethrough_offset,
                ),
                (decoration.lines.overline, fragment.y),
            ] {
                if !enabled {
                    continue;
                }
                for rect in text_decoration_rects(
                    fragment.x,
                    fragment.width,
                    y,
                    thickness,
                    decoration.style,
                ) {
                    emit(solid_rect_primitive(rect, color));
                }
            }
        }
    }
}
