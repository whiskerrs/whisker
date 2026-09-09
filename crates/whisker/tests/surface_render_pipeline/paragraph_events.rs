use super::*;
use whisker_engine::whisker_protocol::{LayoutRect, ParagraphMetrics, TextFragment, TextRange};

struct FragmentHost;

impl MeasurementProvider for FragmentHost {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        responses.extend(requests.iter().map(|request| {
            let mut metrics = MeasurementMetrics::from_size(MeasuredSize::new(100.0, 40.0));
            metrics.prepared_content =
                whisker_engine::whisker_protocol::PreparedContentId::new(request.key.get());
            metrics.paragraph = Some(ParagraphMetrics {
                lines: vec![whisker_engine::whisker_protocol::ParagraphLine {
                    range: TextRange { start: 0, end: 4 },
                    ellipsis_count: 0,
                    bounds: LayoutRect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 40.0,
                    },
                    baseline: 16.0,
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
                        range: TextRange { start: 2, end: 4 },
                        bounds: LayoutRect {
                            x: 60.0,
                            y: 20.0,
                            width: 20.0,
                            height: 20.0,
                        },
                    },
                ],
            });
            if matches!(&request.payload, whisker_engine::whisker_protocol::MeasurementPayload::Text(text) if text.text.is_empty()) {
                metrics.paragraph = None;
                metrics.size = MeasuredSize::new(0.0, 0.0);
            }
            MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics,
            }
        }));
        Ok(())
    }
}

