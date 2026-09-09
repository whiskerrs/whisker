mod geometry;
use geometry::paragraph_geometry;

use std::borrow::Cow;
use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, FontFeatures, FontStyle,
    FontVariations, FontWeight, GenericFamily, IndentOptions, InlineBox, InlineBoxKind, Layout,
    LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty, TextWrapMode, WordBreak,
};
use whisker_protocol::{
    InlineAlignment, LayoutRect, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
    MeasureTextAlignment, MeasureTextDirection, MeasureTextWordBreak, MeasureTextWrap,
    TextMeasurePayload, TextMeasureStyle,
};

#[derive(Default)]
pub(super) struct ParagraphShaper {
    fonts: FontContext,
    context: LayoutContext<u32>,
    strut: Option<(TextMeasureStyle, parley::RunMetrics)>,
}

pub(crate) struct PreparedParagraph {
    pub(crate) layout: Layout<u32>,
    source_offsets: BTreeMap<u32, usize>,
    visible_end: usize,
    pub(crate) lines: Vec<ParagraphLine>,
    pub(crate) attachments: Vec<(usize, LayoutRect)>,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) geometry: whisker_protocol::ParagraphMetrics,
}

pub(crate) struct ParagraphLine {
    pub(crate) top: f32,
    pub(crate) height: f32,
    pub(crate) baseline: f32,
    pub(crate) shift: f32,
    pub(crate) x_height: f32,
}

