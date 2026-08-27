use std::convert::Infallible;

use whisker::css::{
    Angle, Animation, AnimationFillMode, BorderRadius, BorderStyle, Clear, CustomPropertyName,
    Direction, EasingFunction, Float, GridLine, GridTemplate, GridTrack, ImageRendering, Keyframes,
    MotionPathCommand, MotionPathPoint, OffsetPath, OffsetRotate, Overflow, Position, Size,
    StyleProperty, TransformFn, Transition, TransitionPropertyKind,
};
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{
    BindType, create_element_by_name, set_attribute_bool, set_event_listener, set_root,
    try_invoke_element_method, with_installed_renderer,
};
use whisker::{
    ElementModuleDefinition, ElementProviderMetadata, ElementRegistry, ElementTag,
    RuntimeBindingError, SurfaceRuntime,
};
use whisker_engine::RecordingRenderer;
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    CommandId, ElementCommandSchema, ElementEventSchema, ElementMeasurement, ElementPropertySchema,
    ElementSchema, ElementValueKind, EventId, MeasureTextDirection, MeasuredSize,
    MeasurementMetrics, MeasurementPayload, MeasurementRequest, MeasurementResponse, Operation,
    PaintColor, PreparedContentId, PropertyId, SurfaceId, WhiskerValue,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{LayoutOptions, MeasurementProvider};

#[derive(Default)]
struct TextHost {
    calls: Vec<Vec<MeasurementRequest>>,
}

#[whisker::module_component(
    name = "whisker.test/AutoRegistered",
    measurement = None,
)]
fn auto_registered(enabled: Signal<bool>, style: whisker::Style) {}

#[whisker::module_component(
    name = "whisker.test/NativeLabel",
    measurement = Text,
)]
fn native_label(children: whisker::TextChildren) {}

impl MeasurementProvider for TextHost {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        self.calls.push(requests.to_vec());
        responses.extend(requests.iter().map(|request| {
            let MeasurementPayload::Text(payload) = &request.payload else {
                panic!("render! Text emitted a non-text measurement")
            };
            assert_eq!(payload.style.font_size, 20.0);
            MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics: MeasurementMetrics {
                    size: MeasuredSize::new(payload.text.chars().count() as f32 * 10.0, 24.0),
                    first_baseline: Some(18.0),
                    last_baseline: Some(18.0),
                    overflow: None,
                    prepared_content: PreparedContentId::new(request.key.get()),
                },
            }
        }));
        Ok(())
    }
}

