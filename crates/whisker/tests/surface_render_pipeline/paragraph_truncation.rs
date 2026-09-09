use super::*;
use whisker_engine::whisker_protocol::{InlinePlacement, TextByteRange, Visibility};

struct TokenHost {
    visible: bool,
}
impl MeasurementProvider for TokenHost {
    type Error = Infallible;
    fn measure_batch(
        &mut self,
        _: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        for request in requests {
            let MeasurementPayload::Text(payload) = &request.payload else {
                panic!("text measurement expected")
            };
            let mut metrics = MeasurementMetrics::from_size(MeasuredSize::new(90.0, 20.0));
            if !payload.attachments.is_empty() {
                assert_eq!(payload.text, "Before after");
                assert_eq!(payload.attachments.len(), 1);
                let token = &payload.attachments[0];
                assert!(token.truncation);
                assert_eq!(token.range, TextByteRange { start: 12, end: 12 });
                assert!(token.size.width > 0.0);
                metrics.inline_placements.push(InlinePlacement {
                    node: token.node,
                    origin: self.visible.then_some([0.0, 0.0]),
                });
            }
            responses.push(MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics,
            });
        }
        Ok(())
    }
}

#[test]
fn truncation_content_stays_outside_source_and_hidden_state_keeps_its_owner() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(SurfaceId::new(73).unwrap(), StyleEnvironment::default());
    let saved = Rc::new(Cell::new(None));
    let cleaned = Rc::new(Cell::new(0));
    with_installed_renderer(surface.renderer(), || {
        owner.with(|| {
            let saved = saved.clone();
            let cleaned = cleaned.clone();
            let root = render! {
                Text(value: "Before", max_lines: 1u32) {
                    InlineTruncation {
                        StatefulToken(saved: saved, cleaned: cleaned)
                    }
                    Text(value: " after")
                }
            };
            set_root(root);
        })
    });
    assert_eq!(surface.binding_error(), None);
    let mut recorder = RecordingRenderer::new(surface.surface());
    let mut host = TokenHost { visible: false };
    surface
        .render_frame(
            LayoutSize::new(100.0, 120.0),
            1,
            1,
            &mut host,
            &mut recorder,
            LayoutOptions::default(),
        )
        .unwrap();
    let operations = &recorder.frames()[0].packet.operations;
    let token = operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetText { content, .. } => {
                content.payload.attachments.first().map(|token| token.node)
            }
            _ => None,
        })
        .unwrap();
    assert!(operations.iter().any(|operation| matches!(operation, Operation::SetVisibility { node, visibility: Visibility::Hidden } if *node == token)));
    let count = saved.get().unwrap();
    assert_eq!(cleaned.get(), 0);
    with_installed_renderer(surface.renderer(), || {
        count.set(1);
        whisker::flush();
    });
    host.visible = true;
    surface
        .render_frame(
            LayoutSize::new(120.0, 120.0),
            1,
            2,
            &mut host,
            &mut recorder,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(recorder.frames().last().unwrap().packet.operations.iter().any(|operation| matches!(operation, Operation::SetVisibility { node, visibility: Visibility::Visible } if *node == token)));
    assert_eq!(cleaned.get(), 0);
    with_installed_renderer(surface.renderer(), || {
        assert_eq!(count.get(), 1);
        owner.dispose();
    });
    assert_eq!(cleaned.get(), 1);
}

#[component]
fn stateful_token(saved: Rc<Cell<Option<RwSignal<u32>>>>, cleaned: Rc<Cell<u32>>) -> Element {
    let count = signal(0);
    saved.set(Some(count));
    let cleaned = cleaned.clone();
    on_cleanup(move || cleaned.set(cleaned.get() + 1));
    render! { Text(value: computed(move || format!("Read more {}", count.get()))) }
}

#[test]
fn truncation_rejects_nonparagraph_multiple_and_nested_placements() {
    for case in 0..4 {
        __reset_for_tests();
        let owner = Owner::new(None);
        let surface = SurfaceRuntime::new(SurfaceId::new(74).unwrap(), StyleEnvironment::default());
        with_installed_renderer(surface.renderer(), || {
            owner.with(|| {
                let root = match case {
                    0 => render! {
                        View {
                            InlineTruncation {
                                Text(value: "more")
                            }
                        }
                    },
                    1 => render! {
                        Text {
                            InlineTruncation {
                                Text(value: "one")
                            }
                            InlineTruncation {
                                Text(value: "two")
                            }
                        }
                    },
                    2 => render! {
                        Text {
                            Text {
                                InlineTruncation {
                                    Text(value: "nested")
                                }
                            }
                        }
                    },
                    _ => render! {
                        InlineTruncation {
                            Text(value: "root")
                        }
                    },
                };
                set_root(root);
            })
        });
        assert!(
            matches!(
                surface.binding_error(),
                Some(RuntimeBindingError::InvalidParagraph { .. })
            ),
            "case {case}"
        );
        with_installed_renderer(surface.renderer(), || owner.dispose());
    }
}
