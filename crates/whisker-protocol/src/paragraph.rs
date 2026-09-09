//! Styled ranges and inline boxes within a single shaped paragraph.

use std::ops::Range;

use crate::{
    MeasuredSize, MeasurementPayloadError, NodeId, PaintColor, TextMeasurePayload,
    TextMeasureStyle, TextPaint, TextSpanId,
};

/// A half-open UTF-8 byte range within a paragraph's logical string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextByteRange {
    /// Inclusive byte offset.
    pub start: u32,
    /// Exclusive byte offset.
    pub end: u32,
}

impl TextByteRange {
    /// Returns a slice only when both endpoints are valid UTF-8 boundaries.
    pub fn slice(self, text: &str) -> Option<&str> {
        text.get(self.start as usize..self.end as usize)
    }

    /// Converts the range to native UTF-16 code-unit offsets.
    pub fn utf16(self, text: &str) -> Option<Range<u32>> {
        let selected = self.slice(text)?;
        let prefix = &text[..self.start as usize];
        let start = prefix.encode_utf16().count() as u32;
        let length = selected.encode_utf16().count() as u32;
        Some(start..start + length)
    }
}

/// A half-open range in UTF-16 code units, as used by native text APIs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextRange {
    /// Inclusive UTF-16 offset.
    pub start: u32,
    /// Exclusive UTF-16 offset.
    pub end: u32,
}

impl TextRange {
    /// Converts a valid Rust string slice range to native text offsets.
    pub fn from_utf8(text: &str, range: Range<usize>) -> Option<Self> {
        let bytes = TextByteRange {
            start: range.start.try_into().ok()?,
            end: range.end.try_into().ok()?,
        };
        let native = bytes.utf16(text)?;
        Some(Self {
            start: native.start,
            end: native.end,
        })
    }

    /// Converts native offsets to a Rust string slice range, rejecting split surrogates.
    pub fn to_utf8(self, text: &str) -> Option<Range<usize>> {
        if self.start > self.end {
            return None;
        }
        let mut units = 0;
        let mut start = None;
        let mut end = None;
        for (byte, character) in text.char_indices() {
            if units == self.start {
                start = Some(byte);
            }
            if units == self.end {
                end = Some(byte);
                break;
            }
            units += character.len_utf16() as u32;
        }
        if units == self.start {
            start = start.or(Some(text.len()));
        }
        if units == self.end {
            end = end.or(Some(text.len()));
        }
        Some(start?..end?)
    }
}

/// One visible line in the accepted paragraph layout.
#[derive(Clone, Debug, PartialEq)]
pub struct ParagraphLine {
    /// Source offsets before ellipsis replacement.
    pub range: TextRange,
    /// Number of UTF-16 units replaced by the ellipsis.
    pub ellipsis_count: u32,
    /// Line box relative to the paragraph content origin.
    pub bounds: crate::LayoutRect,
    /// Baseline relative to the paragraph content origin.
    pub baseline: f32,
}

/// A visually contiguous portion of a source range.
#[derive(Clone, Debug, PartialEq)]
pub struct TextFragment {
    /// Source range corresponding to this rectangle.
    pub range: TextRange,
    /// Visible rectangle relative to the paragraph content origin.
    pub bounds: crate::LayoutRect,
}

/// Geometry shared by paragraph notification and logical span hit testing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphMetrics {
    /// Visible lines in reading order.
    pub lines: Vec<ParagraphLine>,
    /// Visible source fragments; bidirectional runs may have multiple rectangles.
    pub fragments: Vec<TextFragment>,
}

impl ParagraphMetrics {
    /// Checks source offsets and finite geometry before retaining Host results.
    pub fn validate(&self, text: &str) -> bool {
        let rect = |rect: crate::LayoutRect| {
            rect.x.is_finite()
                && rect.y.is_finite()
                && rect.width.is_finite()
                && rect.height.is_finite()
                && rect.width >= 0.0
                && rect.height >= 0.0
        };
        let mut offset = 0;
        let mut boundaries = std::collections::HashSet::from([0]);
        for character in text.chars() {
            offset += character.len_utf16() as u32;
            boundaries.insert(offset);
        }
        let range = |range: TextRange| {
            range.start <= range.end
                && boundaries.contains(&range.start)
                && boundaries.contains(&range.end)
        };
        self.lines.iter().all(|line| {
            range(line.range)
                && line.ellipsis_count <= line.range.end - line.range.start
                && rect(line.bounds)
                && line.baseline.is_finite()
        }) && self.fragments.iter().all(|fragment| {
            fragment.range.start < fragment.range.end
                && range(fragment.range)
                && rect(fragment.bounds)
        })
    }
}

/// Metric-affecting attributes for a contiguous portion of a paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasureRun {
    /// Baseline placement within the containing line.
    pub alignment: InlineAlignment,
    /// Nonempty range in the logical string.
    pub range: TextByteRange,
    /// Fully resolved attributes, independent of the logical parent tree.
    pub style: TextMeasureStyle,
}