#[test]
fn module_component_registers_its_schema_with_the_active_surface() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(11).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    assert!(
        surface
            .element_registrations()
            .iter()
            .all(|registration| registration.name != auto_registered_schema::NAME)
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                AutoRegistered(
                    enabled: true,
                    style: css!(width: px(40), height: px(20)),
                )
            }
        });
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);
    let registration = surface
        .element_registrations()
        .into_iter()
        .find(|registration| registration.name == auto_registered_schema::NAME)
        .expect("module component schema registered during authoring");

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("auto-registered module frame");
    assert!(renderer.frames()[0].packet.operations.iter().any(
        |operation| matches!(operation, Operation::CreateNode { element_type, .. } if *element_type == registration.element_type)
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn custom_plain_text_children_lower_to_measurement_and_set_text() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(15).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                NativeLabel(style: css!(font_size: px(20))) { "custom text" }
            }
        });
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let registration = surface
        .element_registrations()
        .into_iter()
        .find(|registration| registration.name == native_label_schema::NAME)
        .unwrap();
    assert_eq!(registration.child_policy, whisker::ChildPolicy::PlainText);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();

    assert!(renderer.frames()[0].packet.operations.iter().any(|operation| {
        matches!(operation, Operation::SetText { content, .. } if content.payload.text == "custom text")
    }));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn builtin_text_accepts_dynamic_into_view_text_children() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(17).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! { text(style: css!(font_size: px(20))) { { "dynamic".to_string() } } }
        });
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(renderer.frames()[0].packet.operations.iter().any(|operation| {
        matches!(operation, Operation::SetText { content, .. } if content.payload.text == "dynamic")
    }));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_text_reaches_measured_frame_and_paint_only_delta() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(7).expect("test surface"),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    );
    let value = owner.with(|| signal(String::from("hello")));
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: css!(color: Color::rgba(220, 30, 40, 0.75))) {
                    text(
                        value: value,
                        style: css!(font_size: px(20)),
                    )
                }
            }
        });
        set_root(root);
        root
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    let frame = surface
        .render_frame(
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("render! text frame");
    assert!(frame.layout.has_layout());
    assert!(matches!(
        frame.presentation,
        Some(whisker_engine::whisker_protocol::ApplyResult::Accepted { revision: 1 })
    ));
    let measurement_calls = host.calls.len();
    assert!(measurement_calls > 0);

    let snapshot = &renderer.frames()[0].packet;
    let text_node = snapshot
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetText { node, content } => {
                assert_eq!(content.payload.text, "hello");
                assert_eq!(content.payload.style.font_size, 20.0);
                assert_eq!(
                    content.paint.foreground,
                    PaintColor::Srgba {
                        red: 220,
                        green: 30,
                        blue: 40,
                        alpha: 0.75,
                    }
                );
                assert!(content.prepared_content.is_some());
                Some(*node)
            }
            _ => None,
        })
        .expect("snapshot SetText");
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::InsertChild { child, .. } if *child == text_node)
    ));
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::SetLayout { node, geometry } if *node == text_node && geometry.border_box.width == 50.0 && geometry.border_box.height == 24.0)
    ));
    with_installed_renderer(surface.renderer(), || {
        value.set(String::from("hello world"));
        whisker::flush();
    });
    surface
        .drive_layout(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("reactive render! text update");
    assert!(host.calls.len() > measurement_calls);
    assert!(matches!(
        surface.present(1, &mut renderer),
        Ok(Some(
            whisker_engine::whisker_protocol::ApplyResult::Accepted { revision: 2 }
        ))
    ));
    let text_delta = &renderer.frames()[1].packet;
    assert!(text_delta.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetText { node, content }
            if *node == text_node
                && content.payload.text == "hello world"
                && content.paint.foreground == PaintColor::Srgba {
                    red: 220,
                    green: 30,
                    blue: 40,
                    alpha: 0.75,
                }
                && content.prepared_content.is_some()
    )));
    assert!(text_delta.operations.iter().any(
        |operation| matches!(operation, Operation::SetLayout { node, geometry } if *node == text_node && geometry.border_box.width == 110.0 && geometry.border_box.height == 24.0)
    ));
    let measurement_calls = host.calls.len();

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root, Css::new().color(Color::rgba(20, 60, 230, 1.0)));
    });
    let progress = surface
        .drive_layout(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("paint-only update");
    assert!(progress.has_layout());
    assert_eq!(
        host.calls.len(),
        measurement_calls,
        "paint must not trigger measurement"
    );

    assert!(matches!(
        surface.present(1, &mut renderer),
        Ok(Some(
            whisker_engine::whisker_protocol::ApplyResult::Accepted { revision: 3 }
        ))
    ));
    let delta = &renderer.frames()[2].packet;
    assert!(matches!(
        delta.operations.as_slice(),
        [Operation::SetText { node, content }]
            if *node == text_node
                && content.paint.foreground == PaintColor::Srgba {
                    red: 20,
                    green: 60,
                    blue: 230,
                    alpha: 1.0,
                }
                && content.prepared_content.is_some()
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
    assert_eq!(surface.binding_error(), None);
}

#[test]
fn computed_css_direction_reaches_text_measurement_and_paint() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(22).unwrap(),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: css!(direction: Direction::Rtl)) {
                    text(
                        value: "مرحبا Whisker",
                        style: css!(font_size: px(20)),
                    )
                }
            }
        });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();

    let measured_direction =
        host.calls
            .iter()
            .flatten()
            .find_map(|request| match &request.payload {
                MeasurementPayload::Text(payload) => Some(payload.direction),
                _ => None,
            });
    assert_eq!(measured_direction, Some(MeasureTextDirection::RightToLeft));
    assert!(
        renderer.frames()[0]
            .packet
            .operations
            .iter()
            .any(|operation| {
                matches!(
                    operation,
                    Operation::SetText { content, .. }
                        if content.payload.direction == MeasureTextDirection::RightToLeft
                )
            })
    );

    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn opacity_transition_is_sampled_in_rust_and_emitted_as_ordinary_frame_deltas() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(18).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    let transition = || {
        Transition::new(TransitionPropertyKind::name("opacity"))
            .duration(100.ms())
            .timing(EasingFunction::Linear)
    };
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: Css::new()
                    .width(px(40))
                    .height(px(20))
                    .opacity(0.2)
                    .transition(transition()))
            }
        });
        set_root(root);
        root
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("initial frame");
    assert!(
        !surface.has_active_motion(),
        "initial style must not animate"
    );

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(
            root,
            Css::new()
                .width(px(40))
                .height(px(20))
                .opacity(1.0)
                .transition(transition()),
        );
    });
    assert!(surface.has_active_motion());
    assert!(surface.step_motion(1_000.0).expect("start transition"));
    assert!(surface.step_motion(1_050.0).expect("sample midpoint"));
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("midpoint frame");
    assert!(renderer.frames()[1]
        .packet
        .operations
        .iter()
        .any(|operation| matches!(operation, Operation::SetOpacity { opacity, .. } if (*opacity - 0.6).abs() < 0.0001)));

    assert!(!surface.step_motion(1_100.0).expect("finish transition"));
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("final frame");
    assert!(renderer.frames()[2].packet.operations.iter().any(
        |operation| matches!(operation, Operation::SetOpacity { opacity, .. } if *opacity == 1.0)
    ));
    assert!(!surface.has_active_motion());
    assert_eq!(
        surface.step_motion(f64::NAN),
        Err(RuntimeBindingError::InvalidMotionTimestamp)
    );
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn layout_transition_is_sampled_before_taffy_and_emits_geometry_deltas() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(23).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    let transition = || {
        Transition::new(TransitionPropertyKind::name("width"))
            .duration(100.ms())
            .timing(EasingFunction::Linear)
    };
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: Css::new()
                    .width(px(40))
                    .height(px(20))
                    .transition(transition()))
            }
        });
        set_root(root);
        root
    });
    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(
            root,
            Css::new()
                .width(px(80))
                .height(px(20))
                .transition(transition()),
        );
    });
    assert!(surface.step_motion(2_000.0).unwrap());
    assert!(surface.step_motion(2_050.0).unwrap());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(renderer.frames()[1].packet.operations.iter().any(
        |operation| matches!(operation, Operation::SetLayout { geometry, .. } if (geometry.border_box.width - 60.0).abs() < 0.0001)
    ));
    assert!(!surface.step_motion(2_100.0).unwrap());
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn box_color_transitions_are_composited_into_one_set_box_paint_delta() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(19).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    let transition = || {
        Transition::new(TransitionPropertyKind::All)
            .duration(100.ms())
            .timing(EasingFunction::Linear)
    };
    let painted = |background, border| {
        Css::new()
            .width(px(40))
            .height(px(20))
            .background_color(background)
            .border_top_width(px(2))
            .border_right_width(px(2))
            .border_bottom_width(px(2))
            .border_left_width(px(2))
            .border_top_style(BorderStyle::Solid)
            .border_right_style(BorderStyle::Solid)
            .border_bottom_style(BorderStyle::Solid)
            .border_left_style(BorderStyle::Solid)
            .border_top_color(border)
            .border_right_color(border)
            .border_bottom_color(border)
            .border_left_color(border)
            .transition(transition())
    };
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: painted(Color::rgb(0, 0, 0), Color::rgb(255, 0, 0)))
            }
        });
        set_root(root);
        root
    });
    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(
            root,
            painted(Color::rgb(255, 255, 255), Color::rgb(0, 0, 255)),
        );
    });
    assert!(surface.step_motion(2_000.0).unwrap());
    assert!(surface.step_motion(2_050.0).unwrap());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    let midpoint = renderer.frames()[1]
        .packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetBoxPaint { paint, .. } => Some(paint),
            _ => None,
        })
        .expect("transitioned box paint delta");
    assert_eq!(
        midpoint.background_color,
        PaintColor::Srgba {
            red: 128,
            green: 128,
            blue: 128,
            alpha: 1.0,
        }
    );
    for color in [
        &midpoint.border_colors.top,
        &midpoint.border_colors.right,
        &midpoint.border_colors.bottom,
        &midpoint.border_colors.left,
    ] {
        assert_eq!(
            color,
            &PaintColor::Srgba {
                red: 128,
                green: 0,
                blue: 128,
                alpha: 1.0,
            }
        );
    }
    assert!(!surface.step_motion(2_100.0).unwrap());

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(
            root,
            painted(NamedColor::Red.into(), NamedColor::Lime.into()),
        );
    });
    assert!(surface.has_active_motion());
    assert!(surface.step_motion(3_000.0).unwrap());
    assert!(surface.step_motion(3_050.0).unwrap());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(
        renderer.frames()[2]
            .packet
            .operations
            .iter()
            .any(|operation| {
                matches!(
                    operation,
                    Operation::SetBoxPaint { paint, .. }
                        if paint.background_color == PaintColor::Srgba {
                            red: 255,
                            green: 128,
                            blue: 128,
                            alpha: 1.0,
                        }
                            && paint.border_colors.top == PaintColor::Srgba {
                                red: 0,
                                green: 128,
                                blue: 128,
                                alpha: 1.0,
                            }
                )
            })
    );
    assert!(!surface.step_motion(3_100.0).unwrap());
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn inherited_text_color_transition_is_sampled_on_its_parent() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(20).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    let colored = |color| {
        Css::new()
            .width(px(100))
            .height(px(40))
            .color(color)
            .transition(
                Transition::new(TransitionPropertyKind::name("color"))
                    .duration(100.ms())
                    .timing(EasingFunction::Linear),
            )
    };
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: colored(NamedColor::Black.into())) {
                    text(style: css!(font_size: px(20))) { "inherited" }
                    view(style: css!(color: Color::Named(NamedColor::Red))) {
                        text(style: css!(font_size: px(20))) { "blocked" }
                    }
                }
            }
        });
        set_root(root);
        root
    });
    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root, colored(NamedColor::White.into()));
    });
    assert!(surface.step_motion(4_000.0).unwrap());
    assert!(surface.step_motion(4_050.0).unwrap());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(
        renderer.frames()[1]
            .packet
            .operations
            .iter()
            .any(|operation| {
                matches!(
                    operation,
                    Operation::SetText { content, .. }
                        if content.payload.text == "inherited"
                            && content.paint.foreground == PaintColor::Srgba {
                                red: 128,
                                green: 128,
                                blue: 128,
                                alpha: 1.0,
                            }
                )
            })
    );
    assert!(
        !renderer.frames()[1]
            .packet
            .operations
            .iter()
            .any(|operation| {
                matches!(operation, Operation::SetText { content, .. } if content.payload.text == "blocked")
            })
    );
    assert!(!surface.step_motion(4_100.0).unwrap());
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn compatible_transform_transition_resolves_percentages_after_layout() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(21).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    let transformed = |translate, scale: Option<f32>| {
        let mut functions = vec![TransformFn::TranslateX(percent(translate).into())];
        if let Some(scale) = scale {
            functions.push(TransformFn::Scale(scale, scale));
        }
        Css::new()
            .width(px(40))
            .height(px(20))
            .transform(functions)
            .transform_origin(Position::Coords(px(0).into(), px(0).into()))
            .transition(
                Transition::new(TransitionPropertyKind::name("transform"))
                    .duration(100.ms())
                    .timing(EasingFunction::Linear),
            )
    };
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: transformed(0, None)) });
        set_root(root);
        root
    });
    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root, transformed(100, Some(2.0)));
    });
    assert!(surface.step_motion(5_000.0).unwrap());
    assert!(surface.step_motion(5_050.0).unwrap());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(
        renderer.frames()[1]
            .packet
            .operations
            .iter()
            .any(|operation| {
                matches!(
                    operation,
                    Operation::SetTransform { transform, .. }
                        if (transform.0[0] - 1.5).abs() < 0.0001
                            && (transform.0[5] - 1.5).abs() < 0.0001
                            && (transform.0[12] - 20.0).abs() < 0.0001
                )
            })
    );
    assert!(!surface.step_motion(5_100.0).unwrap());
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn builder_keyframes_are_sampled_in_rust_and_emit_frame_deltas() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(22).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    let fade = Keyframes::builder()
        .named("fade")
        .from(Css::new().opacity(0.0).width(px(20)))
        .to(Css::new().opacity(1.0).width(px(80)))
        .build()
        .unwrap();
    let _root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: Css::new()
                    .width(px(40))
                    .height(px(20))
                    .opacity(0.25)
                    .animation(
                        Animation::new(fade)
                            .duration(100.ms())
                            .timing(EasingFunction::Linear)
                            .fill_mode(AnimationFillMode::Forwards)
                    ))
            }
        });
        set_root(root);
        root
    });
    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(surface.has_active_motion());
    assert!(surface.step_motion(6_000.0).unwrap());
    assert!(surface.step_motion(6_050.0).unwrap());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(renderer.frames()[1].packet.operations.iter().any(
        |operation| matches!(operation, Operation::SetOpacity { opacity, .. } if (*opacity - 0.5).abs() < 0.0001)
    ));
    assert!(renderer.frames()[1].packet.operations.iter().any(
        |operation| matches!(operation, Operation::SetLayout { geometry, .. } if (geometry.border_box.width - 50.0).abs() < 0.0001)
    ));
    assert!(!surface.step_motion(6_100.0).unwrap());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(renderer.frames()[2].packet.operations.iter().any(
        |operation| matches!(operation, Operation::SetOpacity { opacity, .. } if *opacity == 1.0)
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

fn painted_box(background: Color) -> Css {
    Css::new()
        .width(px(120))
        .height(px(48))
        .background_color(background)
        .border_top_width(px(2))
        .border_right_width(px(2))
        .border_bottom_width(px(2))
        .border_left_width(px(2))
        .border_top_color(Color::rgb(10, 20, 30))
        .border_right_color(Color::rgb(10, 20, 30))
        .border_bottom_color(Color::rgb(10, 20, 30))
        .border_left_color(Color::rgb(10, 20, 30))
        .border_top_style(BorderStyle::Solid)
        .border_right_style(BorderStyle::Solid)
        .border_bottom_style(BorderStyle::Solid)
        .border_left_style(BorderStyle::Solid)
        .border_radius(px(8))
        .overflow(Overflow::Hidden)
        .opacity(0.75)
        .z_index(4)
}

#[test]
fn render_box_paint_and_clip_reach_the_frame_sink() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(8).expect("test surface"),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    );
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: painted_box(Color::rgb(200, 30, 40))) });
        set_root(root);
        root
    });
    let mut host = TextHost::default();
    surface
        .drive_layout(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("box layout");
    assert!(host.calls.is_empty());

    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .present(1, &mut renderer)
        .expect("present painted snapshot")
        .expect("painted snapshot exists");
    let root_node = surface.root().expect("surface root");
    let snapshot = &renderer.frames()[0].packet;
    assert!(snapshot.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetBoxPaint { node, paint }
            if *node == root_node
                && paint.background_color == PaintColor::Srgba {
                    red: 200,
                    green: 30,
                    blue: 40,
                    alpha: 1.0,
                }
                && paint.border_widths.top.length == 2.0
                && paint.border_radii.top_left.horizontal.length == 8.0
    )));
    assert!(snapshot.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetClip { node, clip }
            if *node == root_node
                && clip.horizontal == whisker_engine::whisker_protocol::OverflowClip::Hidden
                && clip.vertical == whisker_engine::whisker_protocol::OverflowClip::Hidden
    )));
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::SetOpacity { node, opacity } if *node == root_node && *opacity == 0.75)
    ));
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::SetZOrder { node, z_order } if *node == root_node && *z_order == 4)
    ));

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root, painted_box(Color::rgb(20, 60, 230)));
    });
    surface
        .drive_layout(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("paint-only box update");
    assert!(host.calls.is_empty());
    surface
        .present(1, &mut renderer)
        .expect("present box delta")
        .expect("box delta exists");
    assert!(matches!(
        renderer.frames()[1].packet.operations.as_slice(),
        [Operation::SetBoxPaint { node, paint }]
            if *node == root_node
                && paint.background_color == PaintColor::Srgba {
                    red: 20,
                    green: 60,
                    blue: 230,
                    alpha: 1.0,
                }
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_transform_and_origin_reach_the_frame_sink_after_layout() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(24).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    let style = Css::new()
        .width(px(40))
        .height(px(20))
        .transform(TransformFn::Scale(2.0, 2.0))
        .transform_origin(Position::Coords(percent(25).into(), percent(50).into()));
    let root_element = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: style) });
        set_root(root);
        root
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("transformed frame");

    let root = surface.root().expect("surface root");
    let packet = &renderer.frames()[0].packet;
    assert!(packet.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetTransform { node, transform }
            if *node == root
                && transform.0 == [
                    2.0, 0.0, 0.0, 0.0,
                    0.0, 2.0, 0.0, 0.0,
                    0.0, 0.0, 1.0, 0.0,
                    -10.0, -10.0, 0.0, 1.0,
                ]
    )));

    let resized = Css::new()
        .width(px(80))
        .height(px(20))
        .transform(TransformFn::Scale(2.0, 2.0))
        .transform_origin(Position::Coords(percent(25).into(), percent(50).into()));
    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root_element, resized)
    });
    surface
        .drive_layout(
            LayoutSize::new(100.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("resized transformed layout");
    surface
        .present(1, &mut renderer)
        .expect("present resized transform")
        .expect("resized transform delta exists");
    assert!(
        renderer.frames()[1]
            .packet
            .operations
            .iter()
            .any(|operation| matches!(
                operation,
                Operation::SetTransform { node, transform }
                    if *node == root && transform.0[12] == -20.0 && transform.0[13] == -10.0
            ))
    );

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root_element, Css::new().width(px(80)).height(px(20)))
    });
    surface
        .drive_layout(
            LayoutSize::new(100.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("clear transform");
    surface
        .present(1, &mut renderer)
        .expect("present cleared transform")
        .expect("cleared transform delta exists");
    assert!(renderer.frames()[2]
        .packet
        .operations
        .iter()
        .any(|operation| matches!(
            operation,
            Operation::SetTransform { node, transform }
                if *node == root && *transform == whisker_engine::whisker_protocol::Transform::IDENTITY
        )));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_projective_matrix3d_reaches_the_frame_sink() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(25).expect("test surface"),
        StyleEnvironment::new(100.0, 60.0, 1.0, 14.0),
    );
    let matrix = [
        1.0, 0.0, 0.0, 0.02, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let style = Css::new()
        .width(px(40))
        .height(px(20))
        .transform(TransformFn::Matrix3d(matrix))
        .transform_origin(Position::Coords(px(0).into(), px(0).into()));
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: style) });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 60.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("projective frame");
    let root = surface.root().expect("surface root");
    assert!(
        renderer.frames()[0]
            .packet
            .operations
            .iter()
            .any(|operation| matches!(
                operation,
                Operation::SetTransform { node, transform }
                    if *node == root && transform.0 == matrix
            ))
    );
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_lynx_perspective_is_lowered_into_the_current_node_transform() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(26).expect("test surface"),
        StyleEnvironment::new(100.0, 60.0, 1.0, 14.0),
    );
    let style = Css::new()
        .width(px(40))
        .height(px(20))
        .perspective(px(100))
        .transform(TransformFn::RotateY(Angle::Deg(60.0)))
        .transform_origin(Position::Coords(px(0).into(), px(0).into()));
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: style) });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 60.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("perspective frame");
    let root = surface.root().expect("surface root");
    let transform = renderer.frames()[0]
        .packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetTransform { node, transform } if *node == root => Some(*transform),
            _ => None,
        })
        .expect("perspective emits SetTransform");
    assert!((transform.0[3] - 3.0_f32.sqrt() / 200.0).abs() < 0.000_001);
    assert_eq!(transform.0[11], 0.0);
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_motion_path_is_baked_into_the_existing_transform_operation() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(27).expect("test surface"),
        StyleEnvironment::new(100.0, 70.0, 1.0, 14.0),
    );
    let style = Css::new()
        .width(px(20))
        .height(px(10))
        .offset_path(OffsetPath::path(vec![
            MotionPathCommand::MoveTo(MotionPathPoint::new(0.0, 0.0)),
            MotionPathCommand::LineTo(MotionPathPoint::new(40.0, 0.0)),
            MotionPathCommand::LineTo(MotionPathPoint::new(40.0, 30.0)),
        ]))
        .offset_distance(percent(75))
        .offset_rotate(OffsetRotate::Auto);
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: style) });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 70.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("motion-path frame");
    let root = surface.root().expect("surface root");
    assert!(
        renderer.frames()[0]
            .packet
            .operations
            .iter()
            .any(|operation| matches!(
                operation,
                Operation::SetTransform { node, transform }
                    if *node == root
                        && (transform.0[12] - 55.0).abs() < 0.000_01
                        && (transform.0[13] - 7.5).abs() < 0.000_01
            ))
    );
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_curved_motion_path_uses_rust_resolved_position_and_tangent() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(28).expect("test surface"),
        StyleEnvironment::new(90.0, 50.0, 1.0, 14.0),
    );
    let style = Css::new()
        .width(px(10))
        .height(px(10))
        .offset_path(OffsetPath::path(vec![
            MotionPathCommand::MoveTo(MotionPathPoint::new(0.0, 20.0)),
            MotionPathCommand::QuadraticTo {
                control: MotionPathPoint::new(0.0, 0.0),
                to: MotionPathPoint::new(20.0, 0.0),
            },
        ]))
        .offset_distance(percent(50))
        .offset_rotate(OffsetRotate::Auto);
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: style) });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(90.0, 50.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("curved motion-path frame");
    let root = surface.root().expect("surface root");
    let transform = renderer.frames()[0]
        .packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetTransform { node, transform } if *node == root => Some(*transform),
            _ => None,
        })
        .expect("curve emits SetTransform");
    assert!((transform.0[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.001);
    assert!((transform.0[1] + std::f32::consts::FRAC_1_SQRT_2).abs() < 0.001);
    assert!((transform.0[12] - 2.928_932_2).abs() < 0.001);
    assert!((transform.0[13] - 10.0).abs() < 0.001);
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_circle_motion_path_resolves_against_the_border_box() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(29).expect("test surface"),
        StyleEnvironment::new(150.0, 70.0, 1.0, 14.0),
    );
    let style = Css::new()
        .width(px(40))
        .height(px(20))
        .offset_path(OffsetPath::circle(percent(50)))
        .offset_distance(percent(25))
        .offset_rotate(OffsetRotate::Auto);
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: style) });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(150.0, 70.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("circle motion-path frame");
    let root = surface.root().expect("surface root");
    let transform = renderer.frames()[0]
        .packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetTransform { node, transform } if *node == root => Some(*transform),
            _ => None,
        })
        .expect("circle emits SetTransform");
    assert!((transform.0[0] + 1.0).abs() < 0.001);
    assert!((transform.0[5] + 1.0).abs() < 0.001);
    assert!((transform.0[12] - 60.0).abs() < 0.001);
    assert!((transform.0[13] - 45.811_39).abs() < 0.001);
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_rounded_inset_motion_path_uses_the_standard_clockwise_start() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(30).expect("test surface"),
        StyleEnvironment::new(250.0, 160.0, 1.0, 14.0),
    );
    let style = Css::new()
        .width(px(100))
        .height(px(60))
        .offset_path(OffsetPath::inset_round(
            px(10),
            px(10),
            px(10),
            px(10),
            BorderRadius::all(px(10)),
        ))
        .offset_distance(percent(50))
        .offset_rotate(OffsetRotate::Auto);
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: style) });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(250.0, 160.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("rounded inset motion-path frame");
    let root = surface.root().expect("surface root");
    let transform = renderer.frames()[0]
        .packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetTransform { node, transform } if *node == root => Some(*transform),
            _ => None,
        })
        .expect("inset emits SetTransform");
    assert!((transform.0[0] + 1.0).abs() < 0.001);
    assert!((transform.0[5] + 1.0).abs() < 0.001);
    assert!((transform.0[12] - 180.0).abs() < 0.001);
    assert!((transform.0[13] - 110.0).abs() < 0.001);
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_svg_arc_motion_path_resolves_endpoint_flags_in_rust() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(31).expect("test surface"),
        StyleEnvironment::new(180.0, 120.0, 1.0, 14.0),
    );
    let style = Css::new()
        .width(px(40))
        .height(px(20))
        .offset_path(OffsetPath::path(vec![
            MotionPathCommand::MoveTo(MotionPathPoint::new(0.0, 0.0)),
            MotionPathCommand::ArcTo {
                radius_x: 50.0,
                radius_y: 50.0,
                x_axis_rotation: 0.0,
                large_arc: false,
                sweep: false,
                to: MotionPathPoint::new(100.0, 0.0),
            },
        ]))
        .offset_distance(percent(50))
        .offset_rotate(OffsetRotate::Auto);
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: style) });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(180.0, 120.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("SVG arc motion-path frame");
    let root = surface.root().expect("surface root");
    let transform = renderer.frames()[0]
        .packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetTransform { node, transform } if *node == root => Some(*transform),
            _ => None,
        })
        .expect("SVG arc emits SetTransform");
    assert!((transform.0[0] + 1.0).abs() < 0.001);
    assert!((transform.0[5] + 1.0).abs() < 0.001);
    assert!((transform.0[12] - 90.0).abs() < 0.001);
    assert!((transform.0[13] - 70.0).abs() < 0.001);
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_logical_borders_reach_physical_frame_edges_in_rtl() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(21).expect("test surface"),
        StyleEnvironment::new(100.0, 50.0, 1.0, 14.0),
    );
    let style = Css::new()
        .direction(Direction::Rtl)
        .width(px(100))
        .height(px(50))
        .border_inline_start_width(px(7))
        .border_inline_end_width(px(3))
        .border_inline_start_style(BorderStyle::Solid)
        .border_inline_end_style(BorderStyle::Dotted)
        .border_inline_start_color(Color::rgb(10, 20, 30))
        .border_inline_end_color(Color::rgb(40, 50, 60))
        .border_start_start_radius(px(11))
        .border_start_end_radius(px(12))
        .border_end_start_radius(px(13))
        .border_end_end_radius(px(14));
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: style) });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 50.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    let root = surface.root().unwrap();
    let packet = &renderer.frames()[0].packet;
    assert!(packet.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetLayout { node, geometry }
            if *node == root
                && geometry.content_box.x == 3.0
                && geometry.content_box.width == 90.0
    )));
    assert!(packet.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetBoxPaint { node, paint }
            if *node == root
                && paint.border_widths.left.length == 3.0
                && paint.border_widths.right.length == 7.0
                && paint.border_colors.left == PaintColor::Srgba { red: 40, green: 50, blue: 60, alpha: 1.0 }
                && paint.border_colors.right == PaintColor::Srgba { red: 10, green: 20, blue: 30, alpha: 1.0 }
                && paint.border_radii.top_left.horizontal.length == 12.0
                && paint.border_radii.top_right.horizontal.length == 11.0
                && paint.border_radii.bottom_right.horizontal.length == 13.0
                && paint.border_radii.bottom_left.horizontal.length == 14.0
    )));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_grid_layout_reaches_frame_packet_geometry() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(18).expect("test surface"),
        StyleEnvironment::new(200.0, 50.0, 1.0, 14.0),
    );
    let root_style = Css::new()
        .display_grid()
        .width(px(200))
        .height(px(50))
        .grid_template_columns(GridTemplate::tracks([
            GridTrack::fixed(px(50)),
            GridTrack::fraction(1.0),
        ]))
        .grid_template_rows(GridTemplate::tracks([GridTrack::fixed(px(50))]));
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: root_style) {
                    view(style: Css::new().grid_column(GridLine::Number(1), GridLine::Number(2)))
                    view(style: Css::new().grid_column(GridLine::Number(2), GridLine::Number(3)))
                }
            }
        });
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(200.0, 50.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("render! grid frame");
    assert!(host.calls.is_empty());

    let root_node = surface.root().expect("grid root");
    let packet = &renderer.frames()[0].packet;
    let child_nodes: Vec<_> = packet
        .operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::InsertChild { parent, child, .. } if *parent == root_node => Some(*child),
            _ => None,
        })
        .collect();
    assert_eq!(child_nodes.len(), 2);
    let geometry = |node| {
        packet
            .operations
            .iter()
            .find_map(|operation| match operation {
                Operation::SetLayout {
                    node: candidate,
                    geometry,
                } if *candidate == node => Some(geometry.border_box),
                _ => None,
            })
    };
    let first = geometry(child_nodes[0]).expect("first Grid item geometry");
    let second = geometry(child_nodes[1]).expect("second Grid item geometry");
    assert_eq!((first.x, first.width), (0.0, 50.0));
    assert_eq!((second.x, second.width), (50.0, 150.0));

    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_block_float_and_clear_reach_frame_packet_geometry() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(19).expect("test surface"),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    );
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: Css::new().display_block().width(px(200))) {
                    view(style: Css::new().width(px(50)).height(px(40)).float(Float::Left))
                    view(style: Css::new().width(px(60)).height(px(30)).float(Float::Right))
                    view(style: Css::new().width(px(100)).height(px(10)).clear(Clear::Both))
                }
            }
        });
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("render! float frame");
    assert!(host.calls.is_empty());

    let root_node = surface.root().expect("float root");
    let packet = &renderer.frames()[0].packet;
    let child_nodes: Vec<_> = packet
        .operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::InsertChild { parent, child, .. } if *parent == root_node => Some(*child),
            _ => None,
        })
        .collect();
    assert_eq!(child_nodes.len(), 3);
    let geometry = |node| {
        packet
            .operations
            .iter()
            .find_map(|operation| match operation {
                Operation::SetLayout {
                    node: candidate,
                    geometry,
                } if *candidate == node => Some(geometry.border_box),
                _ => None,
            })
    };
    let left = geometry(child_nodes[0]).expect("left float geometry");
    let right = geometry(child_nodes[1]).expect("right float geometry");
    let cleared = geometry(child_nodes[2]).expect("cleared geometry");
    assert_eq!((left.x, left.y), (0.0, 0.0));
    assert_eq!((right.x, right.y), (140.0, 0.0));
    assert_eq!((cleared.x, cleared.y), (0.0, 40.0));

    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn frame_element_type_comes_from_the_surface_registry() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(10).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    let scroll_type = surface
        .element_registrations()
        .into_iter()
        .find(|registration| registration.name == "whisker.ui/ScrollView")
        .expect("standard ScrollView registration")
        .element_type;
    assert_ne!(
        scroll_type.get(),
        ElementTag::ScrollView as u32,
        "wire IDs must not depend on authoring-tag discriminants"
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { scroll_view() });
        set_root(root);
    });
    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("ScrollView frame");

    assert!(renderer.frames()[0].packet.operations.iter().any(
        |operation| matches!(operation, Operation::CreateNode { element_type, .. } if *element_type == scroll_type)
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn module_element_uses_the_same_retained_frame_path_as_builtins() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let registry = ElementRegistry::standard_builder()
        .register_module(ElementModuleDefinition::new(
            "example.maps",
            [ElementProviderMetadata::named(ElementSchema {
                name: "example.maps/Map".into(),
                child_policy: whisker::ChildPolicy::Elements,
                measurement: ElementMeasurement::None,
                properties: Vec::new(),
                events: Vec::new(),
                commands: Vec::new(),
            })],
        ))
        .build()
        .expect("valid module element registry");
    let map_type = registry
        .registration_for_name("example.maps/Map")
        .expect("map authoring binding")
        .element_type;
    let surface = SurfaceRuntime::with_element_registry(
        SurfaceId::new(12).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
        registry,
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| create_element_by_name("example.maps/Map"));
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("module element frame");

    assert!(renderer.frames()[0].packet.operations.iter().any(
        |operation| matches!(operation, Operation::CreateNode { element_type, .. } if *element_type == map_type)
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn external_element_properties_events_and_commands_share_the_retained_frame_path() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let checked = PropertyId::new(1).unwrap();
    let change = EventId::new(1).unwrap();
    let toggle = CommandId::new(1).unwrap();
    let registry = ElementRegistry::standard_builder()
        .register_provider(ElementProviderMetadata::named(ElementSchema {
            name: "whisker.test/Toggle".into(),
            child_policy: whisker::ChildPolicy::None,
            measurement: ElementMeasurement::None,
            properties: vec![ElementPropertySchema {
                property: checked,
                name: "checked".into(),
                value: ElementValueKind::Bool,
            }],
            events: vec![ElementEventSchema {
                event: change,
                name: "change".into(),
                detail: Some(ElementValueKind::Map),
            }],
            commands: vec![ElementCommandSchema {
                command: toggle,
                name: "toggle".into(),
                arguments: ElementValueKind::Null,
            }],
        }))
        .build()
        .unwrap();
    let surface = SurfaceRuntime::with_element_registry(
        SurfaceId::new(13).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
        registry,
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| create_element_by_name("whisker.test/Toggle"));
        set_attribute_bool(root, "checked", true);
        set_event_listener(root, "change", BindType::Bind, Box::new(|_| {}));
        assert_eq!(
            try_invoke_element_method(root, "toggle", WhiskerValue::args([])),
            Some(WhiskerValue::Null)
        );
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    let operations = &renderer.frames()[0].packet.operations;
    assert!(operations.iter().any(|operation| matches!(
        operation,
        Operation::SetProperty { property, value: WhiskerValue::Bool(true), .. }
            if *property == checked
    )));
    assert!(operations.iter().any(|operation| matches!(
        operation,
        Operation::SetEventMask { event_mask, .. } if *event_mask == 1
    )));
    assert!(operations.iter().any(|operation| matches!(
        operation,
        Operation::InvokeCommand {
            command,
            arguments: WhiskerValue::Null,
            ..
        } if *command == toggle
    )));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn text_leaf_contract_is_enforced_before_frame_generation() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(11).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );

    with_installed_renderer(surface.renderer(), || {
        owner.with(|| render! { text { view() } });
    });

    assert!(matches!(
        surface.binding_error(),
        Some(RuntimeBindingError::ChildrenNotAllowed { .. })
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn element_children_reject_raw_text_before_frame_generation() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(16).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );

    with_installed_renderer(surface.renderer(), || {
        owner.with(|| render! { view { "not implicitly wrapped" } });
    });

    assert!(matches!(
        surface.binding_error(),
        Some(RuntimeBindingError::InvalidRawTextParent { .. })
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn image_rendering_reaches_the_frame_protocol_from_render_macro() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(17).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: Css::new()
                    .width(px(40))
                    .height(px(40))
                    .image_rendering(ImageRendering::Pixelated))
            }
        });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(
        renderer.frames()[0]
            .packet
            .operations
            .iter()
            .any(|operation| {
                matches!(
                    operation,
                    Operation::SetVisualEffects { effects, .. }
                        if effects.image_rendering
                            == whisker_engine::whisker_protocol::ImageRendering::Pixelated
                )
            })
    );
}

#[test]
fn wpt_border_radius_sum_of_radii_001_reaches_layout_and_frame_protocol() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(9).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: Css::new()
                    .width(px(100))
                    .height(px(100))
                    .background_color(Color::rgba(0, 0, 0, 0.0))
                    .border_top_width(px(10))
                    .border_right_width(px(10))
                    .border_bottom_width(px(10))
                    .border_left_width(px(10))
                    .border_top_color(Color::rgb(0, 0, 0))
                    .border_right_color(Color::rgb(0, 0, 0))
                    .border_bottom_color(Color::rgb(0, 0, 0))
                    .border_left_color(Color::rgb(0, 0, 0))
                    .border_top_style(BorderStyle::Solid)
                    .border_right_style(BorderStyle::Solid)
                    .border_bottom_style(BorderStyle::Solid)
                    .border_left_style(BorderStyle::Solid)
                    .border_top_left_radius(px(60))
                    .border_top_right_radius(px(150))
                    .border_bottom_right_radius(px(30))
                    .border_bottom_left_radius(px(30)))
            }
        });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("WPT-derived rounded box frame");
    assert!(host.calls.is_empty());

    let root = surface.root().expect("surface root");
    let packet = &renderer.frames()[0].packet;
    assert!(packet.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetLayout { node, geometry }
            if *node == root
                && geometry.border_box.width == 100.0
                && geometry.border_box.height == 100.0
    )));
    assert!(packet.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetBoxPaint { node, paint }
            if *node == root
                && paint.border_widths.top.length == 10.0
                && paint.border_radii.top_left.horizontal.length == 60.0
                && paint.border_radii.top_right.horizontal.length == 150.0
                && paint.border_radii.bottom_right.horizontal.length == 30.0
                && paint.border_radii.bottom_left.horizontal.length == 30.0
    )));

    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn inherited_custom_property_reaches_taffy_and_frame_protocol() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(19).expect("test surface"),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    );
    let card_width = CustomPropertyName::new("--card-width").unwrap();
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: Css::new()
                    .width(px(200))
                    .height(px(100))
                    .custom_property(card_width.clone(), Size::from(px(72)))) {
                    view(style: Css::new()
                        .property_variable(StyleProperty::Width, card_width)
                        .height(px(20)))
                }
            }
        });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("custom property frame");

    let root = surface.root().expect("surface root");
    let packet = &renderer.frames()[0].packet;
    let child = packet
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::InsertChild { parent, child, .. } if *parent == root => Some(*child),
            _ => None,
        })
        .expect("child inserted below custom-property owner");
    assert!(packet.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetLayout { node, geometry }
            if *node == child
                && geometry.border_box.width == 72.0
                && geometry.border_box.height == 20.0
    )));
}
