use super::*;

pub(super) fn paragraph_geometry(
    payload: &TextMeasurePayload,
    layout: &Layout<u32>,
    lines: &[ParagraphLine],
    offsets: &BTreeMap<u32, usize>,
    attachments: &[(usize, LayoutRect)],
) -> whisker_protocol::ParagraphMetrics {
    use whisker_protocol::{ParagraphMetrics, TextFragment, TextRange};
    let mut source = BTreeMap::<usize, (u32, u32)>::new();
    let mut units = 0;
    for (byte, character) in payload.text.char_indices() {
        source
            .entry(offsets[&(byte as u32)])
            .and_modify(|range| range.1 = units)
            .or_insert((units, units));
        units += character.len_utf16() as u32;
    }
    source
        .entry(offsets[&(payload.text.len() as u32)])
        .and_modify(|range| range.1 = units)
        .or_insert((units, units));
    source.entry(0).or_insert((0, 0));
    let original = |range: std::ops::Range<usize>| -> Option<TextRange> {
        let start = source.get(&range.start)?.1;
        let end = source.get(&range.end)?.0;
        (start <= end).then_some(TextRange { start, end })
    };
    let mut geometry = ParagraphMetrics::default();
    for (line, metrics) in layout.lines().zip(lines) {
        let mut range = original(line.text_range()).unwrap_or_default();
        for (index, rect) in attachments {
            if rect.y >= metrics.top && rect.y < metrics.top + metrics.height {
                let attachment = &payload.attachments[*index];
                if let Some(native) = attachment.range.utf16(&payload.text) {
                    range.start = range.start.min(native.start);
                    range.end = range.end.max(native.end);
                }
            }
        }
        geometry.lines.push(whisker_protocol::ParagraphLine {
            range,
            ellipsis_count: 0,
            bounds: LayoutRect {
                x: line.metrics().offset,
                y: metrics.top,
                width: line.metrics().advance,
                height: metrics.height,
            },
            baseline: metrics.baseline,
        });
        let mut seen = std::collections::BTreeSet::new();
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                continue;
            };
            let run = glyphs.run();
            let source_range = run.text_range();
            if !seen.insert((source_range.start, source_range.end)) {
                continue;
            }
            let mut x = glyphs.offset();
            let mut previous_brush = None;
            for cluster in run.visual_clusters() {
                let width = cluster.advance();
                if let Some(range) =
                    original(cluster.text_range()).filter(|range| range.start < range.end)
                {
                    let brush = cluster.first_style().brush;
                    if previous_brush == Some(brush)
                        && let Some(previous) = geometry.fragments.last_mut()
                        && (previous.range.end == range.start || previous.range.start == range.end)
                    {
                        previous.range.start = previous.range.start.min(range.start);
                        previous.range.end = previous.range.end.max(range.end);
                        previous.bounds.width += width;
                    } else {
                        geometry.fragments.push(TextFragment {
                            range,
                            bounds: LayoutRect {
                                x,
                                y: metrics.baseline
                                    - metrics
                                        .run_shift(run_alignment(payload, brush), run.metrics())
                                    - run.metrics().ascent,
                                width,
                                height: run.metrics().ascent + run.metrics().descent,
                            },
                        });
                    }
                    previous_brush = Some(brush);
                }
                x += width;
            }
        }
    }
    geometry
}

impl PreparedParagraph {
    pub(crate) fn position_at(&self, payload: &TextMeasurePayload, point: [f32; 2]) -> Option<u32> {
        for (index, rect) in &self.attachments {
            if point[0] >= rect.x
                && point[0] <= rect.x + rect.width
                && point[1] >= rect.y
                && point[1] <= rect.y + rect.height
            {
                let range = payload.attachments[*index].range.utf16(&payload.text)?;
                return Some(if point[0] < rect.x + rect.width / 2.0 {
                    range.start
                } else {
                    range.end
                });
            }
        }
        let line = self
            .lines
            .iter()
            .find(|line| point[1] < line.top + line.height)
            .or(self.lines.last())?;
        let cursor = parley::Cursor::from_point(&self.layout, point[0], point[1] - line.shift);
        let source = self
            .source_offsets
            .iter()
            .rev()
            .find(|(byte, offset)| {
                **offset <= cursor.index() && (**byte as usize) <= self.visible_end
            })?
            .0;
        Some(payload.text[..*source as usize].encode_utf16().count() as u32)
    }

    pub(crate) fn selection_rects(
        &self,
        payload: &TextMeasurePayload,
        range: whisker_protocol::TextRange,
    ) -> Option<Vec<LayoutRect>> {
        use parley::{Affinity, Cursor, Selection};
        let bytes = range.to_utf8(&payload.text)?;
        let start = bytes.start.min(self.visible_end);
        let end = bytes.end.min(self.visible_end);
        let selection = Selection::new(
            Cursor::from_byte_index(
                &self.layout,
                *self.source_offsets.get(&(start as u32))?,
                Affinity::Downstream,
            ),
            Cursor::from_byte_index(
                &self.layout,
                *self.source_offsets.get(&(end as u32))?,
                Affinity::Upstream,
            ),
        );
        let mut rects = Vec::new();
        selection.geometry_with(&self.layout, |bounds, line| {
            if let Some(line) = self.lines.get(line) {
                rects.push(LayoutRect {
                    x: bounds.x0 as f32,
                    y: line.top,
                    width: (bounds.x1 - bounds.x0) as f32,
                    height: line.height,
                });
            }
        });
        for (index, rect) in &self.attachments {
            let attachment = &payload.attachments[*index];
            if (attachment.range.start as usize) < end && (attachment.range.end as usize) > start {
                rects.push(*rect);
            }
        }
        Some(rects)
    }
}
