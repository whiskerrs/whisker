use super::*;
use std::convert::Infallible;
use whisker_protocol::{
    InlineAlignment, InlinePlacement, LayoutRect, MeasuredSize, MeasurementMetrics,
    MeasurementPayload, MeasurementResponse, ParagraphLine, ParagraphMetrics, TextByteRange,
    TextFragment, TextRange, TextSpanId,
};
use whisker_style::{Axes, ComputedSizeValue, SpecifiedStyle, StyleEnvironment, resolve_style};

struct ParagraphHost {
    visible: bool,
    calls: usize,
}
impl crate::MeasurementProvider for ParagraphHost {
    type Error = Infallible;
    fn retain_prepared_content(
        &mut self,
        _: SurfaceId,
        _: u64,
        retained: &mut dyn Iterator<Item = whisker_protocol::PreparedContentId>,
    ) {
        assert!(retained.count() > 0);
    }
    fn measure_batch(
        &mut self,
        _: SurfaceId,
        requests: &[whisker_protocol::MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        self.calls += requests.len();
        for request in requests {
            let MeasurementPayload::Text(text) = &request.payload else {
                panic!("text")
            };
            let mut metrics = MeasurementMetrics::from_size(MeasuredSize::new(100.0, 40.0));
            metrics.first_baseline = Some(18.0);
            metrics.prepared_content = whisker_protocol::PreparedContentId::new(request.key.get());
            if !text.runs.is_empty() {
                metrics.paragraph = Some(ParagraphMetrics {
                    lines: vec![ParagraphLine {
                        range: TextRange {
                            start: 0,
                            end: text.text.encode_utf16().count() as u32,
                        },
                        ellipsis_count: 0,
                        bounds: LayoutRect {
                            x: 0.0,
                            y: 0.0,
                            width: 100.0,
                            height: 40.0,
                        },
                        baseline: 18.0,
                    }],
                    fragments: vec![
                        TextFragment {
                            range: TextRange { start: 0, end: 2 },
                            bounds: LayoutRect {
                                x: 0.0,
                                y: 0.0,
                                width: 20.0,
                                height: 20.0,
                            },
                        },
                        TextFragment {
                            range: TextRange { start: 3, end: 5 },
                            bounds: LayoutRect {
                                x: 60.0,
                                y: 20.0,
                                width: 20.0,
                                height: 20.0,
                            },
                        },
                    ],
                });
            }
            metrics.inline_placements = text
                .attachments
                .iter()
                .map(|child| InlinePlacement {
                    node: child.node,
                    origin: self.visible.then_some([20.0, 0.0]),
                })
                .collect();
            responses.push(MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics,
            });
        }
        Ok(())
    }
}
fn style() -> ComputedStyle {
    resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default())
        .unwrap()
        .computed()
        .clone()
}
fn fixed(width: f32, height: Option<f32>) -> ComputedLayoutStyle {
    let size =
        |value| ComputedSizeValue::Value(whisker_style::ComputedLengthPercentage::new(value, 0.0));
    ComputedLayoutStyle {
        size: Axes {
            width: size(width),
            height: height.map_or(ComputedSizeValue::Auto, size),
        },
        ..Default::default()
    }
}
fn run(start: u32, end: u32, id: u64, style: ComputedStyle) -> crate::ResolvedTextRun {
    crate::ResolvedTextRun {
        range: TextByteRange { start, end },
        span: TextSpanId::new(id).unwrap(),
        action: None,
        style,
        inline_box_paint: true,
    }
}
fn drive(surface: &mut SurfaceEngine, root: NodeId, host: &mut ParagraphHost, epoch: u64) {
    surface
        .drive_layout(
            root,
            LayoutSize::new(100.0, 100.0),
            epoch,
            host,
            crate::LayoutOptions::default(),
        )
        .unwrap();
}

