use super::*;

fn node(id: u64) -> NodeId {
    NodeId::new(id).unwrap()
}
fn span(id: u64) -> TextSpanId {
    TextSpanId::new(id).unwrap()
}
fn paint(range: TextByteRange, action: Option<TextSpanId>) -> TextPaintRun {
    TextPaintRun {
        range,
        span: span(1),
        action,
        paint: TextPaint::default(),
        background: None,
        background_radii: PaintCorners {
            top_left: Default::default(),
            top_right: Default::default(),
            bottom_right: Default::default(),
            bottom_left: Default::default(),
        },
    }
}
fn content() -> TextContent {
    let text = "a\u{fffc}b\u{fffc}🦀";
    let ranges = [(0, 1), (1, 4), (4, 5), (5, 8), (8, 12)];
    TextContent {
        payload: TextMeasurePayload {
            text: text.into(),
            style: Default::default(),
            runs: ranges
                .map(|(start, end)| TextMeasureRun {
                    range: TextByteRange { start, end },
                    style: Default::default(),
                    alignment: Default::default(),
                })
                .to_vec(),
            attachments: [(1, 4, Some("icon")), (5, 8, None), (12, 12, Some("more"))]
                .into_iter()
                .enumerate()
                .map(|(id, (start, end, label))| InlineAttachment {
                    node: node(id as u64 + 1),
                    range: TextByteRange { start, end },
                    truncation: start == end,
                    label: label.map(Into::into),
                    size: MeasuredSize::new(10.0, 10.0),
                    baseline: 8.0,
                    alignment: InlineAlignment::Offset(1.0),
                })
                .collect(),
            locale: None,
            direction: MeasureTextDirection::Auto,
            alignment: MeasureTextAlignment::Start,
            indent: Default::default(),
            wrap: MeasureTextWrap::Wrap,
            word_break: Default::default(),
            max_lines: None,
            overflow: MeasureTextOverflow::Clip,
        },
        paint: TextPaint::default(),
        prepared_content: PreparedContentId::new(1),
        paragraph: None,
        runs: ranges
            .into_iter()
            .map(|(start, end)| {
                paint(
                    TextByteRange { start, end },
                    Some(span(if start == 8 { 2 } else { 1 })),
                )
            })
            .collect(),
    }
}
fn geometry(end: u32, ellipsis_count: u32) -> ParagraphMetrics {
    ParagraphMetrics {
        lines: vec![ParagraphLine {
            range: TextRange { start: 0, end },
            ellipsis_count,
            bounds: LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 20.0,
            },
            baseline: 16.0,
        }],
        fragments: vec![],
    }
}

#[test]
fn copy_and_accessibility_use_source_offsets_without_generated_tokens() {
    let mut content = content();
    assert_eq!(content.validate(), Ok(()));
    assert_eq!(
        content.payload.copy_text(TextRange { start: 0, end: 6 }),
        Some("aiconb🦀".into())
    );
    assert_eq!(
        content.payload.copy_text(TextRange { start: 2, end: 3 }),
        Some("b".into())
    );
    assert_eq!(
        content.payload.copy_text(TextRange { start: 4, end: 5 }),
        None
    );
    assert_eq!(content.accessible_text(), "ab🦀");
    assert_eq!(
        content.accessible_actions(),
        vec![(span(1), "ab".into()), (span(2), "🦀".into())]
    );
    content.runs[0].action = None;
    assert_eq!(content.accessible_actions()[0].1, "b");
    content.paragraph = Some(geometry(6, 2));
    assert_eq!(content.accessible_text(), "ab");
    assert_eq!(content.accessible_actions(), vec![(span(1), "b".into())]);
    content.runs[0] = paint(TextByteRange { start: 0, end: 13 }, Some(span(9)));
    assert_eq!(content.accessible_actions(), vec![(span(1), "b".into())]);
    content.paragraph = Some(geometry(5, 0));
    assert_eq!(content.accessible_text(), "");
    assert_eq!(content.accessible_actions(), vec![(span(1), "b".into())]);
}

