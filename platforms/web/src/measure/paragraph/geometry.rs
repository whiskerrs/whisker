use unicode_segmentation::UnicodeSegmentation;
use whisker_protocol::{LayoutRect, ParagraphLine, ParagraphMetrics, TextFragment, TextRange};

use super::ParagraphDom;
use crate::{WebError, js_error};

pub(super) fn rects(list: Option<web_sys::DomRectList>) -> Vec<web_sys::DomRect> {
    list.map(|list| {
        (0..list.length())
            .filter_map(|index| list.item(index))
            .collect()
    })
    .unwrap_or_default()
}

fn relative(rect: &web_sys::DomRect, root: &web_sys::DomRect) -> LayoutRect {
    LayoutRect {
        x: (rect.x() - root.x()) as f32,
        y: (rect.y() - root.y()) as f32,
        width: rect.width() as f32,
        height: rect.height() as f32,
    }
}

pub(super) fn measure(
    dom: &ParagraphDom<'_>,
    visible_end: usize,
    first_baseline: f32,
) -> Result<ParagraphMetrics, WebError> {
    let root = dom.root.get_bounding_client_rect();
    let references = rects(Some(dom.reference.get_client_rects()));
    let mut tops: Vec<_> = references.iter().map(|rect| rect.top()).collect();
    tops.sort_by(f64::total_cmp);
    tops.dedup();
    let baseline_delta =
        first_baseline as f64 + root.top() - tops.first().copied().unwrap_or(root.top());
    let mut lines: Vec<_> = tops
        .iter()
        .map(|top| ParagraphLine {
            range: TextRange {
                start: u32::MAX,
                end: 0,
            },
            ellipsis_count: 0,
            bounds: LayoutRect {
                x: 0.0,
                y: (*top - root.top()) as f32,
                width: 0.0,
                height: 0.0,
            },
            baseline: (*top + baseline_delta - root.top()) as f32,
        })
        .collect();
    let mut fragments = Vec::new();
    for span in &dom.spans {
        if span.start >= visible_end {
            continue;
        }
        let end = span.end.min(visible_end);
        let outer = rects(Some(span.reference.get_client_rects()));
        let inner = rects(Some(span.content.get_client_rects()));
        if outer.len() != inner.len() {
            return Err(WebError("ambiguous browser paragraph fragments".into()));
        }
        let source_range = TextRange::from_utf8(&dom.payload.text, span.start..end)
            .ok_or_else(|| WebError("invalid paragraph source interval".into()))?;
        let mut pieces = Vec::new();
        if span.content.has_attribute("data-whisker-inline-node") {
            pieces.extend(inner.iter().map(|rect| (source_range, rect.clone())));
        } else if let Some(node) = span.content.first_child() {
            let text = &dom.payload.text[span.start..end];
            let mut units = 0;
            let boundaries: Vec<_> = text
                .graphemes(true)
                .map(|grapheme| {
                    let start = units;
                    units += grapheme.encode_utf16().count() as u32;
                    start
                })
                .chain(std::iter::once(source_range.end - source_range.start))
                .collect();
            let range = dom
                .root
                .owner_document()
                .ok_or_else(|| WebError("missing paragraph document".into()))?
                .create_range()
                .map_err(|error| js_error("create paragraph range", error))?;
            subdivide(&range, &node, &boundaries, source_range.start, &mut pieces)?;
        }
        for (source, rect) in pieces {
            let index = inner
                .iter()
                .position(|part| {
                    rect.left() >= part.left() - 0.01
                        && rect.right() <= part.right() + 0.01
                        && rect.top() == part.top()
                        && rect.bottom() == part.bottom()
                })
                .ok_or_else(|| WebError("browser source fragment has no line reference".into()))?;
            let line_index = tops
                .binary_search_by(|top| top.total_cmp(&outer[index].top()))
                .map_err(|_| WebError("missing browser paragraph line".into()))?;
            let bounds = relative(&rect, &root);
            let line = &mut lines[line_index];
            if line.range.start == u32::MAX {
                line.bounds = bounds;
            } else {
                line.bounds = union(line.bounds, bounds);
            }
            line.range.start = line.range.start.min(source.start);
            line.range.end = line.range.end.max(source.end);
            if bounds.width > 0.0 && bounds.height > 0.0 {
                fragments.push(TextFragment {
                    range: source,
                    bounds,
                });
            }
        }
    }
    if visible_end < dom.payload.text.len() {
        if let Some(token) = dom
            .reference
            .query_selector("[data-whisker-ellipsis]")
            .map_err(|error| js_error("find truncation geometry", error))?
        {
            let bounds = relative(&token.get_bounding_client_rect(), &root);
            if let Some(line) = lines.last_mut() {
                if line.bounds.height == 0.0 {
                    line.bounds = bounds;
                } else {
                    line.bounds = union(line.bounds, bounds);
                }
            } else {
                lines.push(ParagraphLine {
                    range: TextRange::default(),
                    ellipsis_count: 0,
                    bounds,
                    baseline: first_baseline,
                });
            }
        }
    }
    let mut end = 0;
    for line in &mut lines {
        if line.range.start == u32::MAX {
            line.range = TextRange { start: end, end };
        }
        end = line.range.end;
    }
    if visible_end < dom.payload.text.len() {
        let visible_units = dom.payload.text[..visible_end].encode_utf16().count() as u32;
        let total_units = dom.payload.text.encode_utf16().count() as u32;
        if let Some(last) = lines.last_mut() {
            last.range.end = total_units;
            last.ellipsis_count = total_units - visible_units;
        }
    }
    Ok(ParagraphMetrics { lines, fragments })
}

fn subdivide(
    range: &web_sys::Range,
    node: &web_sys::Node,
    boundaries: &[u32],
    source: u32,
    output: &mut Vec<(TextRange, web_sys::DomRect)>,
) -> Result<(), WebError> {
    if boundaries.len() < 2 {
        return Ok(());
    }
    let start = boundaries[0];
    let end = boundaries[boundaries.len() - 1];
    range
        .set_start(node, start)
        .map_err(|error| js_error("set paragraph range start", error))?;
    range
        .set_end(node, end)
        .map_err(|error| js_error("set paragraph range end", error))?;
    let rectangles = rects(range.get_client_rects());
    if rectangles.len() <= 1 || boundaries.len() == 2 {
        output.extend(rectangles.into_iter().map(|rect| {
            (
                TextRange {
                    start: source + start,
                    end: source + end,
                },
                rect,
            )
        }));
    } else {
        let middle = boundaries.len() / 2;
        subdivide(range, node, &boundaries[..=middle], source, output)?;
        subdivide(range, node, &boundaries[middle..], source, output)?;
    }
    Ok(())
}

fn union(a: LayoutRect, b: LayoutRect) -> LayoutRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    LayoutRect {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}
