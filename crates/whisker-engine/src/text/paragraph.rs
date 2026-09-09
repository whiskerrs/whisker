use super::*;

/// One logical inline Text with fully resolved style.
#[derive(Clone, Debug)]
pub struct ResolvedTextRun {
    /// UTF-8 byte range in the complete paragraph.
    pub range: whisker_protocol::TextByteRange,
    /// Runtime identity used for logical event propagation.
    pub span: whisker_protocol::TextSpanId,
    /// Logical inline ancestor with a tap action.
    pub action: Option<whisker_protocol::TextSpanId>,
    /// Resolved style of the inline Text.
    pub style: ComputedStyle,
    /// Whether box paint belongs to a logical inline span.
    pub inline_box_paint: bool,
}

/// An inline subtree whose dimensions are resolved by the layout adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineAttachmentInput {
    /// Whether this subtree replaces the hidden suffix.
    pub truncation: bool,
    /// Explicit alternative used by selection copy.
    pub label: Option<String>,
    /// Retained subtree root.
    pub node: whisker_protocol::NodeId,
    /// Object replacement character in the paragraph.
    pub range: whisker_protocol::TextByteRange,
    /// Position relative to the surrounding line.
    pub alignment: whisker_protocol::InlineAlignment,
}

/// Lowers a paragraph while keeping metric and paint-only changes separate.
pub fn lower_rich_text(
    input: &PlainTextInput,
    style: &ComputedStyle,
    runs: &[ResolvedTextRun],
    attachments: &[InlineAttachmentInput],
) -> LoweredPlainText {
    let mut lowered = lower_plain_text(input, style);
    if runs.is_empty() && attachments.is_empty() {
        return lowered;
    }
    let mut metric_hash = DefaultHasher::new();
    lowered.measurement.style_hash.hash(&mut metric_hash);
    for run in runs {
        let resolved = lower_plain_text(&PlainTextInput::new(""), &run.style);
        run.style.vertical_align().hash(&mut metric_hash);
        run.range.hash(&mut metric_hash);
        hash_font_metrics(run.style.inherited_text(), &mut metric_hash);
        let mut metrics = resolved.content.payload.style;
        metrics.line_height = lowered.content.payload.style.line_height;
        lowered
            .content
            .payload
            .runs
            .push(whisker_protocol::TextMeasureRun {
                alignment: inline_alignment(run.style.vertical_align()),
                range: run.range,
                style: metrics,
            });
        lowered.content.runs.push(whisker_protocol::TextPaintRun {
            range: run.range,
            span: run.span,
            action: run.action,
            paint: resolved.content.paint,
            background: run
                .inline_box_paint
                .then(|| lower_color(&run.style.paint().background_color)),
            background_radii: whisker_protocol::PaintCorners {
                top_left: crate::paint::corner_radius(&run.style.paint().border_radii.top_left),
                top_right: crate::paint::corner_radius(&run.style.paint().border_radii.top_right),
                bottom_right: crate::paint::corner_radius(
                    &run.style.paint().border_radii.bottom_right,
                ),
                bottom_left: crate::paint::corner_radius(
                    &run.style.paint().border_radii.bottom_left,
                ),
            },
        });
    }
    for attachment in attachments {
        attachment.truncation.hash(&mut metric_hash);
        attachment.range.hash(&mut metric_hash);
        attachment.node.hash(&mut metric_hash);
        std::mem::discriminant(&attachment.alignment).hash(&mut metric_hash);
        if let whisker_protocol::InlineAlignment::Offset(value) = attachment.alignment {
            value.to_bits().hash(&mut metric_hash);
        }
    }
    lowered.measurement.style_hash = metric_hash.finish();
    lowered.measurement.payload = MeasurementPayload::Text(lowered.content.payload.clone());
    lowered
}

fn inline_alignment(
    value: whisker_style::ComputedVerticalAlign,
) -> whisker_protocol::InlineAlignment {
    use whisker_protocol::InlineAlignment as Inline;
    use whisker_style::ComputedVerticalAlign as Computed;
    match value {
        Computed::Baseline => Inline::Baseline,
        Computed::Top => Inline::Top,
        Computed::Middle => Inline::Middle,
        Computed::Bottom => Inline::Bottom,
        Computed::Offset(value) => Inline::Offset(value.get()),
    }
}