/// Resolved alignment of an indivisible inline box relative to its line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum InlineAlignment {
    /// Align the box's baseline with the surrounding text baseline.
    #[default]
    Baseline,
    /// Align the box with the top of the line box.
    Top,
    /// Align the box center with the surrounding font's x-height center.
    Middle,
    /// Align the box with the bottom of the line box.
    Bottom,
    /// Raise the baseline by a signed number of logical pixels.
    Offset(f32),
}

/// A measured View or Image occupying one object-replacement character.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineAttachment {
    /// Replacement used only when the paragraph overflows.
    pub truncation: bool,
    /// Explicit copy text; absent labels omit decorative objects.
    pub label: Option<String>,
    /// Source element retained by the Runtime.
    pub node: NodeId,
    /// Range containing exactly one U+FFFC character.
    pub range: TextByteRange,
    /// Margin-box extent used by paragraph line breaking.
    pub size: MeasuredSize,
    /// Baseline measured from the margin-box top.
    pub baseline: f32,
    /// Placement relative to the surrounding line.
    pub alignment: InlineAlignment,
}

/// Final margin-box position of an inline child, or suppression by truncation.
#[derive(Clone, Debug, PartialEq)]
pub struct InlinePlacement {
    /// Child from the corresponding paragraph measurement request.
    pub node: NodeId,
    /// Logical coordinates relative to the paragraph content origin.
    pub origin: Option<[f32; 2]>,
}

/// Paint and event identity for a contiguous portion of a paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct TextPaintRun {
    /// Nonempty range in the logical string.
    pub range: TextByteRange,
    /// Logical element used for event propagation.
    pub span: TextSpanId,
    /// Logical inline ancestor activated by an assistive technology.
    pub action: Option<TextSpanId>,
    /// Resolved glyph decoration and color.
    pub paint: TextPaint,
    /// Optional background painted behind each visual fragment.
    pub background: Option<PaintColor>,
    /// Corner radii resolved independently against each visual fragment.
    pub background_radii: crate::PaintCorners<crate::PaintCornerRadius>,
}

impl TextMeasurePayload {
    pub(crate) fn validate_ranges(&self) -> Result<(), MeasurementPayloadError> {
        let invalid = MeasurementPayloadError::InvalidTextRange;
        let mut previous = 0;
        for run in &self.runs {
            if run.range.start < previous
                || run.range.start >= run.range.end
                || run.range.slice(&self.text).is_none()
            {
                return Err(invalid);
            }
            if matches!(run.alignment, InlineAlignment::Offset(value) if !value.is_finite()) {
                return Err(invalid);
            }
            run.style.validate()?;
            previous = run.range.end;
        }
        previous = 0;
        let mut nodes = std::collections::HashSet::new();
        let mut truncation = false;
        for attachment in &self.attachments {
            let valid_range = if attachment.truncation {
                let valid = !truncation
                    && attachment.range.start == self.text.len() as u32
                    && attachment.range.end == attachment.range.start;
                truncation = true;
                valid
            } else {
                attachment.range.slice(&self.text) == Some("\u{fffc}")
            };
            if attachment.range.start < previous
                || !valid_range
                || !attachment.size.is_valid()
                || !attachment.baseline.is_finite()
                || matches!(attachment.alignment, InlineAlignment::Offset(value) if !value.is_finite())
                || !nodes.insert(attachment.node)
            {
                return Err(invalid);
            }
            previous = attachment.range.end;
        }
        Ok(())
    }
}

impl crate::MeasurementMetrics {
    /// Validates geometry against the exact inline children in its request.
    pub fn matches_payload(&self, payload: &crate::MeasurementPayload) -> bool {
        if !self.is_valid() {
            return false;
        }
        let attachments = match payload {
            crate::MeasurementPayload::Text(text) => {
                if self
                    .paragraph
                    .as_ref()
                    .is_some_and(|paragraph| !paragraph.validate(&text.text))
                {
                    return false;
                }
                text.attachments.as_slice()
            }
            _ if self.paragraph.is_some() => return false,
            _ => &[],
        };
        if attachments.len() != self.inline_placements.len() {
            return false;
        }
        let mut seen = std::collections::HashSet::new();
        self.inline_placements.iter().all(|placement| {
            seen.insert(placement.node)
                && attachments
                    .iter()
                    .any(|attachment| attachment.node == placement.node)
        })
    }
}

impl TextPaintRun {
    pub(crate) fn validate(&self, text: &str) -> bool {
        self.range.start < self.range.end
            && self.range.slice(text).is_some()
            && self.paint.validate()
            && self
                .background
                .as_ref()
                .is_none_or(|color| color.is_valid())
            && [
                self.background_radii.top_left,
                self.background_radii.top_right,
                self.background_radii.bottom_right,
                self.background_radii.bottom_left,
            ]
            .into_iter()
            .all(crate::PaintCornerRadius::is_valid)
    }
}