impl ParagraphShaper {
    pub(super) fn shape(
        &mut self,
        payload: &TextMeasurePayload,
        width: Option<f32>,
    ) -> PreparedParagraph {
        let token = payload
            .attachments
            .iter()
            .position(|attachment| attachment.truncation);
        let mut body = payload.clone();
        body.attachments.retain(|attachment| !attachment.truncation);
        let full = self.shape_candidate(&body, width);
        if (token.is_none() && payload.overflow != whisker_protocol::MeasureTextOverflow::Ellipsis)
            || fits(&full, payload, width)
        {
            return full;
        }
        let boundaries: Vec<_> = payload
            .text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .chain(std::iter::once(payload.text.len()))
            .collect();
        let mut best_end = 0;
        let mut best = self.shape_candidate(&prefix_payload(payload, 0, width), width);
        let mut low = 0;
        let mut high = boundaries.len();
        for _ in 0..60 {
            if low >= high {
                break;
            }
            let middle = low + (high - low) / 2;
            let candidate =
                self.shape_candidate(&prefix_payload(payload, boundaries[middle], width), width);
            if fits(&candidate, payload, width) {
                best_end = boundaries[middle];
                best = candidate;
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if let Some(token) = token {
            if let Some((index, _)) = best.attachments.last_mut() {
                *index = token;
            }
        }
        best.visible_end = best_end;
        let visible = payload.text[..best_end].encode_utf16().count() as u32;
        let total = payload.text.encode_utf16().count() as u32;
        best.geometry.fragments.retain_mut(|fragment| {
            fragment.range.end = fragment.range.end.min(visible);
            fragment.range.start < fragment.range.end
        });
        if let Some(last) = best.geometry.lines.last_mut() {
            last.range.end = total;
            last.ellipsis_count = total - visible;
        }
        best
    }

    fn strut_metrics(&mut self, style: &TextMeasureStyle) -> parley::RunMetrics {
        if let Some((cached, metrics)) = &self.strut {
            if cached == style {
                return *metrics;
            }
        }
        let mut builder = self
            .context
            .ranged_builder(&mut self.fonts, " ", 1.0, false);
        for property in metric_properties(style) {
            builder.push_default(property);
        }
        let mut layout = builder.build(" ");
        layout.break_all_lines(None);
        let metrics = layout
            .lines()
            .next()
            .and_then(|line| line.runs().next())
            .map(|run| *run.metrics())
            .unwrap_or_default();
        self.strut = Some((style.clone(), metrics));
        metrics
    }

    fn shape_candidate(
        &mut self,
        payload: &TextMeasurePayload,
        width: Option<f32>,
    ) -> PreparedParagraph {
        let strut = self.strut_metrics(&payload.style);
        let (text, offsets) = paragraph_source(payload);
        let mut builder = self
            .context
            .ranged_builder(&mut self.fonts, &text, 1.0, false);
        for property in metric_properties(&payload.style) {
            builder.push_default(property);
        }
        builder.push_default(StyleProperty::Brush(0));
        builder.push_default(StyleProperty::TextWrapMode(match payload.wrap {
            MeasureTextWrap::Wrap | MeasureTextWrap::PreserveWhitespace => TextWrapMode::Wrap,
            MeasureTextWrap::NoWrap => TextWrapMode::NoWrap,
        }));
        builder.push_default(StyleProperty::WordBreak(match payload.word_break {
            MeasureTextWordBreak::Normal => WordBreak::Normal,
            MeasureTextWordBreak::BreakAll => WordBreak::BreakAll,
            MeasureTextWordBreak::KeepAll => WordBreak::KeepAll,
        }));
        for (index, run) in payload.runs.iter().enumerate() {
            let range = offsets[&run.range.start]..offsets[&run.range.end];
            if range.is_empty() {
                continue;
            }
            for property in metric_properties(&run.style) {
                builder.push(property, range.clone());
            }
            builder.push(StyleProperty::Brush(index as u32 + 1), range);
        }
        for (index, attachment) in payload.attachments.iter().enumerate() {
            builder.push_inline_box(InlineBox {
                id: index as u64,
                kind: InlineBoxKind::InFlow,
                index: offsets[&attachment.range.start],
                width: attachment.size.width,
                height: attachment.size.height,
            });
        }
        let mut layout = builder.build(&text);
        layout.set_text_indent(
            payload.indent.resolve(width.unwrap_or_default()),
            IndentOptions::default(),
        );
        layout.break_all_lines(width);
        layout.align(
            match payload.alignment {
                MeasureTextAlignment::Start => Alignment::Start,
                MeasureTextAlignment::End => Alignment::End,
                MeasureTextAlignment::Left => Alignment::Left,
                MeasureTextAlignment::Right => Alignment::Right,
                MeasureTextAlignment::Center => Alignment::Center,
            },
            AlignmentOptions::default(),
        );
        let mut top = 0.0;
        let mut lines = Vec::new();
        let mut attachments = Vec::new();
        let mut measured_width = 0.0_f32;
        for line in layout
            .lines()
            .take(payload.max_lines.unwrap_or(u32::MAX) as usize)
        {
            let mut ascent = strut.ascent;
            let mut descent = strut.descent;
            let mut line_height = strut.line_height;
            let x_height = strut.x_height.unwrap_or(payload.style.font_size / 2.0);
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(glyphs) = item {
                    let run = glyphs.run();
                    let metrics = run.metrics();
                    let alignment = run_alignment(payload, glyphs.style().brush);
                    line_height = line_height.max(metrics.line_height);
                    if matches!(alignment, InlineAlignment::Top | InlineAlignment::Bottom) {
                        line_height = line_height.max(metrics.ascent + metrics.descent);
                    } else {
                        let shift = metric_shift(alignment, metrics, x_height);
                        ascent = ascent.max(metrics.ascent + shift);
                        descent = descent.max(metrics.descent - shift);
                    }
                }
            }
            let mut boxes = Vec::new();
            for item in line.items() {
                if let PositionedLayoutItem::InlineBox(placed) = item {
                    let attachment = &payload.attachments[placed.id as usize];
                    let height = attachment.size.height;
                    let baseline = match attachment.alignment {
                        InlineAlignment::Baseline => attachment.baseline,
                        InlineAlignment::Offset(offset) => attachment.baseline + offset,
                        InlineAlignment::Middle => (height + x_height) / 2.0,
                        InlineAlignment::Top | InlineAlignment::Bottom => {
                            line_height = line_height.max(height);
                            boxes.push((placed.id as usize, placed.x, None));
                            continue;
                        }
                    };
                    ascent = ascent.max(baseline);
                    descent = descent.max(height - baseline);
                    boxes.push((placed.id as usize, placed.x, Some(baseline)));
                }
            }
            line_height = line_height.max(ascent + descent);
            let baseline = top + ascent + (line_height - ascent - descent) / 2.0;
            for (index, x, box_baseline) in boxes {
                let attachment = &payload.attachments[index];
                let size = attachment.size;
                let y = if let Some(box_baseline) = box_baseline {
                    baseline - box_baseline
                } else if attachment.alignment == InlineAlignment::Top {
                    top
                } else {
                    top + line_height - size.height
                };
                attachments.push((
                    index,
                    LayoutRect {
                        x,
                        y,
                        width: size.width,
                        height: size.height,
                    },
                ));
            }
            lines.push(ParagraphLine {
                top,
                height: line_height,
                baseline,
                shift: baseline - line.metrics().baseline,
                x_height,
            });
            top += line_height;
            measured_width = measured_width.max(line.metrics().advance);
        }
        let geometry = paragraph_geometry(payload, &layout, &lines, &offsets, &attachments);
        PreparedParagraph {
            geometry,
            source_offsets: offsets,
            visible_end: payload.text.len(),
            layout,
            lines,
            attachments,
            width: measured_width,
            height: top,
        }
    }
}

fn paragraph_source(payload: &TextMeasurePayload) -> (String, BTreeMap<u32, usize>) {
    let mut source = String::new();
    match payload.direction {
        MeasureTextDirection::LeftToRight => source.push('\u{200e}'),
        MeasureTextDirection::RightToLeft => source.push('\u{200f}'),
        MeasureTextDirection::Auto => {}
    }
    let mut offsets = BTreeMap::new();
    let mut attachments = payload.attachments.iter().peekable();
    for (offset, character) in payload.text.char_indices() {
        offsets.insert(offset as u32, source.len());
        if attachments
            .peek()
            .is_some_and(|attachment| attachment.range.start as usize == offset)
        {
            attachments.next();
        } else {
            source.push(character);
        }
    }
    offsets.insert(payload.text.len() as u32, source.len());
    (source, offsets)
}

fn metric_properties(style: &TextMeasureStyle) -> Vec<StyleProperty<'static, u32>> {
    let families = style
        .font_families
        .iter()
        .map(|family| match family {
            MeasureFontFamily::System => FontFamilyName::Generic(GenericFamily::SansSerif),
            MeasureFontFamily::Named(name) => FontFamilyName::Named(Cow::Owned(name.clone())),
        })
        .collect();
    let features = style
        .features
        .iter()
        .map(|feature| {
            parley::FontFeature::new(
                parley::setting::Tag::new(&feature.tag.get()),
                feature.value.min(u16::MAX as u32) as u16,
            )
        })
        .collect();
    let mut variations: Vec<_> = style
        .variations
        .iter()
        .map(|variation| {
            parley::FontVariation::new(
                parley::setting::Tag::new(&variation.tag.get()),
                variation.value,
            )
        })
        .collect();
    if style.optical_sizing == whisker_protocol::FontOpticalSizing::Auto
        && !style
            .variations
            .iter()
            .any(|variation| variation.tag.get() == *b"opsz")
    {
        variations.push(parley::FontVariation::new(
            parley::setting::Tag::new(b"opsz"),
            style.font_size,
        ));
    }
    vec![
        StyleProperty::FontFamily(FontFamily::List(Cow::Owned(families))),
        StyleProperty::FontSize(style.font_size),
        StyleProperty::FontWeight(FontWeight::new(f32::from(style.font_weight))),
        StyleProperty::FontStyle(match style.font_style {
            MeasureFontStyle::Normal => FontStyle::Normal,
            MeasureFontStyle::Italic => FontStyle::Italic,
            MeasureFontStyle::Oblique => FontStyle::Oblique(None),
        }),
        StyleProperty::LetterSpacing(style.letter_spacing),
        StyleProperty::LineHeight(match style.line_height {
            MeasureLineHeight::Normal => LineHeight::MetricsRelative(1.0),
            MeasureLineHeight::LogicalPixels(value) => LineHeight::Absolute(value),
        }),
        StyleProperty::FontFeatures(FontFeatures::List(Cow::Owned(features))),
        StyleProperty::FontVariations(FontVariations::List(Cow::Owned(variations))),
    ]
}