#[test]
fn disjoint_inline_fragments_route_through_logical_capture_and_catch() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(73).unwrap(),
        StyleEnvironment::new(120.0, 100.0, 1.0, 14.0),
    );
    let received = Rc::new(RefCell::new(Vec::new()));
    let elements = with_installed_renderer(surface.renderer(), || {
        owner.with(|| {
            let child = Text::builder()
                .value("link")
                .id("link")
                .dataset(Dataset::new().int("destination", 42))
                .build();
            let middle = Text::builder()
                .id("middle")
                .body(|body| body.push(child))
                .build();
            let root = Text::builder()
                .id("paragraph")
                .style(css!(width: px(100), height: px(40), padding: px(10)))
                .body(|body| body.push(middle))
                .build();
            for (element, binding) in [
                (root, BindType::CaptureBind),
                (middle, BindType::CaptureBind),
                (child, BindType::Bind),
                (middle, BindType::Catch),
                (root, BindType::Bind),
            ] {
                let received = received.clone();
                whisker::runtime::event::bind_typed(
                    element,
                    "tap",
                    binding,
                    move |event: whisker::event::TouchEvent| received.borrow_mut().push(event),
                );
            }
            set_root(root);
            (root, middle, child)
        })
    });
    assert!(surface.binding_error().is_none());
    let mut host = FragmentHost;
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(120.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    let node = renderer.frames()[0]
        .packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetText { node, .. } => Some(*node),
            _ => None,
        })
        .unwrap();
    for (x, y) in [(15.0, 15.0), (75.0, 35.0)] {
        let dispatch = with_installed_renderer(surface.renderer(), || {
            surface.dispatch_input(&InputEvent {
                presentation_revision: None,
                surface: surface.surface(),
                timestamp_ms: 12.0,
                kind: InputEventKind::Tap,
                pointer: Some(PointerInput {
                    id: PointerId::new(1).unwrap(),
                    kind: PointerKind::Touch,
                    position: whisker_engine::whisker_protocol::InputPoint { x, y },
                    buttons: 1,
                    changed_button: 0,
                }),
                target: Some(node),
                detail: WhiskerValue::Null,
            })
        })
        .unwrap();
        assert_eq!(dispatch.listener_count, 4);
        let events = received.borrow_mut().drain(..).collect::<Vec<_>>();
        assert_eq!(
            events
                .iter()
                .map(|event| event.current_target.id.as_str())
                .collect::<Vec<_>>(),
            ["paragraph", "middle", "link", "middle"]
        );
        for event in events {
            assert_eq!(event.target.id, "link");
            assert_eq!(
                event.target.dataset.get("destination"),
                Some(&WhiskerValue::Int(42))
            );
        }
    }
    with_installed_renderer(surface.renderer(), || {
        surface.dispatch_input(&InputEvent {
            presentation_revision: None,
            surface: surface.surface(),
            timestamp_ms: 13.0,
            kind: InputEventKind::Tap,
            pointer: Some(PointerInput {
                id: PointerId::new(1).unwrap(),
                kind: PointerKind::Touch,
                position: whisker_engine::whisker_protocol::InputPoint { x: 50.0, y: 25.0 },
                buttons: 1,
                changed_button: 0,
            }),
            target: Some(node),
            detail: WhiskerValue::Null,
        })
    })
    .unwrap();
    assert!(
        received
            .borrow()
            .iter()
            .all(|event| event.target.id == "paragraph")
    );
    let _ = elements;
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn text_layout_notifies_only_after_acceptance_and_only_when_geometry_changes() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(74).unwrap(),
        StyleEnvironment::new(120.0, 100.0, 1.0, 14.0),
    );
    let received = Rc::new(RefCell::new(Vec::new()));
    let color = owner.with(|| signal(Color::hex(0xff0000)));
    let value = owner.with(|| signal("link".to_owned()));
    with_installed_renderer(surface.renderer(), || {
        owner.with(|| {
            let received = received.clone();
            let text = Text::builder()
                .value(value)
                .style(computed(move || Css::new().color(color.get())))
                .on_text_layout(move |event| received.borrow_mut().push(event))
                .build();
            set_root(text);
        })
    });
    let mut host = FragmentHost;
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .drive_layout(
            LayoutSize::new(120.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(received.borrow().is_empty());
    surface.present(1, &mut renderer).unwrap();
    assert_eq!(received.borrow().len(), 1);
    assert_eq!(received.borrow()[0].detail.lines[0].end, 4);
    surface.present(1, &mut renderer).unwrap();
    assert_eq!(received.borrow().len(), 1);
    with_installed_renderer(surface.renderer(), || {
        color.set(Color::hex(0x0000ff));
        whisker::flush();
    });
    surface
        .render_frame(
            LayoutSize::new(120.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert_eq!(received.borrow().len(), 1);
    with_installed_renderer(surface.renderer(), || {
        value.set(String::new());
        whisker::flush();
    });
    surface
        .render_frame(
            LayoutSize::new(120.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert_eq!(received.borrow().len(), 2);
    assert!(received.borrow()[1].detail.lines.is_empty());
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn pending_text_content_cannot_receive_events_from_the_previous_presentation() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(75).unwrap(),
        StyleEnvironment::new(120.0, 100.0, 1.0, 14.0),
    );
    let calls = Rc::new(Cell::new(0));
    let value = owner.with(|| signal("link".to_owned()));
    with_installed_renderer(surface.renderer(), || {
        owner.with(|| {
            let calls = calls.clone();
            let child = Text::builder()
                .value(value)
                .on_tap(move |_| calls.set(calls.get() + 1))
                .build();
            let root = Text::builder().body(|body| body.push(child)).build();
            set_root(root);
        })
    });
    let mut host = FragmentHost;
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(120.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    let node = renderer.frames()[0]
        .packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetText { node, .. } => Some(*node),
            _ => None,
        })
        .unwrap();
    let event = InputEvent {
        presentation_revision: None,
        surface: surface.surface(),
        timestamp_ms: 12.0,
        kind: InputEventKind::Tap,
        pointer: Some(PointerInput {
            id: PointerId::new(1).unwrap(),
            kind: PointerKind::Touch,
            position: whisker_engine::whisker_protocol::InputPoint { x: 5.0, y: 5.0 },
            buttons: 1,
            changed_button: 0,
        }),
        target: Some(node),
        detail: WhiskerValue::Null,
    };
    surface.dispatch_input(&event).unwrap();
    assert_eq!(calls.get(), 1);
    with_installed_renderer(surface.renderer(), || {
        value.set("next".into());
        whisker::flush();
    });
    assert!(!surface.dispatch_input(&event).unwrap().consumed);
    surface
        .drive_layout(
            LayoutSize::new(120.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(!surface.dispatch_input(&event).unwrap().consumed);
    assert_eq!(calls.get(), 1);
    let displayed_revision = surface.accepted_revision();
    surface.present(1, &mut renderer).unwrap();
    let delayed = InputEvent {
        presentation_revision: Some(displayed_revision),
        ..event.clone()
    };
    assert_eq!(surface.dispatch_input(&delayed).unwrap().listener_count, 0);
    assert_eq!(calls.get(), 1);
    surface.dispatch_input(&event).unwrap();
    assert_eq!(calls.get(), 2);
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn accessible_inline_actions_validate_identity_and_revision_before_dispatch() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(SurfaceId::new(76).unwrap(), StyleEnvironment::default());
    let calls = Rc::new(Cell::new(0));
    let value = owner.with(|| signal("link".to_owned()));
    with_installed_renderer(surface.renderer(), || {
        owner.with(|| {
            let calls = calls.clone();
            let child = Text::builder()
                .value(value)
                .id("link")
                .on_tap(move |event| {
                    assert_eq!(event.target.id, "link");
                    calls.set(calls.get() + 1);
                })
                .build();
            set_root(Text::builder().body(|body| body.push(child)).build());
        })
    });
    let mut recorder = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(120.0, 100.0),
            1,
            1,
            &mut FragmentHost,
            &mut recorder,
            LayoutOptions::default(),
        )
        .unwrap();
    let (node, content) = recorder.frames()[0]
        .packet
        .operations
        .iter()
        .find_map(|op| {
            if let Operation::SetText { node, content } = op {
                Some((*node, content))
            } else {
                None
            }
        })
        .unwrap();
    let actions = content.accessible_actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].1, "link");
    let event = InputEvent {
        presentation_revision: None,
        surface: surface.surface(),
        timestamp_ms: 0.0,
        kind: InputEventKind::Named("textactivate".into()),
        pointer: None,
        target: Some(node),
        detail: WhiskerValue::map([
            ("span", WhiskerValue::Int(actions[0].0.get() as i64)),
            (
                "revision",
                WhiskerValue::Int(content.prepared_content.unwrap().get() as i64),
            ),
        ]),
    };
    assert!(surface.dispatch_input(&event).unwrap().consumed);
    assert_eq!(calls.get(), 1);
    with_installed_renderer(surface.renderer(), || {
        value.set("next".into());
        whisker::flush();
    });
    assert!(!surface.dispatch_input(&event).unwrap().consumed);
    surface
        .render_frame(
            LayoutSize::new(120.0, 100.0),
            1,
            2,
            &mut FragmentHost,
            &mut recorder,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(!surface.dispatch_input(&event).unwrap().consumed);
    with_installed_renderer(surface.renderer(), || owner.dispose());
    assert!(!surface.dispatch_input(&event).unwrap().consumed);
    assert_eq!(calls.get(), 1);
}