impl TextMeasurePayload {
    /// Returns copy text with explicitly labeled objects and no generated truncation token.
    pub fn copy_text(&self, range: TextRange) -> Option<String> {
        let bytes = range.to_utf8(&self.text)?;
        let mut result = String::new();
        let mut cursor = bytes.start;
        for attachment in self
            .attachments
            .iter()
            .filter(|attachment| !attachment.truncation)
        {
            let start = attachment.range.start as usize;
            let end = attachment.range.end as usize;
            if start < bytes.start || end > bytes.end {
                continue;
            }
            result.push_str(self.text.get(cursor..start)?);
            if let Some(label) = &attachment.label {
                result.push_str(label);
            }
            cursor = end;
        }
        result.push_str(self.text.get(cursor..bytes.end)?);
        Some(result)
    }
}

impl crate::TextContent {
    /// Visible inline actions in reading order, without duplicating paragraph text nodes.
    pub fn accessible_actions(&self) -> Vec<(TextSpanId, String)> {
        let mut actions: Vec<(TextSpanId, String)> = Vec::new();
        let limit = self.visible_source_end();
        for run in &self.runs {
            let Some(action) = run.action else {
                continue;
            };
            let Some(range) = run.range.utf16(&self.payload.text) else {
                continue;
            };
            let Some(bytes) = (TextRange {
                start: range.start.min(limit),
                end: range.end.min(limit),
            })
            .to_utf8(&self.payload.text) else {
                continue;
            };
            let label = self.payload.text[bytes].replace('\u{fffc}', "");
            if label.is_empty() {
                continue;
            }
            if let Some((_, previous)) = actions.iter_mut().find(|(span, _)| *span == action) {
                previous.push_str(&label);
            } else {
                actions.push((action, label));
            }
        }
        actions
    }

    /// Announced paragraph text; embedded controls provide their own semantics.
    pub fn accessible_text(&self) -> String {
        let range = TextRange {
            start: 0,
            end: self.visible_source_end(),
        };
        range
            .to_utf8(&self.payload.text)
            .map_or_else(String::new, |range| {
                self.payload.text[range].replace('\u{fffc}', "")
            })
    }

    fn visible_source_end(&self) -> u32 {
        self.paragraph
            .as_ref()
            .and_then(|paragraph| paragraph.lines.last())
            .map_or_else(
                || self.payload.text.encode_utf16().count() as u32,
                |line| line.range.end - line.ellipsis_count,
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_ranges_round_trip_every_unicode_boundary() {
        let text = "a🦀日本e\u{301}\u{fffc}";
        let boundaries = text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .collect::<Vec<_>>();
        for &start in &boundaries {
            for &end in boundaries.iter().filter(|&&end| end >= start) {
                let range = TextRange::from_utf8(text, start..end).unwrap();
                assert_eq!(range.to_utf8(text), Some(start..end));
            }
        }
        assert_eq!(TextRange { start: 2, end: 3 }.to_utf8(text), None);
        assert_eq!(TextRange { start: 1, end: 2 }.to_utf8(text), None);
        assert_eq!(TextRange { start: 3, end: 1 }.to_utf8(text), None);
        assert_eq!(TextRange { start: 0, end: 100 }.to_utf8(text), None);
        assert_eq!(TextRange::from_utf8(text, 2..5), None);
        assert_eq!(TextRange::default().to_utf8(""), Some(0..0));
    }

    #[test]
    fn paragraph_geometry_rejects_invalid_host_offsets_and_coordinates() {
        let mut geometry = ParagraphMetrics {
            lines: vec![ParagraphLine {
                range: TextRange { start: 0, end: 3 },
                ellipsis_count: 0,
                bounds: crate::LayoutRect {
                    x: 0.0,
                    y: 0.0,
                    width: 30.0,
                    height: 20.0,
                },
                baseline: 15.0,
            }],
            fragments: vec![TextFragment {
                range: TextRange { start: 1, end: 3 },
                bounds: crate::LayoutRect {
                    x: 10.0,
                    y: 0.0,
                    width: 20.0,
                    height: 20.0,
                },
            }],
        };
        assert!(geometry.validate("a🦀"));
        geometry.fragments[0].range.start = 2;
        assert!(!geometry.validate("a🦀"));
        geometry.fragments[0].range.start = 1;
        geometry.lines[0].ellipsis_count = 4;
        assert!(!geometry.validate("a🦀"));
        geometry.lines[0].ellipsis_count = 0;
        geometry.fragments[0].bounds.x = f32::NAN;
        assert!(!geometry.validate("a🦀"));
    }

    #[test]
    fn utf16_offsets_preserve_non_ascii_and_supplementary_characters() {
        let text = "a🦀日本";
        let range = TextByteRange { start: 1, end: 8 };
        assert_eq!(range.slice(text), Some("🦀日"));
        assert_eq!(range.utf16(text), Some(1..4));
        assert_eq!(TextByteRange { start: 2, end: 5 }.utf16(text), None);
        assert_eq!(TextByteRange { start: 8, end: 1 }.slice(text), None);
    }
}