fn fits(paragraph: &PreparedParagraph, payload: &TextMeasurePayload, width: Option<f32>) -> bool {
    paragraph.layout.len() <= payload.max_lines.unwrap_or(u32::MAX) as usize
        && width.is_none_or(|width| {
            paragraph
                .layout
                .lines()
                .all(|line| line.metrics().advance <= width)
        })
}

fn prefix_payload(
    payload: &TextMeasurePayload,
    end: usize,
    width: Option<f32>,
) -> TextMeasurePayload {
    let mut prefix = payload.clone();
    prefix.text.truncate(end);
    let token = payload
        .attachments
        .iter()
        .find(|attachment| attachment.truncation);
    prefix
        .text
        .push(if token.is_some() { '\u{fffc}' } else { '…' });
    prefix.runs.retain_mut(|run| {
        run.range.end = run.range.end.min(end as u32);
        run.range.start < run.range.end
    });
    prefix
        .attachments
        .retain(|attachment| !attachment.truncation && attachment.range.end <= end as u32);
    if let Some(token) = token {
        let mut token = token.clone();
        token.truncation = false;
        token.range = whisker_protocol::TextByteRange {
            start: end as u32,
            end: prefix.text.len() as u32,
        };
        if let Some(width) = width {
            token.size.width = token.size.width.min(width);
        }
        prefix.attachments.push(token);
    }
    prefix
}

pub(crate) fn run_alignment(payload: &TextMeasurePayload, brush: u32) -> InlineAlignment {
    brush
        .checked_sub(1)
        .and_then(|index| payload.runs.get(index as usize))
        .map_or(InlineAlignment::Baseline, |run| run.alignment)
}

fn metric_shift(alignment: InlineAlignment, metrics: &parley::RunMetrics, x_height: f32) -> f32 {
    match alignment {
        InlineAlignment::Offset(shift) => shift,
        InlineAlignment::Middle => (metrics.descent - metrics.ascent + x_height) / 2.0,
        _ => 0.0,
    }
}

impl ParagraphLine {
    pub(crate) fn run_shift(
        &self,
        alignment: InlineAlignment,
        metrics: &parley::RunMetrics,
    ) -> f32 {
        match alignment {
            InlineAlignment::Top => self.baseline - self.top - metrics.ascent,
            InlineAlignment::Bottom => self.baseline - self.top - self.height + metrics.descent,
            _ => metric_shift(alignment, metrics, self.x_height),
        }
    }
}

#[cfg(test)]
mod tests;