#[test]
fn paragraph_metrics_reject_unmatched_duplicate_and_nonfinite_placements() {
    let content = content();
    let payload = MeasurementPayload::Text(content.payload);
    let mut metrics = MeasurementMetrics::from_size(MeasuredSize::new(100.0, 20.0));
    metrics.paragraph = Some(geometry(6, 0));
    assert!(!metrics.matches_payload(&payload));
    metrics.inline_placements = vec![
        InlinePlacement {
            node: node(1),
            origin: Some([0.0, 0.0]),
        },
        InlinePlacement {
            node: node(2),
            origin: None,
        },
        InlinePlacement {
            node: node(3),
            origin: None,
        },
    ];
    assert!(metrics.matches_payload(&payload));
    metrics.inline_placements[0].origin = Some([f32::NAN, 0.0]);
    assert!(!metrics.matches_payload(&payload));
    metrics.inline_placements[0].origin = None;
    metrics.inline_placements[1].node = node(1);
    assert!(!metrics.matches_payload(&payload));
    metrics.inline_placements[1].node = node(9);
    assert!(!metrics.matches_payload(&payload));
    metrics.inline_placements[1].node = node(2);
    metrics.paragraph = Some(geometry(7, 0));
    assert!(!metrics.matches_payload(&payload));
    let custom = MeasurementPayload::Custom(CustomMeasurePayload {
        version: 1,
        data: WhiskerValue::Bytes(vec![]),
    });
    assert!(!metrics.matches_payload(&custom));
    metrics.paragraph = None;
    metrics.inline_placements.clear();
    assert!(metrics.matches_payload(&custom));
}

#[test]
fn rich_ranges_and_paint_are_validated_before_presentation() {
    let good = content();
    let mut broken = good.clone();
    broken.payload.runs[0].alignment = InlineAlignment::Offset(f32::NAN);
    assert!(broken.validate().is_err());
    broken = good.clone();
    broken.payload.runs[0].style.font_size = -1.0;
    assert!(broken.validate().is_err());
    broken = good.clone();
    broken
        .payload
        .attachments
        .push(broken.payload.attachments[2].clone());
    assert!(broken.validate().is_err());
    for end in [11, 13] {
        broken = good.clone();
        broken.payload.attachments[2].range.end = end;
        assert!(broken.validate().is_err());
    }
    for case in 0..8 {
        broken = good.clone();
        match case {
            0 => broken.runs[0].range.end = 0,
            1 => broken.runs[0].range.end = 13,
            2 => broken.runs[1].range.start = 0,
            3 => {
                broken.runs[0].paint.foreground = PaintColor::Srgba {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: -1.0,
                }
            }
            4 => {
                broken.runs[0].background = Some(PaintColor::Srgba {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: -1.0,
                })
            }
            5 => broken.runs[0].background_radii.top_left.horizontal.length = -1.0,
            6 => broken.payload.attachments[0].baseline = f32::NAN,
            _ => broken.payload.attachments[0].alignment = InlineAlignment::Offset(f32::NAN),
        }
        assert!(broken.validate().is_err(), "case {case}");
    }
    broken = good.clone();
    broken.runs[0].background = Some(PaintColor::default());
    assert!(broken.validate().is_ok());
}

#[test]
fn inline_effects_and_typography_require_their_own_host_capabilities() {
    let mut content = content();
    content.runs[0].paint.decoration.style = TextDecorationStyle::Wavy;
    content.runs[0].paint.decoration.lines.underline = true;
    content.payload.runs[0].style.optical_sizing = FontOpticalSizing::Auto;
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![Operation::SetText {
            node: node(1),
            content,
        }],
    };
    let required = packet.required_capabilities();
    assert!(required.contains(&RenderCapability::RichText));
    assert!(required.contains(&RenderCapability::TextTypography));
    assert!(required.contains(&RenderCapability::TextEffects));
}

#[test]
fn malformed_copy_ranges_never_slice_through_unicode_or_reversed_attachments() {
    assert_eq!(TextRange::from_utf8("", usize::MAX..usize::MAX), None);
    assert_eq!(TextRange::from_utf8("", 0..usize::MAX), None);
    let mut payload = content().payload;
    payload.attachments.swap(0, 1);
    assert_eq!(payload.copy_text(TextRange { start: 0, end: 6 }), None);
    payload.attachments.truncate(1);
    payload.attachments[0].range = TextByteRange { start: 1, end: 9 };
    assert_eq!(payload.copy_text(TextRange { start: 0, end: 6 }), None);
}

#[test]
fn presentation_rejects_geometry_outside_the_current_source() {
    let mut content = content();
    content.paragraph = Some(geometry(6, 0));
    assert!(content.validate().is_ok());
    content.paragraph = Some(geometry(7, 0));
    assert!(content.validate().is_err());
}