#[test]
fn rich_paragraphs_preserve_source_hit_testing_and_inline_visibility_across_updates() {
    let mut surface = SurfaceEngine::new(SurfaceId::new(1).unwrap());
    let root = surface
        .create_node(ElementTypeId::new(1).unwrap(), fixed(100.0, None))
        .unwrap();
    let inline = surface
        .create_node(ElementTypeId::new(2).unwrap(), fixed(20.0, Some(20.0)))
        .unwrap();
    let child = surface
        .create_node(ElementTypeId::new(2).unwrap(), fixed(5.0, Some(5.0)))
        .unwrap();
    surface.insert_child(root, inline, 0).unwrap();
    surface.insert_child(inline, child, 0).unwrap();
    let style = style();
    assert!(
        surface
            .set_rich_text(
                NodeId::new(999).unwrap(),
                &PlainTextInput::new(""),
                &style,
                &[],
                &[]
            )
            .is_err()
    );
    assert_eq!(
        Rc::make_mut(&mut surface.paragraph_visibility)
            .visibility(root, whisker_protocol::Visibility::Visible),
        whisker_protocol::Visibility::Visible
    );
    let input = PlainTextInput::new("ab\u{fffc}cd");
    let runs = vec![
        run(0, 2, 11, style.clone()),
        run(2, 5, 11, style.clone()),
        run(5, 7, 12, style.clone()),
    ];
    let attachments = vec![crate::InlineAttachmentInput {
        truncation: false,
        label: Some("icon".into()),
        node: inline,
        range: TextByteRange { start: 2, end: 5 },
        alignment: InlineAlignment::Offset(3.0),
    }];
    let mut baseline_attachments = attachments.clone();
    baseline_attachments[0].alignment = InlineAlignment::Baseline;
    assert_ne!(
        crate::lower_rich_text(&input, &style, &runs, &baseline_attachments).measurement(),
        crate::lower_rich_text(&input, &style, &runs, &attachments).measurement()
    );
    assert!(
        surface
            .set_rich_text(root, &input, &style, &runs, &attachments)
            .unwrap()
    );
    assert!(
        surface
            .set_rich_text(
                NodeId::new(999).unwrap(),
                &input,
                &style,
                &runs,
                &attachments
            )
            .is_err()
    );
    assert!(
        !surface
            .set_rich_text(root, &input, &style, &runs, &attachments)
            .unwrap()
    );
    let mut host = ParagraphHost {
        visible: true,
        calls: 0,
    };
    drive(&mut surface, root, &mut host, 1);
    assert_eq!(
        Rc::make_mut(&mut surface.paragraph_visibility).accessibility(root, Accessibility::new()),
        Accessibility::new()
    );
    let snapshot = surface.scene.node_snapshot(root).unwrap();
    assert!(
        surface
            .scene
            .node_snapshot(NodeId::new(999).unwrap())
            .is_none()
    );
    assert_eq!(
        surface
            .scene
            .text_span_at(root, whisker_protocol::InputPoint { x: 5.0, y: 5.0 }),
        TextSpanId::new(11)
    );
    assert_eq!(
        surface
            .scene
            .text_span_at(root, whisker_protocol::InputPoint { x: 65.0, y: 25.0 }),
        TextSpanId::new(12)
    );
    assert_eq!(
        surface
            .scene
            .text_span_at(root, whisker_protocol::InputPoint { x: 40.0, y: 25.0 }),
        None
    );
    let calls = host.calls;
    let colored = resolve_style(
        &SpecifiedStyle::new().push(
            whisker_style::StyleProperty::Color,
            whisker_style::StyleValue::Color(whisker_style::ColorValue::Rgba {
                red: 255,
                green: 0,
                blue: 0,
                alpha: whisker_style::StyleNumber::new(1.0),
            }),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    let changed = vec![
        run(0, 2, 11, colored.computed().clone()),
        runs[1].clone(),
        runs[2].clone(),
    ];
    surface
        .set_rich_text(root, &input, &style, &changed, &attachments)
        .unwrap();
    drive(&mut surface, root, &mut host, 1);
    assert_eq!(calls, host.calls);
    assert_ne!(snapshot.text(), surface.scene.node(root).unwrap().text());
    host.visible = false;
    drive(&mut surface, root, &mut host, 2);
    assert!(
        surface
            .last_layout
            .as_ref()
            .unwrap()
            .has_paragraph_suppression()
    );
    for node in [inline, child] {
        assert_eq!(
            surface.scene.node(node).unwrap().visibility(),
            Some(whisker_protocol::Visibility::Hidden)
        );
    }
    surface
        .set_accessibility(inline, Accessibility::new().label("updated"))
        .unwrap();
    surface.update_computed_style(inline, &style).unwrap();
    drive(&mut surface, root, &mut host, 2);
    assert_eq!(
        surface.scene.node(inline).unwrap().visibility(),
        Some(whisker_protocol::Visibility::Hidden)
    );
    host.visible = true;
    drive(&mut surface, root, &mut host, 3);
    assert_eq!(
        surface
            .scene
            .node(inline)
            .unwrap()
            .accessibility()
            .unwrap()
            .label
            .as_deref(),
        Some("updated")
    );
    assert_eq!(
        surface.scene.node(inline).unwrap().visibility(),
        Some(whisker_protocol::Visibility::Visible)
    );
    host.visible = false;
    drive(&mut surface, root, &mut host, 4);
    surface.delete_node(inline).unwrap();
    surface
        .set_rich_text(root, &PlainTextInput::new("plain"), &style, &[], &[])
        .unwrap();
    drive(&mut surface, root, &mut host, 5);
    assert!(surface.paragraph_visibility.is_empty());
    assert!(
        surface
            .scene
            .node(root)
            .unwrap()
            .text()
            .unwrap()
            .runs
            .is_empty()
    );
}

#[test]
fn rich_lowering_separates_metric_styles_from_paint_for_all_inline_alignments() {
    use whisker_style::{
        StyleProperty as Property, StyleValue as Value, VerticalAlignValue as Align,
    };
    for alignment in [
        Value::VerticalAlign(Align::Baseline),
        Value::VerticalAlign(Align::Top),
        Value::VerticalAlign(Align::Middle),
        Value::VerticalAlign(Align::Bottom),
        Value::Length(whisker_style::LengthValue::Dimension {
            value: whisker_style::StyleNumber::new(3.0),
            unit: whisker_style::LengthUnit::Px,
        }),
    ] {
        let resolved = resolve_style(
            &SpecifiedStyle::new()
                .push(Property::VerticalAlign, alignment)
                .push(
                    Property::WhiteSpace,
                    Value::WhiteSpace(whisker_style::WhiteSpaceValue::PreWrap),
                ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        let input = PlainTextInput::new("ab");
        let lowered = crate::lower_rich_text(
            &input,
            resolved.computed(),
            &[run(0, 2, 1, resolved.computed().clone())],
            &[],
        );
        assert_eq!(
            lowered.content().payload.wrap,
            whisker_protocol::MeasureTextWrap::PreserveWhitespace
        );
        assert!(lowered.content().validate().is_ok());
    }
}

#[test]
fn inline_hit_testing_requires_shaped_text_and_invertible_ancestor_geometry() {
    let mut scene = Scene::new(SurfaceId::new(1).unwrap());
    let root = scene.create_node(ElementTypeId::new(1).unwrap()).unwrap();
    let point = whisker_protocol::InputPoint { x: 5.0, y: 5.0 };
    assert!(
        scene
            .text_span_at(NodeId::new(999).unwrap(), point)
            .is_none()
    );
    assert!(scene.text_span_at(root, point).is_none());
    let style = style();
    let source = PlainTextInput::new("ab");
    let mut content = crate::lower_rich_text(&source, &style, &[run(0, 2, 1, style.clone())], &[])
        .content()
        .clone();
    scene.set_text(root, content.clone()).unwrap();
    assert!(scene.text_span_at(root, point).is_none());
    content.paragraph = Some(ParagraphMetrics {
        lines: vec![],
        fragments: vec![TextFragment {
            range: TextRange { start: 0, end: 2 },
            bounds: LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
        }],
    });
    scene.set_text(root, content).unwrap();
    assert!(scene.text_span_at(root, point).is_none());
    scene
        .set_layout(
            root,
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        )
        .unwrap();
    let parent = scene.create_node(ElementTypeId::new(2).unwrap()).unwrap();
    scene.insert_child(parent, root, 0).unwrap();
    assert!(scene.text_span_at(root, point).is_none());
    scene
        .set_layout(
            parent,
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        )
        .unwrap();
    scene
        .set_transform(parent, whisker_protocol::Transform([0.0; 16]))
        .unwrap();
    assert!(scene.text_span_at(root, point).is_none());
}
