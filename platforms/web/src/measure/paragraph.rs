use unicode_segmentation::UnicodeSegmentation;
use whisker_protocol::{
    InlinePlacement, MeasureTextOverflow, ParagraphMetrics, TextMeasurePayload,
};

use crate::{WebError, js_error, set_style};

mod geometry;

pub(crate) struct SourceSpan {
    reference: web_sys::Element,
    content: web_sys::Element,
    start: usize,
    end: usize,
}

pub(crate) struct ParagraphDom<'a> {
    root: &'a web_sys::Element,
    reference: web_sys::Element,
    spans: Vec<SourceSpan>,
    payload: &'a TextMeasurePayload,
}

impl<'a> ParagraphDom<'a> {
    pub(crate) fn read(
        root: &'a web_sys::Element,
        payload: &'a TextMeasurePayload,
    ) -> Result<Self, WebError> {
        let reference = root
            .query_selector("[data-whisker-paragraph]")
            .map_err(|error| js_error("find paragraph reference", error))?
            .ok_or_else(|| WebError("missing paragraph reference".into()))?;
        let children = reference.children();
        let mut spans = Vec::with_capacity(children.length() as usize);
        for index in 0..children.length() {
            let child = children
                .item(index)
                .ok_or_else(|| WebError("missing text reference".into()))?;
            let Some(start) = child.get_attribute("data-start") else {
                continue;
            };
            let start = start
                .parse::<usize>()
                .map_err(|_| WebError("invalid text offset".into()))?;
            let end = child
                .get_attribute("data-end")
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| WebError("invalid text end".into()))?;
            let content = child
                .first_element_child()
                .ok_or_else(|| WebError("missing text content".into()))?;
            spans.push(SourceSpan {
                reference: child,
                content,
                start,
                end,
            });
        }
        Ok(Self {
            root,
            reference,
            spans,
            payload,
        })
    }

    pub(crate) fn show_prefix(&self, end: usize, ellipsis: bool) -> Result<(), WebError> {
        for span in &self.spans {
            let visible = span.start < end;
            set_style(
                &span.reference,
                "display",
                if visible { "inline" } else { "none" },
            )?;
            if !span.content.has_attribute("data-whisker-inline-node") {
                span.content.set_text_content(Some(if visible {
                    &self.payload.text[span.start..end.min(span.end)]
                } else {
                    ""
                }));
            }
        }
        let token = if let Some(token) = self
            .reference
            .query_selector("[data-whisker-ellipsis]")
            .map_err(|error| js_error("find paragraph ellipsis", error))?
        {
            token
        } else {
            let token = self
                .root
                .owner_document()
                .ok_or_else(|| WebError("missing document".into()))?
                .create_element("span")
                .map_err(|error| js_error("create paragraph ellipsis", error))?;
            token
                .set_attribute("data-whisker-ellipsis", "")
                .map_err(|error| js_error("identify paragraph ellipsis", error))?;
            self.reference
                .append_child(&token)
                .map_err(|error| js_error("append paragraph ellipsis", error))?;
            token
        };
        if let Some(attachment) = self
            .payload
            .attachments
            .iter()
            .find(|attachment| attachment.truncation)
        {
            token.set_text_content(None);
            set_style(
                &token,
                "display",
                if end < self.payload.text.len() {
                    "inline-block"
                } else {
                    "none"
                },
            )?;
            set_style(
                &token,
                "width",
                &format!("min({}px, 100%)", attachment.size.width),
            )?;
            set_style(&token, "height", &format!("{}px", attachment.size.height))?;
            let alignment = match attachment.alignment {
                whisker_protocol::InlineAlignment::Top => "top".into(),
                whisker_protocol::InlineAlignment::Middle => "middle".into(),
                whisker_protocol::InlineAlignment::Bottom => "bottom".into(),
                whisker_protocol::InlineAlignment::Offset(shift) => {
                    format!("{}px", attachment.baseline + shift - attachment.size.height)
                }
                whisker_protocol::InlineAlignment::Baseline => {
                    format!("{}px", attachment.baseline - attachment.size.height)
                }
            };
            set_style(&token, "vertical-align", &alignment)?;
            token
                .set_attribute(
                    "data-whisker-truncation",
                    &attachment.node.get().to_string(),
                )
                .map_err(|error| js_error("identify truncation token", error))?;
        } else {
            token.set_text_content(Some(if ellipsis { "…" } else { "" }));
        }
        Ok(())
    }

    pub(crate) fn prepare(&self) -> Result<usize, WebError> {
        let max_lines = match self.payload.max_lines {
            Some(lines) => lines,
            None if self.payload.wrap == whisker_protocol::MeasureTextWrap::NoWrap => 1,
            None => return Ok(self.payload.text.len()),
        };
        if self.fits(max_lines)? {
            return Ok(self.payload.text.len());
        }
        let boundaries: Vec<_> = self
            .payload
            .text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .chain(std::iter::once(self.payload.text.len()))
            .collect();
        let ellipsis = self.payload.overflow == MeasureTextOverflow::Ellipsis;
        self.show_prefix(0, ellipsis)?;
        let mut best = 0;
        let mut low = 0;
        let mut high = boundaries.len();
        for _ in 0..60 {
            if low >= high {
                break;
            }
            let middle = low + (high - low) / 2;
            self.show_prefix(boundaries[middle], ellipsis)?;
            if self.fits(max_lines)? {
                best = boundaries[middle];
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        self.show_prefix(best, ellipsis)?;
        self.fits(max_lines)?;
        Ok(best)
    }

    fn fits(&self, max_lines: u32) -> Result<bool, WebError> {
        let rects = geometry::rects(Some(self.reference.get_client_rects()));
        let mut tops: Vec<_> = rects.iter().map(|rect| rect.top()).collect();
        tops.sort_by(f64::total_cmp);
        tops.dedup();
        if tops.len() > max_lines as usize {
            return Ok(false);
        }
        let root = self.root.get_bounding_client_rect();
        let indent = self.payload.indent.resolve(root.width() as f32).min(0.0) as f64;
        Ok(rects.iter().all(|rect| {
            rect.left() >= root.left() + indent - 0.01
                && rect.right() <= root.right() - indent + 0.01
        }))
    }

    pub(crate) fn metrics(
        &self,
        visible_end: usize,
        first_baseline: f32,
    ) -> Result<ParagraphMetrics, WebError> {
        geometry::measure(self, visible_end, first_baseline)
    }

    pub(crate) fn placements(&self, visible_end: usize) -> Result<Vec<InlinePlacement>, WebError> {
        let root = self.root.get_bounding_client_rect();
        self.payload
            .attachments
            .iter()
            .map(|attachment| {
                let origin = if attachment.truncation {
                    if visible_end < self.payload.text.len() {
                        let token = self
                            .reference
                            .query_selector("[data-whisker-truncation]")
                            .map_err(|error| js_error("find truncation token", error))?
                            .ok_or_else(|| WebError("missing truncation token".into()))?;
                        let bounds = token.get_bounding_client_rect();
                        Some([
                            (bounds.left() - root.left()) as f32,
                            (bounds.top() - root.top()) as f32,
                        ])
                    } else {
                        None
                    }
                } else if (attachment.range.start as usize) < visible_end {
                    let span = self
                        .spans
                        .iter()
                        .find(|span| span.start == attachment.range.start as usize)
                        .ok_or_else(|| WebError("missing inline source span".into()))?;
                    let bounds = span.content.get_bounding_client_rect();
                    Some([
                        (bounds.left() - root.left()) as f32,
                        (bounds.top() - root.top()) as f32,
                    ])
                } else {
                    None
                };
                Ok(InlinePlacement {
                    node: attachment.node,
                    origin,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;
    use whisker_protocol::{MeasureLineHeight, TextByteRange, TextMeasureRun, TextMeasureStyle};

    #[wasm_bindgen_test]
    fn custom_token_is_clipped_and_excluded_from_source_geometry() {
        use whisker_protocol::*;
        let document = web_sys::window().unwrap().document().unwrap();
        for width in [120.0, 20.0] {
            let root = document.create_element("div").unwrap();
            let text = "Prefix 🧑‍🚀 followed by more text to truncate.".to_string();
            let end = text.len() as u32;
            let payload = TextMeasurePayload {
                text,
                style: TextMeasureStyle::default(),
                runs: vec![],
                attachments: vec![InlineAttachment {
                    truncation: true,
                    label: None,
                    node: NodeId::new(22).unwrap(),
                    range: TextByteRange { start: end, end },
                    size: MeasuredSize::new(42.0, 20.0),
                    baseline: 16.0,
                    alignment: InlineAlignment::Baseline,
                }],
                locale: None,
                direction: MeasureTextDirection::Auto,
                alignment: MeasureTextAlignment::Start,
                indent: Default::default(),
                wrap: MeasureTextWrap::Wrap,
                word_break: Default::default(),
                max_lines: Some(1),
                overflow: MeasureTextOverflow::Clip,
            };
            crate::paint::text::apply_metrics_style(&root, &payload).unwrap();
            crate::paint::text::apply_content(&root, &payload, &[]).unwrap();
            set_style(&root, "width", &format!("{width}px")).unwrap();
            document.body().unwrap().append_child(&root).unwrap();
            let dom = ParagraphDom::read(&root, &payload).unwrap();
            let end = dom.prepare().unwrap();
            let geometry = dom.metrics(end, 16.0).unwrap();
            assert!(geometry.validate(&payload.text), "{geometry:?}");
            assert_eq!(geometry.lines.len(), 1);
            assert!(geometry.lines[0].ellipsis_count > 0);
            assert!(dom.placements(end).unwrap()[0].origin.is_some());
            let token = root
                .query_selector("[data-whisker-truncation]")
                .unwrap()
                .unwrap();
            assert!(token.get_bounding_client_rect().width() <= width);
            root.remove();
        }
    }

    #[wasm_bindgen_test]
    fn paragraph_geometry_keeps_mixed_fonts_and_clamped_sources_distinct() {
        let document = web_sys::window().unwrap().document().unwrap();
        for direction in ["ltr", "rtl"] {
            let root = document.create_element("div").unwrap();
            let mut payload = TextMeasurePayload {
                text: "Small 🦀 BIG 日本語 wraps across several lines.".into(),
                style: TextMeasureStyle {
                    font_size: 20.0,
                    line_height: MeasureLineHeight::LogicalPixels(24.0),
                    ..Default::default()
                },
                max_lines: Some(2),
                overflow: MeasureTextOverflow::Ellipsis,
                runs: Vec::new(),
                attachments: Vec::new(),
                locale: None,
                direction: whisker_protocol::MeasureTextDirection::Auto,
                alignment: whisker_protocol::MeasureTextAlignment::Start,
                indent: Default::default(),
                wrap: whisker_protocol::MeasureTextWrap::Wrap,
                word_break: Default::default(),
            };
            payload.runs = vec![
                TextMeasureRun {
                    alignment: Default::default(),
                    range: TextByteRange { start: 0, end: 11 },
                    style: payload.style.clone(),
                },
                TextMeasureRun {
                    alignment: Default::default(),
                    range: TextByteRange { start: 11, end: 14 },
                    style: TextMeasureStyle {
                        font_size: 36.0,
                        ..payload.style.clone()
                    },
                },
                TextMeasureRun {
                    alignment: Default::default(),
                    range: TextByteRange {
                        start: 14,
                        end: payload.text.len() as u32,
                    },
                    style: payload.style.clone(),
                },
            ];
            crate::paint::text::apply_metrics_style(&root, &payload).unwrap();
            crate::paint::text::apply_content(&root, &payload, &[]).unwrap();
            set_style(&root, "width", "140px").unwrap();
            set_style(&root, "direction", direction).unwrap();
            document.body().unwrap().append_child(&root).unwrap();
            let dom = ParagraphDom::read(&root, &payload).unwrap();
            let end = dom.prepare().unwrap();
            let geometry = dom.metrics(end, 20.0).unwrap();
            assert!(geometry.validate(&payload.text), "{geometry:?}");
            assert_eq!(geometry.lines.len(), 2, "{geometry:?}");
            assert!(end < payload.text.len());
            let visible_units = payload.text[..end].encode_utf16().count() as u32;
            assert!(
                geometry
                    .fragments
                    .iter()
                    .all(|fragment| fragment.range.end <= visible_units)
            );
            assert!(geometry.lines.last().unwrap().ellipsis_count > 0);
            root.remove();
        }
    }
}
